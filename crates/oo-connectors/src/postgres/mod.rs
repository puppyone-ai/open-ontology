//! PostgreSQL v0.2 adapter. Legacy root APIs retain their admission contract.
//!
//! Each operation uses one TLS connection. Keyed chunks use short read-only
//! transactions. Physical ranges require a caller-owned exported snapshot;
//! import it with `with_snapshot` and keep its exporter alive until completion.
//! Plans belong to this engine instance, not a durable/cross-cycle checkpoint.

pub mod changes;
mod declared;
mod kingbase;
pub mod lsn;
pub mod opengauss;
mod plan;
mod probe;
mod read;
pub mod reconcile;
mod shape;
mod sqltext;
pub mod version;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::engine::*;
use crate::session::{self, cell_bool, cell_i64, cell_opt_string, cell_string};
use crate::{PgConnector, PgSession, PgSourceConfig, SourceCatalog, SourceError};
use sqltext::{CompareForm, KeyCol};
use sqlx::{Connection, Row};

#[derive(Clone, Debug)]
pub struct Protection {
    pub active_session_limit: u32,
    pub connection_reserve: u32,
    pub pause_timeout: Duration,
    /// Other client sessions waiting on a heavyweight lock or an LWLock (row
    /// lock convoys, WAL write contention) at which a read backs off. Unlike
    /// the active-session limit this never fails a read: after
    /// `pause_timeout` the read proceeds.
    pub wait_limit: u32,
}
impl Default for Protection {
    fn default() -> Self {
        Self {
            active_session_limit: 16,
            connection_reserve: 5,
            pause_timeout: Duration::from_secs(30),
            wait_limit: 4,
        }
    }
}

/// Back-off before re-checking a source under lock/LWLock wait pressure:
/// doubles from 100 ms to at most 1.6 s.
fn wait_backoff(attempt: u32) -> Duration {
    Duration::from_millis(100 << attempt.min(4))
}

#[derive(Clone)]
struct Planned {
    plan: TableChunkPlan,
    relation: String,
    keys: Vec<KeyCol>,
    schema_hash: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ReadTiming {
    pub work_ns: u128,
    pub rest_ns: u128,
}

pub struct PostgresEngine {
    family: EngineFamily,
    product_version: Option<EngineVersion>,
    connector: PgConnector,
    session: Option<PgSession>,
    protection: Protection,
    snapshot: Option<String>,
    snapshot_owner: Option<PgSession>,
    plans: BTreeMap<TableRef, Planned>,
    revision: u32,
    /// Actual EXPLAIN output, suitable for a caller's evidence bundle.
    pub explains: BTreeMap<TableRef, String>,
    pub last_read_timing: Option<ReadTiming>,
    /// Digest computed in the successful chunk transaction, never a later read.
    pub last_chunk_digest: Option<RangeDigest>,
    /// Rows per C5 discovery digest statement, adapted to DIGEST_STEP_MS.
    digest_step: u64,
    /// Rows per keyed chunk part, adapted to READ_STEP_MS.
    read_step: u64,
}

/// Source time per C5 discovery digest statement. A 50,000-row range hashed
/// in one statement held a CPU for about 90 ms and showed in foreground p99.
const DIGEST_STEP_MS: u128 = 10;
const DIGEST_STEP_ROWS: (u64, u64) = (1024, 65_536);

/// Source time per keyed chunk part (rows, their digest and the boundary
/// probe in one short transaction).
const READ_STEP_MS: u128 = 25;

/// Rows for the next digest statement from the last one's measured rate.
fn next_digest_step(step: u64, rows: u64, elapsed: Duration) -> u64 {
    next_step(step, rows, elapsed, DIGEST_STEP_MS)
}

/// Rows for the next part so that it takes about `target_ms`.
fn next_step(step: u64, rows: u64, elapsed: Duration, target_ms: u128) -> u64 {
    let (min, max) = DIGEST_STEP_ROWS;
    if rows < step {
        // A short tail measures little; keep the current size.
        return step.clamp(min, max);
    }
    let rate = u128::from(rows) * 1_000_000 / elapsed.as_micros().max(1);
    u64::try_from(rate * target_ms / 1000)
        .unwrap_or(max)
        .clamp(min, max)
}

impl PostgresEngine {
    pub fn read_consistency(&self) -> &'static str {
        if self.snapshot.is_some() {
            "single"
        } else {
            "chunk_consistent"
        }
    }

    pub fn new(config: PgSourceConfig) -> Self {
        Self {
            family: EngineFamily::Postgres,
            product_version: None,
            connector: PgConnector::new(config),
            session: None,
            protection: Protection::default(),
            snapshot: None,
            snapshot_owner: None,
            plans: BTreeMap::new(),
            revision: 0,
            explains: BTreeMap::new(),
            last_read_timing: None,
            last_chunk_digest: None,
            digest_step: 8192,
            read_step: 8192,
        }
    }

    /// Kingbase PG mode shares the PostgreSQL transport and read algorithms.
    /// Product identity is checked on connection, not inferred from its port.
    pub fn new_kingbase(config: PgSourceConfig) -> Self {
        Self {
            family: EngineFamily::Kingbase,
            ..Self::new(config)
        }
    }

    pub fn with_protection(mut self, protection: Protection) -> Result<Self, SourceError> {
        if protection.active_session_limit == 0
            || protection.wait_limit == 0
            || protection.pause_timeout.is_zero()
            || protection.pause_timeout > Duration::from_secs(120)
        {
            return Err(SourceError::config("source_protection"));
        }
        self.protection = protection;
        Ok(self)
    }

    /// The exporting connection is owned by the cycle coordinator. Changing or
    /// closing that snapshot requires a new engine instance and new plans.
    pub fn with_snapshot(mut self, snapshot: String) -> Result<Self, SourceError> {
        if !self.plans.is_empty() || !valid_snapshot(&snapshot) {
            return Err(SourceError::protocol("snapshot"));
        }
        self.snapshot = Some(snapshot);
        Ok(self)
    }

    fn selected(&self, table: &TableRef) -> Result<(), SourceError> {
        if self
            .connector
            .source_config()
            .selection()
            .tables()
            .iter()
            .any(|t| t.schema() == table.schema && t.table() == table.name)
        {
            Ok(())
        } else {
            Err(SourceError::config("selected_table"))
        }
    }

    /// Keep physical locators alive only for their table, not the keyed binding.
    /// An explicitly supplied external snapshot continues to cover all tables.
    pub async fn prepare_table_snapshot(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<(), SourceError> {
        if self.snapshot.is_some() && self.snapshot_owner.is_none() {
            return Ok(());
        }
        if let Some(owner) = self.snapshot_owner.take() {
            self.snapshot = None;
            owner.conn.close().await.map_err(crate::error::map_sqlx)?;
        }
        self.plans.clear();
        let columns = self.export_shape(table, budget).await?;
        let columns = columns.as_deref().unwrap_or(&[]);
        let keyed = columns.iter().any(|c| c.primary_key_ordinal.is_some())
            || columns
                .iter()
                .flat_map(|c| c.unique_key_ordinals.keys())
                .any(|index| {
                    columns
                        .iter()
                        .filter(|c| c.unique_key_ordinals.contains_key(index))
                        .all(|c| !c.nullable)
                });
        if !keyed {
            let mut owner = self.connect().await?;
            owner.begin().await?;
            let row =
                session::fetch_one(&mut owner, sqlx::query("SELECT pg_export_snapshot()")).await?;
            self.snapshot = Some(cell_string(&row, 0)?);
            self.snapshot_owner = Some(owner);
        }
        Ok(())
    }

    async fn connect(&mut self) -> Result<PgSession, SourceError> {
        if let Some(mut session) = self.session.take() {
            session.ensure_open()?;
            session.budget = self.connector.source_config().budget();
            session.selection = self.connector.source_config().selection().clone();
            return Ok(session);
        }
        let mut s = self.connector.connect().await?;
        if !s.identity_aligned || !s.startup_clean {
            return Err(SourceError::config("session_identity"));
        }
        if !(12..=18).contains(&s.major) {
            return Err(SourceError::version("version_untested"));
        }
        if self.family == EngineFamily::Kingbase {
            let row = session::fetch_one(
                &mut s,
                sqlx::query("SELECT version(), current_setting('database_mode')"),
            )
            .await?;
            self.product_version = Some(kingbase::identity(
                &cell_string(&row, 0)?,
                &cell_string(&row, 1)?,
                s.major,
            )?);
            s.kingbase = true;
        }
        session::fetch_control(&mut s, sqlx::query(
            "SELECT set_config('search_path','pg_catalog',false), set_config('TimeZone','UTC',false), set_config('DateStyle','ISO, YMD',false), \
             set_config('IntervalStyle','postgres',false), set_config('bytea_output','hex',false), \
             set_config('max_parallel_workers_per_gather','0',false)"
        )).await?;
        Ok(s)
    }

    async fn connect_table(&mut self, table: &TableRef) -> Result<PgSession, SourceError> {
        self.selected(table)?;
        let mut session = self.connect().await?;
        // Catalog fingerprints belong to this table, not every selected table.
        session.selection = crate::TableSelection::new(vec![crate::SelectedTable::new(
            &table.schema,
            &table.name,
        )?])?;
        Ok(session)
    }

    /// Release the cached reader before a separate control connection is used.
    /// The caller-owned snapshot exporter remains alive.
    pub async fn close_idle_session(&mut self) -> Result<(), SourceError> {
        if let Some(session) = self.session.take() {
            session.conn.close().await.map_err(crate::error::map_sqlx)?;
        }
        Ok(())
    }

    async fn transaction(&self, s: &mut PgSession) -> Result<(), SourceError> {
        s.begin().await?;
        if let Some(snapshot) = &self.snapshot {
            // PostgreSQL's SET TRANSACTION SNAPSHOT grammar takes a literal,
            // not a bind parameter. Only the strict server token alphabet is accepted.
            let sql = format!("SET TRANSACTION SNAPSHOT '{snapshot}'");
            let result = tokio::time::timeout(
                s.budget.statement_timeout(),
                sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql.clone())).execute(&mut s.conn),
            )
            .await;
            result
                .map_err(|_| SourceError::budget("statement_timeout"))?
                .map_err(crate::error::map_sqlx)?;
        }
        Ok(())
    }

    async fn protect(&self, s: &mut PgSession) -> Result<(), SourceError> {
        let end = Instant::now() + self.protection.pause_timeout;
        let mut waits = 0;
        loop {
            let r = session::fetch_control(s, sqlx::query(
                "SELECT (SELECT rolsuper OR pg_has_role(current_user, 'pg_read_all_stats', 'MEMBER') FROM pg_roles WHERE rolname=current_user), \
                 count(*) FILTER (WHERE state='active' AND pid<>pg_backend_pid())::int8, count(*)::int8, \
                 current_setting('max_connections')::int8, \
                 count(*) FILTER (WHERE state='active' AND pid<>pg_backend_pid() \
                   AND wait_event_type IN ('Lock','LWLock'))::int8 \
                 FROM pg_stat_activity WHERE backend_type='client backend'"
            )).await?;
            if !cell_bool(&r, 0)? {
                return Err(SourceError::privilege("pg_read_all_stats"));
            }
            let capacity = cell_i64(&r, 1)? < i64::from(self.protection.active_session_limit)
                && cell_i64(&r, 2)? + i64::from(self.protection.connection_reserve)
                    < cell_i64(&r, 3)?;
            let pressure = cell_i64(&r, 4)? >= i64::from(self.protection.wait_limit);
            if capacity && (!pressure || Instant::now() >= end) {
                return Ok(());
            }
            if Instant::now() >= end {
                return Err(SourceError::budget("active_sessions"));
            }
            if capacity {
                // Adaptive: a convoy is building on the source; give it room.
                tokio::time::sleep(wait_backoff(waits)).await;
                waits += 1;
            } else {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    /// A keyed chunk in key-ordered parts of about READ_STEP_MS of source
    /// work each. Every part's rows and its range digest are read in one short
    /// read-only transaction (the same snapshot in `single` mode), and parts
    /// are paced, so no statement holds a CPU or a snapshot (and the source's
    /// `xmin`) for the whole range. The chunk proof is the sum of the parts'
    /// digests, each bound to the payload rows of its own transaction.
    async fn read_parts(
        &mut self,
        mut s: PgSession,
        planned: &Planned,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt, SourceError> {
        let mut timing = ReadTiming {
            work_ns: 0,
            rest_ns: 0,
        };
        let mut stream: Option<read::ChunkStream> = None;
        let mut proof = read::DigestSum::default();
        let mut sub_lo = chunk.lo.clone();
        let mut low = None;
        let result = loop {
            if let Err(error) = self.protect(&mut s).await {
                break Err(error);
            }
            let start = Instant::now();
            let step = self.read_step;
            let part = async {
                if low.is_none() {
                    low = Some(position(&mut s).await?);
                }
                self.transaction(&mut s).await?;
                tokio::time::timeout(s.budget.statement_timeout(), async {
                    let first = stream.is_none();
                    if first {
                        stream = Some(read::ChunkStream::open(&mut s, planned, chunk).await?);
                    }
                    let reader = stream.as_mut().expect("opened above");
                    let next = read::boundary(
                        &mut s,
                        planned,
                        sub_lo.as_deref(),
                        chunk.hi.as_deref(),
                        step,
                    )
                    .await?;
                    let hi = next.as_deref().or(chunk.hi.as_deref());
                    let rows = reader
                        .read(&mut s, planned, sub_lo.as_deref(), hi, budget, sink)
                        .await?;
                    let columns = reader.columns.clone();
                    let digest =
                        read::digest_with(&mut s, planned, &columns, sub_lo.as_deref(), hi).await?;
                    if digest.rows != rows {
                        return Err(SourceError::protocol("chunk_proof_count"));
                    }
                    // The plan's columns must still hold when the last part ends.
                    if next.is_none() && !first {
                        read::digest_columns(&mut s, planned).await?;
                    }
                    Ok((next, digest))
                })
                .await
                .map_err(|_| SourceError::budget("client_timeout"))
                .and_then(|v| v)
            }
            .await;
            let part = match part {
                Ok(value) => tokio::time::timeout(Duration::from_secs(1), s.finish(Ok(value)))
                    .await
                    .map_err(|_| SourceError::protocol("cleanup_unconfirmed"))
                    .and_then(|v| v),
                // An interrupted stream cannot safely accept another command.
                Err(error) => Err(error),
            };
            let elapsed = start.elapsed();
            let rest = duty(start, budget.duty_cycle_percent).await;
            timing.work_ns += rest.work_ns;
            timing.rest_ns += rest.rest_ns;
            let (next, digest) = match part {
                Ok(value) => value,
                Err(error) => break Err(error),
            };
            if let Err(error) = proof.add(&digest) {
                break Err(error);
            }
            self.read_step = next_step(step, digest.rows, elapsed, READ_STEP_MS);
            match next {
                Some(next) => sub_lo = Some(next),
                None => break Ok(()),
            }
        };
        let result = match result {
            Ok(()) => match stream.take() {
                Some(stream) => stream.finish(budget, sink),
                None => Err(SourceError::protocol("arrow_ipc")),
            },
            Err(error) => Err(error),
        };
        let result = match result {
            Ok((rows, bytes, sha256)) => {
                let proof = proof.finish(&chunk.table, chunk.lo.as_deref(), chunk.hi.as_deref());
                if proof.rows != rows {
                    Err(SourceError::protocol("chunk_proof_count"))
                } else {
                    match position(&mut s).await {
                        Ok(high) => {
                            self.last_chunk_digest = Some(proof);
                            Ok(ChunkReceipt {
                                table: chunk.table.clone(),
                                plan_revision: chunk.plan_revision,
                                chunk_id: chunk.chunk_id,
                                rows,
                                bytes,
                                sha256,
                                low: low.flatten(),
                                high,
                            })
                        }
                        Err(error) => Err(error),
                    }
                }
            }
            Err(error) => Err(error),
        };
        if result.is_ok() {
            self.session = Some(s);
        } else {
            drop(s);
        }
        self.last_read_timing = Some(timing);
        result
    }

    /// Read a durable keyset partition (C5 keeps its baseline's ranges) after a
    /// live `plan_chunks` of the same table, key and strategy. Keyset bounds are
    /// key values, not snapshot-bound locators, so any durable partition of the
    /// key space stays readable; physical or changed-key plans are refused.
    /// An unchanged SingleRead plan needs no range remapping and is also safe.
    pub fn adopt_keyset_plan(&mut self, durable: &TableChunkPlan) -> Result<(), SourceError> {
        let planned = self
            .plans
            .get_mut(&durable.table)
            .ok_or_else(|| SourceError::protocol("unknown_plan"))?;
        if !adoptable(&planned.plan, durable) {
            return Err(SourceError::protocol("stale_chunk"));
        }
        planned.plan.chunks = durable.chunks.clone();
        Ok(())
    }

    pub async fn catalog(&mut self) -> Result<SourceCatalog, SourceError> {
        let mut s = self.connect().await?;
        self.protect(&mut s).await?;
        self.transaction(&mut s).await?;
        let result = s.collect_catalog().await;
        let result = s.finish(result).await;
        if result.is_ok() {
            self.session = Some(s);
        }
        result
    }

    async fn plan_inner(
        &mut self,
        s: &mut PgSession,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<TableChunkPlan, SourceError> {
        let rel = session::fetch_one(s, sqlx::query(
            "SELECT c.oid::int8, quote_ident(n.nspname)||'.'||quote_ident(c.relname), \
             c.reltuples::float8, pg_relation_size(c.oid)::int8 / current_setting('block_size')::int8, \
             c.relkind::text, c.relrowsecurity, EXISTS(SELECT 1 FROM pg_inherits WHERE inhparent=c.oid) \
             FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
             WHERE n.nspname=$1 AND c.relname=$2"
        ).bind(&table.schema).bind(&table.name)).await?;
        let oid = cell_i64(&rel, 0)?;
        let relation = cell_string(&rel, 1)?;
        if cell_bool(&rel, 5)? {
            return Err(SourceError::privilege("row_security"));
        }
        if !matches!(cell_string(&rel, 4)?.as_str(), "r" | "p") {
            return Err(SourceError::catalog("relkind"));
        }
        if cell_string(&rel, 4)? == "r" && cell_bool(&rel, 6)? {
            return Err(SourceError::catalog("inheritance_parent"));
        }
        let estimated: f64 = rel
            .try_get(2)
            .map_err(|_| SourceError::catalog("reltuples"))?;
        let rows = if estimated >= 0.0 {
            estimated as u64
        } else {
            let q = format!("EXPLAIN (FORMAT JSON) SELECT * FROM {relation}");
            let r =
                session::fetch_one(s, sqlx::query(sqlx_core::sql_str::AssertSqlSafe(q.clone())))
                    .await?;
            plan::explain_rows(&explain_text(&r)?)?
        };
        let blocks = cell_i64(&rel, 3)?.max(0) as u64;
        s.arm_rows(2048);
        let entries = session::fetch_all(s, sqlx::query(
            "SELECT ic.relname::text, i.indisprimary, (i.indpred IS NULL AND i.indexprs IS NULL \
             AND NOT EXISTS (SELECT 1 FROM unnest(i.indcollation) WITH ORDINALITY z(coll,ord) \
                 JOIN pg_attribute a2 ON a2.attrelid=i.indrelid AND a2.attnum=i.indkey[z.ord::int-1] \
                 WHERE z.coll<>a2.attcollation)), \
             a.attname::text, a.attnotnull, format_type(a.atttypid,a.atttypmod), \
             COALESCE(NULLIF(t.typbasetype,0),t.oid)::int8, t.typcategory::text, co.collname::text, quote_ident(a.attname) \
             FROM pg_index i JOIN pg_class ic ON ic.oid=i.indexrelid JOIN pg_am am ON am.oid=ic.relam \
             CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY k(attnum,ord) \
             JOIN pg_attribute a ON a.attrelid=i.indrelid AND a.attnum=k.attnum \
             JOIN pg_type t ON t.oid=a.atttypid LEFT JOIN pg_collation co ON co.oid=a.attcollation \
             WHERE i.indrelid=$1::bigint::oid AND i.indisunique AND i.indisvalid AND i.indisready \
             AND am.amname='btree' AND k.ord<=i.indnkeyatts ORDER BY i.indisprimary DESC,ic.relname,k.ord LIMIT 2048"
        ).bind(oid)).await?;
        if entries.len() >= 2048 {
            return Err(SourceError::budget("index_catalog"));
        }
        let mut candidates: Vec<plan::IndexCandidate> = Vec::new();
        for r in &entries {
            let name = cell_string(r, 0)?;
            if candidates.last().is_none_or(|c| c.name != name) {
                candidates.push(plan::IndexCandidate {
                    name,
                    primary: cell_bool(r, 1)?,
                    plain: cell_bool(r, 2)?,
                    columns: Vec::new(),
                });
            }
            candidates.last_mut().unwrap().columns.push(plan::IndexCol {
                name: cell_string(r, 3)?,
                not_null: cell_bool(r, 4)?,
                cast_type: cell_string(r, 5)?,
                base_oid: cell_i64(r, 6)?,
                category: cell_string(r, 7)?,
                collation: cell_opt_string(r, 8)?,
            });
        }
        let (key, index) = plan::choose_key(&candidates);
        let mut keys = Vec::new();
        if let Some(index) = &index {
            for c in &index.columns {
                let quoted = s.quote_one(&c.name).await?;
                sqltext::check_idents([quoted.as_str()])?;
                if !sqltext::valid_cast_type(&c.cast_type) {
                    return Err(SourceError::catalog("cast_type"));
                }
                keys.push(KeyCol {
                    name: c.name.clone(),
                    quoted,
                    cast_type: c.cast_type.clone(),
                    class: sqltext::KeyClass::from_oid(c.base_oid),
                    collation: c.collation.clone(),
                });
            }
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| SourceError::budget("plan_revision"))?;
        let target = plan::target_rows(budget.max_chunk_rows);
        let (strategy, chunks) = if keys.is_empty() {
            if version::tid_range_scan(s.major as u32) && cell_string(&rel, 4)? == "r" {
                if self.snapshot.is_none() {
                    return Err(SourceError::protocol("ctid_requires_exported_snapshot"));
                }
                (
                    ChunkStrategy::PhysicalRange {
                        kind: PhysicalRangeKind::Ctid,
                    },
                    plan::ctid_chunks(
                        table,
                        self.revision,
                        blocks,
                        rows.div_ceil(blocks.max(1)).max(1),
                        target,
                    )?,
                )
            } else {
                (
                    ChunkStrategy::SingleRead,
                    plan::keyset_chunks(table, self.revision, vec![], Some(rows))?,
                )
            }
        } else {
            let mut bounds = Vec::new();
            // A global min/max distribution factor cannot detect a dense tail.
            // Walk boundaries on the source even when reltuples is stale.
            {
                let mut lo: Option<Vec<KeyValue>> = None;
                loop {
                    let sql =
                        sqltext::boundary_probe(&relation, &keys, CompareForm::Row, lo.is_some());
                    let mut q = sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql.clone()));
                    if let Some(values) = &lo {
                        for v in values {
                            q = q.bind(sqltext::bind_text(v)?);
                        }
                    }
                    q = q.bind(
                        i64::try_from(target).map_err(|_| SourceError::budget("max_chunk_rows"))?,
                    );
                    s.arm_rows(1);
                    let found = session::fetch_all(s, q).await?;
                    let Some(r) = found.first() else {
                        break;
                    };
                    let value = keys
                        .iter()
                        .enumerate()
                        .map(|(i, k)| k.class.value(cell_opt_string(r, i)?))
                        .collect::<Result<Vec<_>, _>>()?;
                    if lo.as_ref() == Some(&value) {
                        return Err(SourceError::protocol("key_progress"));
                    }
                    lo = Some(value.clone());
                    bounds.push(value);
                    if bounds.len() >= 100_000 {
                        return Err(SourceError::budget("chunk_count"));
                    }
                }
            }
            // Verify the actual row-comparison shape without changing planner settings.
            if let Some(b) = bounds.first() {
                let (w, _) = sqltext::range_where(&keys, CompareForm::Row, true, false);
                let sql = format!(
                    "EXPLAIN (FORMAT JSON) {}",
                    sqltext::chunk_select(
                        &relation,
                        &keys.iter().map(|k| k.quoted.clone()).collect::<Vec<_>>(),
                        &w,
                        &keys
                    )
                );
                let mut q = sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql.clone()));
                for v in b {
                    q = q.bind(sqltext::bind_text(v)?);
                }
                let r = session::fetch_one(s, q).await?;
                let text = explain_text(&r)?;
                let note = plan::read_explain(&text, &index.as_ref().unwrap().name)?;
                self.explains.insert(table.clone(), text);
                if keys.len() > 1 && !note.key_index {
                    return Err(SourceError::protocol("composite_index_plan"));
                }
            }
            (
                ChunkStrategy::Keyset {
                    columns: keys.iter().map(|k| k.name.clone()).collect(),
                },
                plan::keyset_chunks(table, self.revision, bounds, Some(rows))?,
            )
        };
        if matches!(strategy, ChunkStrategy::PhysicalRange { .. }) && chunks.len() > 1 {
            let sql = format!(
                "EXPLAIN (FORMAT JSON) SELECT * FROM {relation} WHERE ctid < CAST($1 AS tid)"
            );
            let hi = chunks[0]
                .hi
                .as_ref()
                .ok_or_else(|| SourceError::protocol("ctid_bound"))?;
            let r = session::fetch_one(
                s,
                sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql))
                    .bind(sqltext::bind_text(&hi[0])?),
            )
            .await?;
            let text = explain_text(&r)?;
            let note = plan::read_explain(&text, "")?;
            self.explains.insert(table.clone(), text);
            if note.node != "Tid Range Scan" {
                return Err(SourceError::protocol("ctid_range_plan"));
            }
        }
        let result = TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table: table.clone(),
            key,
            strategy,
            estimated_rows: Some(rows),
            chunks,
        };
        let schema_hash = s.collect_catalog().await?.schema_hash;
        self.plans.insert(
            table.clone(),
            Planned {
                plan: result.clone(),
                relation,
                keys,
                schema_hash,
            },
        );
        Ok(result)
    }
}

fn adoptable(live: &TableChunkPlan, durable: &TableChunkPlan) -> bool {
    (matches!(durable.strategy, ChunkStrategy::Keyset { .. })
        || (matches!(durable.strategy, ChunkStrategy::SingleRead) && live == durable))
        && live.protocol == durable.protocol
        && live.table == durable.table
        && live.key == durable.key
        && live.strategy == durable.strategy
        && !durable.chunks.is_empty()
        && durable.chunks.iter().all(|c| c.table == durable.table)
}

fn valid_snapshot(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

fn explain_text(r: &sqlx::postgres::PgRow) -> Result<String, SourceError> {
    // JSON feature is intentionally absent from SQLx; EXPLAIN's JSON payload
    // is UTF-8 on the wire, unlike jsonb it has no binary version prefix.
    let raw = r
        .try_get_raw(0)
        .map_err(|_| SourceError::protocol("explain"))?;
    let b = raw
        .as_bytes()
        .map_err(|_| SourceError::protocol("explain"))?;
    String::from_utf8(b.to_vec()).map_err(|_| SourceError::protocol("explain"))
}

fn validate_budget(b: &ReadBudget) -> Result<(), SourceError> {
    if b.max_chunk_rows == 0
        || b.max_chunk_rows > 100_000
        || b.max_chunk_bytes == 0
        || b.max_chunk_bytes > 32 * 1024 * 1024
        || !(1..=100).contains(&b.duty_cycle_percent)
        || b.statement_timeout_ms == 0
        || b.statement_timeout_ms > 120_000
    {
        return Err(SourceError::config("read_budget"));
    }
    Ok(())
}

async fn guards(s: &mut PgSession, b: &ReadBudget) -> Result<(), SourceError> {
    validate_budget(b)?;
    let old = s.budget;
    s.budget = crate::PgBudget::new(
        old.connect_timeout(),
        Duration::from_millis(b.statement_timeout_ms).min(old.statement_timeout()),
        old.lock_timeout(),
        old.max_rows(),
        old.max_bytes(),
    )?;
    let ms = s.budget.statement_timeout().as_millis().to_string();
    session::fetch_control(
        s,
        sqlx::query("SELECT set_config('statement_timeout',$1,false)").bind(ms),
    )
    .await?;
    Ok(())
}

async fn position(s: &mut PgSession) -> Result<Option<Watermark>, SourceError> {
    let r = session::fetch_control(s, sqlx::query("SELECT CASE WHEN pg_is_in_recovery() THEN pg_last_wal_replay_lsn()::text ELSE pg_current_wal_lsn()::text END")).await?;
    cell_opt_string(&r, 0)?
        .map(|v| lsn::parse_lsn(&v).map(|lsn| Watermark::PgLsn { lsn }))
        .transpose()
}

async fn duty(start: Instant, percent: u8) -> ReadTiming {
    let work = start.elapsed();
    let rest_start = Instant::now();
    tokio::time::sleep(work.mul_f64((100.0 - f64::from(percent)) / f64::from(percent))).await;
    ReadTiming {
        work_ns: work.as_nanos(),
        rest_ns: rest_start.elapsed().as_nanos(),
    }
}

impl SourceEngine for PostgresEngine {
    async fn column_shape(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> std::result::Result<Option<Vec<crate::engine::SourceColumnShape>>, SourceError> {
        self.export_shape(table, budget).await
    }

    async fn declared_catalog(
        &mut self,
        tables: &[TableRef],
        budget: &ReadBudget,
    ) -> Result<Option<DeclaredSourceCatalog>, SourceError> {
        self.export_declared(tables, budget).await
    }

    fn family(&self) -> EngineFamily {
        self.family
    }
    async fn probe(&mut self) -> Result<ProbeReport, SourceError> {
        let mut s = self.connect().await?;
        let result = probe::observe(&mut s).await.map(|mut report| {
            if let Some(version) = &self.product_version {
                report.settings.insert(
                    "postgres_compatibility_version".into(),
                    report.version.raw.clone(),
                );
                report.settings.insert("database_mode".into(), "pg".into());
                report.family = self.family;
                report.version = version.clone();
                report.capabilities.insert(
                    Capability::LogCdc,
                    CapabilityState::Unsupported {
                        reason: "V9R1C10 image has no pgoutput plugin; syslogical_output and \
                                 decoderbufs are not the PostgreSQL C1 protocol"
                            .into(),
                    },
                );
            }
            report
        });
        if result.is_ok() {
            self.session = Some(s);
        }
        result
    }
    async fn plan_chunks(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<TableChunkPlan, SourceError> {
        self.selected(table)?;
        validate_budget(budget)?;
        let mut s = self.connect_table(table).await?;
        guards(&mut s, budget).await?;
        self.protect(&mut s).await?;
        let start = Instant::now();
        self.transaction(&mut s).await?;
        let result = self.plan_inner(&mut s, table, budget).await;
        let result = s.finish(result).await;
        if result.is_ok() {
            self.session = Some(s);
        } else {
            drop(s);
        }
        duty(start, budget.duty_cycle_percent).await;
        result
    }
    async fn split_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
    ) -> Result<Option<Vec<ChunkSpec>>, SourceError> {
        self.selected(&chunk.table)?;
        validate_budget(budget)?;
        let planned = self
            .plans
            .get(&chunk.table)
            .ok_or_else(|| SourceError::protocol("unknown_plan"))?
            .clone();
        if !planned.plan.chunks.contains(chunk) {
            return Err(SourceError::protocol("stale_chunk"));
        }
        if planned.keys.is_empty() {
            return Ok(None);
        }
        let first_id = planned
            .plan
            .chunks
            .iter()
            .map(|c| c.chunk_id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| SourceError::budget("chunk_id"))?;
        let revision = chunk
            .plan_revision
            .checked_add(1)
            .ok_or_else(|| SourceError::budget("plan_revision"))?;
        let mut s = self.connect_table(&chunk.table).await?;
        guards(&mut s, budget).await?;
        self.protect(&mut s).await?;
        let start = Instant::now();
        self.transaction(&mut s).await?;
        let result = async {
            let keys = &planned.keys;
            let sql = sqltext::bounded_boundary_probe(
                &planned.relation,
                keys,
                CompareForm::Row,
                chunk.lo.is_some(),
                chunk.hi.is_some(),
            );
            let mut offset = budget.max_chunk_rows.div_ceil(2).max(1);
            let middle = loop {
                let mut q = sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql.clone()));
                for bound in [&chunk.lo, &chunk.hi].into_iter().flatten() {
                    if bound.len() != keys.len() {
                        return Err(SourceError::protocol("chunk_bound_width"));
                    }
                    for value in bound {
                        q = q.bind(sqltext::bind_text(value)?);
                    }
                }
                q = q.bind(
                    i64::try_from(offset).map_err(|_| SourceError::budget("max_chunk_rows"))?,
                );
                s.arm_rows(1);
                let rows = session::fetch_all(&mut s, q).await?;
                if let Some(row) = rows.first() {
                    break keys
                        .iter()
                        .enumerate()
                        .map(|(i, k)| k.class.value(cell_opt_string(row, i)?))
                        .collect::<Result<Vec<_>, _>>()?;
                }
                if offset == 1 {
                    return Err(SourceError::budget("single_row_cannot_split"));
                }
                offset = (offset / 2).max(1);
            };
            Ok(vec![
                ChunkSpec {
                    plan_revision: revision,
                    chunk_id: first_id,
                    hi: Some(middle.clone()),
                    estimated_rows: None,
                    ..chunk.clone()
                },
                ChunkSpec {
                    plan_revision: revision,
                    chunk_id: first_id
                        .checked_add(1)
                        .ok_or_else(|| SourceError::budget("chunk_id"))?,
                    lo: Some(middle),
                    estimated_rows: None,
                    ..chunk.clone()
                },
            ])
        }
        .await;
        let result = s.finish(result).await;
        if result.is_ok() {
            self.session = Some(s);
        } else {
            drop(s);
        }
        duty(start, budget.duty_cycle_percent).await;
        let children = result?;
        let registered = self.plans.get_mut(&chunk.table).unwrap();
        let index = registered
            .plan
            .chunks
            .iter()
            .position(|c| c == chunk)
            .unwrap();
        registered
            .plan
            .chunks
            .splice(index..=index, children.clone());
        Ok(Some(children))
    }

    async fn read_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt, SourceError> {
        self.last_read_timing = None;
        self.last_chunk_digest = None;
        self.selected(&chunk.table)?;
        validate_budget(budget)?;
        let planned = self
            .plans
            .get(&chunk.table)
            .ok_or_else(|| SourceError::protocol("unknown_plan"))?
            .clone();
        if !planned.plan.chunks.contains(chunk) {
            return Err(SourceError::protocol("stale_chunk"));
        }
        let mut s = self.connect_table(&chunk.table).await?;
        guards(&mut s, budget).await?;
        if planned.plan.key != KeyShape::NoKey
            && !planned.keys.is_empty()
            && !matches!(planned.plan.strategy, ChunkStrategy::PhysicalRange { .. })
        {
            return self.read_parts(s, &planned, chunk, budget, sink).await;
        }
        self.protect(&mut s).await?;
        let start = Instant::now();
        let low = position(&mut s).await?;
        let result = tokio::time::timeout(s.budget.statement_timeout(), async {
            self.transaction(&mut s).await?;
            let payload = read::chunk(&mut s, &planned, chunk, budget, sink).await?;
            let proof = if planned.plan.key != KeyShape::NoKey {
                Some(
                    read::digest(&mut s, &planned, chunk.lo.as_deref(), chunk.hi.as_deref())
                        .await?,
                )
            } else {
                None
            };
            if proof.as_ref().is_some_and(|d| d.rows != payload.0) {
                return Err(SourceError::protocol("chunk_proof_count"));
            }
            Ok((payload, proof))
        })
        .await
        .map_err(|_| SourceError::budget("client_timeout"))
        .and_then(|v| v);
        let result = match result {
            Ok(value) => tokio::time::timeout(Duration::from_secs(1), s.finish(Ok(value)))
                .await
                .map_err(|_| SourceError::protocol("cleanup_unconfirmed"))
                .and_then(|v| v),
            // An interrupted stream cannot safely accept another command.
            // Dropping the sole socket ends its read-only transaction.
            Err(err) => Err(err),
        };
        let result = match result {
            Ok(((rows, bytes, sha256), proof)) => {
                self.last_chunk_digest = proof;
                Ok(ChunkReceipt {
                    table: chunk.table.clone(),
                    plan_revision: chunk.plan_revision,
                    chunk_id: chunk.chunk_id,
                    rows,
                    bytes,
                    sha256,
                    low,
                    high: position(&mut s).await?,
                })
            }
            Err(e) => Err(e),
        };
        if result.is_ok() {
            self.session = Some(s);
        } else {
            drop(s);
        }
        self.last_read_timing = Some(duty(start, budget.duty_cycle_percent).await);
        result
    }
    async fn watermark(&mut self) -> Result<Option<Watermark>, SourceError> {
        let mut s = self.connect().await?;
        let result = position(&mut s).await;
        if result.is_ok() {
            self.session = Some(s);
        }
        result
    }
    async fn digest_range(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest, SourceError> {
        self.selected(table)?;
        let planned = self
            .plans
            .get(table)
            .ok_or_else(|| SourceError::protocol("unknown_plan"))?
            .clone();
        let mut s = self.connect_table(table).await?;
        // Keyed ranges are digested in key-ordered sub-ranges of about
        // DIGEST_STEP_MS each, paced in between, rather than one statement
        // that hashes the whole range at once. The digest is additive, so the
        // sum equals the single-statement digest of an unchanged range; each
        // sub-range is observed at its own instant, like ranges in a cycle.
        let keyed = !planned.keys.is_empty()
            && !matches!(planned.plan.strategy, ChunkStrategy::PhysicalRange { .. });
        let mut sub_lo = lo.map(<[_]>::to_vec);
        let mut sum = read::DigestSum::default();
        let mut columns: Option<Vec<String>> = None;
        let result = loop {
            if let Err(error) = self.protect(&mut s).await {
                break Err(error);
            }
            let start = Instant::now();
            if let Err(error) = self.transaction(&mut s).await {
                break Err(error);
            }
            let step = self.digest_step;
            let first = columns.is_none();
            // A dropped network must end the digest, not hang it: bound it like read_chunk.
            let part = tokio::time::timeout(s.budget.statement_timeout(), async {
                let cols = match &columns {
                    Some(cols) => cols.clone(),
                    None => read::digest_columns(&mut s, &planned).await?,
                };
                let next = if keyed {
                    read::boundary(&mut s, &planned, sub_lo.as_deref(), hi, step).await?
                } else {
                    None
                };
                let part = read::digest_with(
                    &mut s,
                    &planned,
                    &cols,
                    sub_lo.as_deref(),
                    next.as_deref().or(hi),
                )
                .await?;
                // The plan's columns must still hold when the last part ends.
                if next.is_none() && !first {
                    read::digest_columns(&mut s, &planned).await?;
                }
                Ok((cols, next, part))
            })
            .await
            .map_err(|_| SourceError::budget("client_timeout"))
            .and_then(|v| v);
            let part = match part {
                Ok(value) => tokio::time::timeout(Duration::from_secs(1), s.finish(Ok(value)))
                    .await
                    .map_err(|_| SourceError::protocol("cleanup_unconfirmed"))
                    .and_then(|v| v),
                Err(error) => Err(error),
            };
            let elapsed = start.elapsed();
            duty(start, 25).await;
            let (cols, next, part) = match part {
                Ok(value) => value,
                Err(error) => break Err(error),
            };
            columns = Some(cols);
            if let Err(error) = sum.add(&part) {
                break Err(error);
            }
            self.digest_step = next_digest_step(step, part.rows, elapsed);
            match next {
                Some(next) => sub_lo = Some(next),
                None => break Ok(std::mem::take(&mut sum).finish(table, lo, hi)),
            }
        };
        if result.is_ok() {
            self.session = Some(s);
        } else {
            drop(s);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_same_key_keyset_plans_are_adoptable() {
        let table = TableRef {
            schema: "public".into(),
            name: "bulk".into(),
        };
        let chunk = |hi: Option<i64>| ChunkSpec {
            table: table.clone(),
            plan_revision: 0,
            chunk_id: 0,
            lo: None,
            hi: hi.map(|v| vec![KeyValue::Int(v.into())]),
            estimated_rows: None,
        };
        let live = TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table: table.clone(),
            key: KeyShape::IntegerPrimaryKey {
                column: "id".into(),
            },
            strategy: ChunkStrategy::Keyset {
                columns: vec!["id".into()],
            },
            estimated_rows: Some(10),
            chunks: vec![chunk(None)],
        };
        let mut durable = live.clone();
        durable.chunks = vec![chunk(Some(5)), chunk(None)];
        assert!(adoptable(&live, &durable));
        let mut other_key = durable.clone();
        other_key.key = KeyShape::IntegerPrimaryKey { column: "n".into() };
        assert!(!adoptable(&live, &other_key));
        let mut physical = durable.clone();
        physical.strategy = ChunkStrategy::SingleRead;
        assert!(!adoptable(&live, &physical));
        let mut single = live.clone();
        single.strategy = ChunkStrategy::SingleRead;
        single.key = KeyShape::NoKey;
        assert!(adoptable(&single, &single));
        let mut changed_single = single.clone();
        changed_single.chunks[0].plan_revision += 1;
        assert!(!adoptable(&single, &changed_single));
        let mut empty = durable;
        empty.chunks.clear();
        assert!(!adoptable(&live, &empty));
    }

    #[test]
    fn sub_range_digests_add_up_to_the_whole_range_digest() {
        let table = TableRef {
            schema: "public".into(),
            name: "bulk".into(),
        };
        let part = |rows, high: i128, low: i128| RangeDigest {
            table: table.clone(),
            lo: None,
            hi: None,
            rows,
            digest: format!("pg-text-sha256-sum128/v1:{high}:{low}"),
        };
        let mut sum = read::DigestSum::default();
        sum.add(&part(3, i128::from(i64::MAX) * 3, -5)).unwrap();
        sum.add(&part(0, 0, 0)).unwrap();
        sum.add(&part(2, -7, i128::from(i64::MIN) * 2)).unwrap();
        let lo = [KeyValue::Int(1.into())];
        let whole = sum.finish(&table, Some(&lo), None);
        assert_eq!(whole.rows, 5);
        assert_eq!(whole.lo.as_deref(), Some(&lo[..]));
        assert_eq!(
            whole.digest,
            format!(
                "pg-text-sha256-sum128/v1:{}:{}",
                i128::from(i64::MAX) * 3 - 7,
                i128::from(i64::MIN) * 2 - 5
            )
        );
        // An empty range matches the statement's COALESCE(..., 0) text.
        assert_eq!(
            read::DigestSum::default().finish(&table, None, None).digest,
            "pg-text-sha256-sum128/v1:0:0"
        );
        for bad in [
            "pg-text-sha256-sum128/v1:1",
            "other/v1:1:2",
            "pg-text-sha256-sum128/v1:1.5:2",
            "pg-text-sha256-sum128/v1::2",
            "pg-text-sha256-sum128/v1:+1:2",
        ] {
            let mut sum = read::DigestSum::default();
            let mut digest = part(1, 0, 0);
            digest.digest = bad.into();
            assert!(sum.add(&digest).is_err(), "{bad}");
        }
    }

    #[test]
    fn wait_pressure_backs_off_exponentially_up_to_a_cap() {
        let backoff: Vec<_> = (0..7).map(|n| wait_backoff(n).as_millis()).collect();
        assert_eq!(backoff, [100, 200, 400, 800, 1600, 1600, 1600]);
        let defaults = Protection::default();
        assert!(defaults.wait_limit > 0 && defaults.wait_limit < defaults.active_session_limit);
    }

    #[test]
    fn digest_step_follows_the_measured_rate_within_bounds() {
        let ms = Duration::from_millis;
        // 8,192 rows in 20 ms: half that fits in 10 ms.
        assert_eq!(next_digest_step(8192, 8192, ms(20)), 4096);
        assert_eq!(next_digest_step(8192, 8192, ms(1)), DIGEST_STEP_ROWS.1);
        assert_eq!(next_digest_step(8192, 8192, ms(1000)), DIGEST_STEP_ROWS.0);
        // A short final part does not resize the step.
        assert_eq!(next_digest_step(8192, 10, ms(1)), 8192);
        assert_eq!(
            next_digest_step(8192, 8192, Duration::ZERO),
            DIGEST_STEP_ROWS.1
        );
    }

    #[test]
    fn bounds_and_snapshot_validation() {
        assert!(validate_budget(&ReadBudget::default()).is_ok());
        assert!(
            validate_budget(&ReadBudget {
                duty_cycle_percent: 100,
                ..ReadBudget::default()
            })
            .is_ok()
        );
        for b in [
            ReadBudget {
                max_chunk_rows: 0,
                ..ReadBudget::default()
            },
            ReadBudget {
                duty_cycle_percent: 101,
                ..ReadBudget::default()
            },
            ReadBudget {
                max_chunk_rows: 100_001,
                ..ReadBudget::default()
            },
            ReadBudget {
                max_chunk_bytes: 0,
                ..ReadBudget::default()
            },
            ReadBudget {
                max_chunk_bytes: 32 * 1024 * 1024 + 1,
                ..ReadBudget::default()
            },
            ReadBudget {
                statement_timeout_ms: 0,
                ..ReadBudget::default()
            },
            ReadBudget {
                statement_timeout_ms: 120_001,
                ..ReadBudget::default()
            },
        ] {
            assert!(validate_budget(&b).is_err());
        }
        assert!(valid_snapshot("00000003-0000001B-1"));
        for s in ["", "x' ; SELECT 1", "abc/def"] {
            assert!(!valid_snapshot(s));
        }
    }
}
