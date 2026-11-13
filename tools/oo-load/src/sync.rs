//! Bounded PostgreSQL C1. Snapshot repair/handoff is deliberately not inferred.
use super::*;
use oo_connectors::{
    engine::{ChangeBatch, ChangeSource, ChunkSink, Watermark},
    postgres::changes::{CaptureConfig, PostgresChanges},
};
use oo_pipeline::{
    engine_load::Capture, engine_sync, engine_sync_pg::PgSync, native_arrow::decode_arrow_stream,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    initial_position: Watermark,
    baseline_aligned: bool,
    slot: String,
    publication: String,
    required_wal_headroom: u64,
    max_batches: u32,
}

struct Target<'a, 's> {
    sink: PgSync<'a>,
    budget: ReadBudget,
    source: &'s Source,
    store: &'s SecretStore,
    lineage: String,
    tables: Vec<TableRef>,
    engine: PostgresEngine,
    admission: c1::Admission,
}
impl engine_sync::Target for Target<'_, '_> {
    async fn position(&mut self) -> engine_load::Result<Watermark> {
        self.sink.position().await
    }
    async fn feedback_position(&mut self) -> engine_load::Result<Watermark> {
        self.sink.feedback_position().await
    }
    async fn apply(&mut self, from: &Watermark, batch: &ChangeBatch) -> engine_load::Result<()> {
        // Recheck after capture and before target commit, not only after commit
        // while sending feedback. A detected epoch change must not enter data.
        let mut control = source_control_connection(self.source, self.store)
            .await
            .map_err(|_| "sync_lineage_control")?;
        if capture_lineage(&mut control, &self.tables).await? != self.lineage {
            return Err("sync_rebaseline_required_lineage");
        }
        control.close().await.map_err(|_| "sync_lineage_control")?;
        let shape = c1::check_shapes(&mut self.engine, &self.admission, &self.budget, batch).await;
        self.engine
            .close_idle_session()
            .await
            .map_err(|_| "sync_source_close")?;
        shape?;
        let mut images = Vec::new();
        let mut bytes = 0u64;
        for image in &batch.images {
            let mut sink = oo_load::IpcSink::new(&self.budget);
            sink.push(image.clone()).map_err(|_| "sync_image_budget")?;
            let ipc = sink.finish()?;
            bytes = bytes
                .checked_add(ipc.len() as u64)
                .ok_or("sync_image_budget")?;
            if bytes > self.budget.max_chunk_bytes.min(32 * 1024 * 1024) {
                return Err("sync_image_budget");
            }
            images.push(decode_arrow_stream(&ipc).map_err(|_| "sync_image_decode")?);
        }
        self.sink.apply(from, batch, &images).await
    }
}

async fn capture_lineage(
    conn: &mut PgConnection,
    tables: &[TableRef],
) -> engine_load::Result<String> {
    let mut value: serde_json::Value =
        serde_json::from_str(&lineage(conn, tables).await?).map_err(|_| "sync_lineage")?;
    for relation in value
        .get_mut(1)
        .and_then(|v| v.as_array_mut())
        .ok_or("sync_lineage")?
    {
        relation.as_array_mut().ok_or("sync_lineage")?.truncate(2);
    }
    serde_json::to_string(&value).map_err(|_| "sync_lineage")
}

struct Feedback {
    connection: PgConnection,
    slot: String,
    lineage: String,
    tables: Vec<TableRef>,
    duty: u8,
}

pub(super) async fn lineage(
    conn: &mut PgConnection,
    tables: &[TableRef],
) -> engine_load::Result<String> {
    super::bounded(
        super::SOURCE_CONTROL_TIMEOUT,
        lineage_unbounded(conn, tables),
        "sync_lineage_timeout",
    )
    .await
}

async fn lineage_unbounded(
    conn: &mut PgConnection,
    tables: &[TableRef],
) -> engine_load::Result<String> {
    let system: (String, i32, i64) = sqlx::query_as(
        "SELECT s.system_identifier::text,c.timeline_id,d.oid::bigint
         FROM pg_control_system() s,pg_control_checkpoint() c,pg_database d
         WHERE d.datname=current_database()",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|_| "sync_lineage_permission_required")?;
    let mut relations = Vec::new();
    for table in tables {
        let oid: i64 = sqlx::query_scalar(
            "SELECT c.oid::bigint FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
             WHERE n.nspname=$1 AND c.relname=$2",
        )
        .bind(&table.schema)
        .bind(&table.name)
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| "sync_rebaseline_required_relation")?;
        // Column names/types/nullability and primary key: metadata-only DDL
        // (rename, widen) changes no row or range digest, so it must change lineage.
        let columns: String = sqlx::query_scalar(
            "SELECT COALESCE(json_agg(json_build_array(a.attname::text,
               format_type(a.atttypid,a.atttypmod),a.attnotnull,
               COALESCE(a.attnum=ANY(i.indkey),false)) ORDER BY a.attnum)::text,'[]')
             FROM pg_attribute a LEFT JOIN pg_index i ON i.indrelid=a.attrelid AND i.indisprimary
             WHERE a.attrelid=$1::oid AND a.attnum>0 AND NOT a.attisdropped",
        )
        .bind(oid)
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| "sync_lineage_permission_required")?;
        relations.push((table, oid, columns));
    }
    serde_json::to_string(&(system, relations)).map_err(|_| "sync_lineage")
}

impl engine_sync::Feedback for Feedback {
    async fn acknowledge(&mut self, durable: &Watermark) -> engine_load::Result<()> {
        let started = std::time::Instant::now();
        let result = super::bounded(
            super::SOURCE_CONTROL_TIMEOUT,
            async {
                if capture_lineage(&mut self.connection, &self.tables).await? != self.lineage {
                    return Err("sync_rebaseline_required_lineage");
                }
                let Watermark::PgLsn { lsn } = durable else {
                    return Err("sync_position_kind");
                };
                let high: String = sqlx::query_scalar("SELECT pg_current_wal_lsn()::text")
                    .fetch_one(&mut self.connection)
                    .await
                    .map_err(|_| "sync_source_position")?;
                if *lsn
                    > oo_connectors::postgres::lsn::parse_lsn(&high)
                        .map_err(|_| "sync_source_position")?
                {
                    return Err("sync_rebaseline_required_future_position");
                }
                let lsn = format!("{:X}/{:X}", lsn >> 32, lsn & 0xffff_ffff);
                // This is replication progress feedback, never business DML. Only
                // the durable target position may release retained source WAL.
                let actual: String = sqlx::query_scalar(
                    "SELECT end_lsn::text FROM pg_replication_slot_advance($1,$2::pg_lsn)",
                )
                .bind(&self.slot)
                .bind(&lsn)
                .fetch_one(&mut self.connection)
                .await
                .map_err(|_| "sync_feedback")?;
                if actual != lsn {
                    return Err("sync_feedback_position");
                }
                Ok(())
            },
            "sync_feedback_timeout",
        )
        .await;
        tokio::time::sleep(started.elapsed() * (100 - u32::from(self.duty)) / u32::from(self.duty))
            .await;
        result
    }
}

pub(super) async fn run(
    config: PgSourceConfig,
    source: &Source,
    tables: &[TableRef],
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    sync: Config,
) -> Result<(), String> {
    let setup_started = std::time::Instant::now();
    if !sync.baseline_aligned
        || sync.max_batches == 0
        || sync.max_batches > 10_000
        || !(1..=100).contains(&budget.duty_cycle_percent)
        || !matches!(sync.initial_position, Watermark::PgLsn { .. })
    {
        return Err("sync_requires_aligned_baseline_and_bounded_batches".into());
    }
    let mut feedback_conn = source_control_connection(source, store).await?;
    let mut tables = tables.to_vec();
    tables.sort();
    let live_lineage = capture_lineage(&mut feedback_conn, &tables).await?;
    let binding = super::binding(source)?;
    let durable_lineage = serde_json::to_string(&(&live_lineage, &sync.slot, &sync.publication))
        .map_err(|_| "sync_lineage")?;
    // Pre-0129 bindings stored relation shapes in the lineage (see c1::prepare).
    let legacy_lineage = serde_json::to_string(&(
        lineage(&mut feedback_conn, &tables).await?,
        &sync.slot,
        &sync.publication,
    ))
    .map_err(|_| "sync_lineage")?;
    let mut conn = target_connection(target, store).await?;
    let mut engine = PostgresEngine::new(config.clone());
    let mut admission = c1::prepare(
        &mut engine,
        &mut conn,
        source,
        &tables,
        target,
        budget,
        &durable_lineage,
        Some(&legacy_lineage),
        &sync.initial_position,
    )
    .await?;
    if let Some(code) = admission.replay {
        conn.close().await.map_err(|_| "sync_target_close")?;
        feedback_conn
            .close()
            .await
            .map_err(|_| "sync_source_close")?;
        return c1::outcome(code);
    }
    for table in admission.repairs.clone() {
        let mut repair_changes = PostgresChanges::new(
            config.clone(),
            CaptureConfig {
                slot: sync.slot.clone(),
                publication: sync.publication.clone(),
                required_wal_headroom: sync.required_wal_headroom,
            },
        )
        .map_err(|e| e.to_string())?;
        repair_changes.quarantine(tables.iter().filter(|t| **t != table).cloned().collect());
        let repaired = c1_repair::run(
            &mut engine,
            &mut repair_changes,
            &mut conn,
            source,
            store,
            &tables,
            &table,
            target,
            budget,
            &durable_lineage,
            &sync.initial_position,
        )
        .await;
        c1::finish_repair(
            &mut conn,
            source,
            &tables,
            &durable_lineage,
            &sync.initial_position,
            &mut admission,
            table,
            repaired,
        )
        .await?;
    }
    // Feedback remains connected; do not retain a third metadata reader
    // while the bounded capture session is active.
    engine
        .close_idle_session()
        .await
        .map_err(|e| e.to_string())?;
    let mut changes = PostgresChanges::new(
        config,
        CaptureConfig {
            slot: sync.slot.clone(),
            publication: sync.publication,
            required_wal_headroom: sync.required_wal_headroom,
        },
    )
    .map_err(|e| e.to_string())?;
    changes.quarantine(admission.quarantined.clone());
    changes
        .earliest_available()
        .await
        .map_err(|e| e.to_string())?;
    let mut sink = PgSync::open(
        &mut conn,
        &binding,
        &durable_lineage,
        &tables,
        &sync.initial_position,
    )
    .await?;
    changes.projections(sink.projections());
    changes.table_positions(
        sink.table_positions()
            .await?
            .into_iter()
            .map(|table| {
                let Watermark::PgLsn { lsn } = table.position else {
                    return Err("sync_position_kind");
                };
                Ok((table.table, lsn))
            })
            .collect::<Result<_, &str>>()?,
    );
    let mut target = Target {
        sink,
        budget: budget.clone(),
        source,
        store,
        lineage: live_lineage.clone(),
        tables: tables.clone(),
        engine,
        admission,
    };
    let mut feedback = Feedback {
        connection: feedback_conn,
        slot: sync.slot,
        lineage: live_lineage,
        tables,
        duty: budget.duty_cycle_percent,
    };
    let before = target.sink.metrics().await?;
    // Include lineage/retention preflight in the source budget. Engine pacing
    // covers row reads, not all coordinator-owned control queries.
    let duty = u32::from(budget.duty_cycle_percent);
    tokio::time::sleep(setup_started.elapsed() * (100 - duty) / duty).await;
    let mut batches = 0;
    while batches < sync.max_batches && target.admission.quarantined.len() < target.tables.len() {
        let started = std::time::Instant::now();
        let result = engine_sync::step(
            &mut changes,
            &mut target,
            &mut feedback,
            budget,
            engine_sync::at_or_after,
        )
        .await;
        tokio::time::sleep(started.elapsed() * (100 - duty) / duty).await;
        if !result? {
            break;
        }
        batches += 1;
    }
    let after = target.sink.metrics().await?;
    println!(
        "{}",
        serde_json::json!({
            "protocol":"oo.sync/v1", "source":source.id, "mode":"c1", "batches":batches,
            "durable_position":target.sink.position().await?, "generation":after.0,
            "new_generations":after.0-before.0, "target_write_bytes":after.1-before.1,
            "write_basis":"committed_delta_and_new_tombstone_tuple_bytes_excluding_indexes_wal_metadata",
            "ack":"after_target_commit", "baseline_alignment":"caller_asserted",
            "publication":"local_table_view", "gateway_published":false, "source_time":"unmeasured"
        })
    );
    let outcome = c1::report(&mut target.sink, source, &target.admission).await;
    target
        .engine
        .close_idle_session()
        .await
        .map_err(|e| e.to_string())?;
    drop(target);
    conn.close().await.map_err(|_| "sync_target_close")?;
    feedback
        .connection
        .close()
        .await
        .map_err(|_| "sync_source_close")?;
    outcome
}
