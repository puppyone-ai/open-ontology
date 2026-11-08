//! Explicit, epoch-bound log capture; never infer an aligned baseline from now.
use super::*;
use epoch::Epoch;
use oo_connectors::engine::{ChangeBatch, ChangeSource, ChunkSink, Watermark};
use oo_pipeline::{
    engine_load::Capture, engine_sync, engine_sync_pg::PgSync, native_arrow::decode_arrow_stream,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    initial_position: Watermark,
    source_epoch: String,
    baseline_aligned: bool,
    max_batches: u32,
    #[serde(default)]
    server_id: u32,
    #[serde(default)]
    longest_cycle_seconds: u64,
    #[serde(default)]
    recovery_seconds: u64,
    #[serde(default)]
    oracle_logminer_opt_in: bool,
    oracle_retained_floor: Option<u64>,
    mining: Option<Mining>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mining {
    service: String,
    user: String,
    password: oo_stack::stack::SecretRef,
}

fn validate(config: &Config, live: &str, oracle: bool) -> Result<(), &'static str> {
    if !config.baseline_aligned || !(1..=10_000).contains(&config.max_batches) {
        return Err("sync_requires_aligned_baseline_and_bounded_batches");
    }
    if config.source_epoch.is_empty() || config.source_epoch != live {
        return Err("sync_rebaseline_required_lineage");
    }
    if oracle {
        if !config.oracle_logminer_opt_in {
            return Err("oracle_logminer_explicit_opt_in_required");
        }
        if config.oracle_retained_floor.is_none_or(|floor| floor == 0) {
            return Err("oracle_retained_floor_required");
        }
        if !matches!(config.initial_position, Watermark::OracleScn { .. }) {
            return Err("sync_position_kind");
        }
    } else if !matches!(config.initial_position, Watermark::MysqlFilePos { .. }) {
        return Err("sync_mysql_file_position_required");
    }
    Ok(())
}

struct Target<'a, 's, E> {
    sink: PgSync<'a>,
    engine: &'s mut E,
    source: &'s Source,
    store: &'s SecretStore,
    tables: &'s [TableRef],
    epoch: String,
    budget: ReadBudget,
    admission: c1::Admission,
}

impl<E: Epoch> engine_sync::Target for Target<'_, '_, E> {
    async fn position(&mut self) -> engine_load::Result<Watermark> {
        self.sink.position().await
    }
    async fn feedback_position(&mut self) -> engine_load::Result<Watermark> {
        self.sink.feedback_position().await
    }
    async fn apply(&mut self, from: &Watermark, batch: &ChangeBatch) -> engine_load::Result<()> {
        let live = self
            .engine
            .epoch(self.source, self.store, self.tables)
            .await
            .map_err(|_| "sync_epoch_read")?;
        if live != self.epoch {
            return Err("sync_rebaseline_required_lineage");
        }
        c1::check_shapes(self.engine, &self.admission, &self.budget, batch).await?;
        let mut images = Vec::new();
        let mut bytes = 0u64;
        for image in &batch.images {
            let mut capture = oo_load::IpcSink::new(&self.budget);
            capture
                .push(image.clone())
                .map_err(|_| "sync_image_budget")?;
            let ipc = capture.finish()?;
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

struct OracleData(oo_connectors::oracle::engine::logminer::OracleLogMiner);
trait IsolatedChanges: ChangeSource {
    fn failed_tables(&self) -> std::collections::BTreeSet<TableRef> {
        Default::default()
    }
    fn exclude(&mut self, _tables: &std::collections::BTreeSet<TableRef>) {}
}
#[cfg(feature = "mysql-binlog")]
impl IsolatedChanges for oo_connectors::mysql::MysqlChanges {}
impl IsolatedChanges for OracleData {
    fn failed_tables(&self) -> std::collections::BTreeSet<TableRef> {
        self.0.rebaseline_tables().clone()
    }
    fn exclude(&mut self, tables: &std::collections::BTreeSet<TableRef>) {
        self.0.quarantine(tables);
    }
}
impl ChangeSource for OracleData {
    async fn earliest_available(&mut self) -> Result<Watermark, oo_connectors::SourceError> {
        self.0.earliest_available().await
    }
    async fn next_batch(
        &mut self,
        from: &Watermark,
        budget: &ReadBudget,
    ) -> Result<Option<ChangeBatch>, oo_connectors::SourceError> {
        // Oracle scans the whole SCN window on every poll, not one transaction.
        // Keeping the old cursor on an empty window cannot hide later changes.
        Ok(self
            .0
            .next_batch(from, budget)
            .await?
            .and_then(oracle_data_batch))
    }
}
fn oracle_data_batch(batch: ChangeBatch) -> Option<ChangeBatch> {
    (!batch.changes.is_empty()).then_some(batch)
}

struct NoFeedback;
impl engine_sync::Feedback for NoFeedback {
    async fn acknowledge(&mut self, _: &Watermark) -> engine_load::Result<()> {
        // These engines do not release source logs via a consumer ACK.
        Ok(())
    }
}

async fn run<E: Epoch, C: IsolatedChanges>(
    engine: &mut E,
    changes: &mut C,
    source: &Source,
    tables: &[TableRef],
    store: &SecretStore,
    budget: &ReadBudget,
    config: Config,
    setup: (PgConnection, c1::Admission),
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let live = engine.epoch(source, store, tables).await?;
    validate(&config, &live, source.engine == "oracle")?;
    let duty = u32::from(budget.duty_cycle_percent);
    if !(1..=100).contains(&duty) {
        return Err("sync_budget".into());
    }
    let (mut conn, admission) = setup;
    let durable_epoch = if source.engine == "oracle" {
        serde_json::to_string(&(&live, config.oracle_retained_floor)).map_err(|_| "sync_epoch")?
    } else {
        live.clone()
    };
    let sink = PgSync::open(
        &mut conn,
        &binding(source)?,
        &durable_epoch,
        tables,
        &config.initial_position,
    )
    .await?;
    let mut target = Target {
        sink,
        engine,
        source,
        store,
        tables,
        epoch: live,
        budget: budget.clone(),
        admission,
    };
    let before = target.sink.metrics().await?;
    tokio::time::sleep(started.elapsed() * (100 - duty) / duty).await;
    let mut batches = 0;
    while batches < config.max_batches && target.admission.quarantined.len() < tables.len() {
        let started = std::time::Instant::now();
        let changed = engine_sync::step(
            changes,
            &mut target,
            &mut NoFeedback,
            budget,
            engine_sync::at_or_after,
        )
        .await;
        tokio::time::sleep(started.elapsed() * (100 - duty) / duty).await;
        let changed = match changed {
            Ok(changed) => changed,
            Err(error) => {
                let failed = changes.failed_tables();
                if failed.is_empty() {
                    return Err(error.into());
                }
                for table in failed {
                    target.sink.quarantine(&table, "schema_unproven").await?;
                    target.admission.quarantined.insert(table.clone());
                    target
                        .admission
                        .classes
                        .insert(table.clone(), "schema_unproven".into());
                    target.admission.attempts.insert(table, 1);
                }
                changes.exclude(&target.admission.quarantined);
                continue;
            }
        };
        if !changed {
            // Empty polling must not turn into a metadata/connection hot loop.
            tokio::time::sleep(Duration::from_secs(1)).await;
            break;
        }
        batches += 1;
    }
    let after = target.sink.metrics().await?;
    println!(
        "{}",
        serde_json::json!({
            "protocol":"oo.sync/v1", "source":source.id, "mode":"c1", "batches":batches,
            "source_epoch":target.epoch, "durable_position":target.sink.position().await?,
            "generation":after.0, "new_generations":after.0-before.0,
            "target_write_bytes":after.1-before.1,
            "write_basis":"committed_delta_and_new_tombstone_tuple_bytes_excluding_indexes_wal_metadata",
            "baseline_alignment":"caller_asserted", "publication":"local_table_view",
            "gateway_published":false, "source_time":"unmeasured",
            "oracle_internal_logminer_writes":source.engine == "oracle",
            "oracle_retained_floor":config.oracle_retained_floor
        })
    );
    let outcome = c1::report(&mut target.sink, source, &target.admission).await;
    drop(target);
    conn.close().await.map_err(|_| "sync_target_close")?;
    outcome
}

async fn prepare<E: Epoch>(
    engine: &mut E,
    source: &Source,
    tables: &[TableRef],
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    config: &Config,
) -> Result<(PgConnection, c1::Admission), String> {
    let live = engine.epoch(source, store, tables).await?;
    validate(config, &live, source.engine == "oracle")?;
    let lineage = if source.engine == "oracle" {
        serde_json::to_string(&(&live, config.oracle_retained_floor)).map_err(|_| "sync_epoch")?
    } else {
        live
    };
    let mut conn = target_connection(target, store).await?;
    let admission = c1::prepare(
        engine,
        &mut conn,
        source,
        tables,
        target,
        budget,
        &lineage,
        // MySQL/Oracle C1 lineage never carried a table shape: no legacy proof.
        None,
        &config.initial_position,
    )
    .await?;
    if admission.quarantined.len() == tables.len() && admission.repairs.is_empty() {
        let mut sink = PgSync::open(
            &mut conn,
            &binding(source)?,
            &lineage,
            tables,
            &config.initial_position,
        )
        .await?;
        let outcome = c1::report(&mut sink, source, &admission).await;
        drop(sink);
        conn.close().await.map_err(|_| "sync_target_close")?;
        return outcome.and(Err("sync_all_stale".into()));
    }
    Ok((conn, admission))
}

#[cfg(feature = "mysql-binlog")]
pub(super) async fn mysql(
    mut engine: oo_connectors::mysql::engine::MysqlEngine,
    source: &Source,
    tables: &[TableRef],
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    config: Config,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    engine
        .use_file_positions()
        .await
        .map_err(|e| e.to_string())?;
    let mut setup = prepare(&mut engine, source, tables, target, store, budget, &config).await?;
    if let Some(code) = setup.1.replay {
        setup.0.close().await.map_err(|_| "sync_target_close")?;
        engine.close().await;
        return c1::outcome(code);
    }
    for table in setup.1.repairs.clone() {
        let mut repair_changes = engine
            .changes(std::slice::from_ref(&table), config.server_id)
            .await
            .map_err(|e| e.to_string())?;
        let repaired = c1_repair::run(
            &mut engine,
            &mut repair_changes,
            &mut setup.0,
            source,
            store,
            tables,
            &table,
            target,
            budget,
            &config.source_epoch,
            &config.initial_position,
        )
        .await;
        repair_changes.close().await;
        c1::finish_repair(
            &mut setup.0,
            source,
            tables,
            &config.source_epoch,
            &config.initial_position,
            &mut setup.1,
            table,
            repaired,
        )
        .await?;
    }
    if setup.1.quarantined.len() == tables.len() {
        let mut sink = PgSync::open(
            &mut setup.0,
            &binding(source)?,
            &config.source_epoch,
            tables,
            &config.initial_position,
        )
        .await?;
        return c1::report(&mut sink, source, &setup.1).await;
    }
    let active: Vec<_> = tables
        .iter()
        .filter(|t| !setup.1.quarantined.contains(*t))
        .cloned()
        .collect();
    let mut changes = engine
        .changes(&active, config.server_id)
        .await
        .map_err(|e| e.to_string())?;
    changes.allow_classified_alter(setup.1.mysql_alter.clone(), setup.1.quarantined.clone());
    let mut positions = PgSync::open(
        &mut setup.0,
        &binding(source)?,
        &config.source_epoch,
        tables,
        &config.initial_position,
    )
    .await?;
    changes.projections(positions.projections(), setup.1.previous_columns.clone());
    changes.table_positions(
        positions
            .table_positions()
            .await?
            .into_iter()
            .map(|table| (table.table, table.position))
            .collect(),
    );
    drop(positions);
    validate(&config, changes.source_epoch(), false)?;
    changes
        .retention(config.longest_cycle_seconds, config.recovery_seconds)
        .await
        .map_err(|e| e.to_string())?
        .require_sufficient()
        .map_err(|_| "sync_rebaseline_required_retention")?;
    engine.close().await;
    tokio::time::sleep(started.elapsed() * 3).await;
    let result = run(
        &mut engine,
        &mut changes,
        source,
        tables,
        store,
        budget,
        config,
        setup,
    )
    .await;
    changes.close().await;
    engine.close().await;
    result
}

pub(super) async fn oracle(
    mut engine: oo_connectors::oracle::engine::OracleEngine,
    source: &Source,
    tables: &[TableRef],
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    mut config: Config,
    worker: Option<&String>,
) -> Result<(), String> {
    use oo_connectors::oracle::engine::{
        OracleConnectConfig, OracleEngine, logminer::OracleLogMiner,
    };
    // Refuse before opening any LogMiner session (D11a).
    validate(
        &config,
        &engine.source_epoch().map_err(|e| e.to_string())?,
        true,
    )?;
    let mining = config
        .mining
        .take()
        .ok_or("oracle_mining_connection_required")?;
    {
        // Source-wide logging refusal precedes any target write, including
        // the fresh binding's initial position.
        let root = OracleConnectConfig::new(
            &source.host,
            source.port,
            &mining.service,
            &mining.user,
            zeroize::Zeroizing::new(store.read(&mining.password.secret_ref)?),
        );
        let mut root = match worker {
            Some(worker) => {
                OracleEngine::connect_worker(root, &mut std::process::Command::new(worker))
            }
            None => OracleEngine::connect(root),
        }
        .map_err(|e| e.to_string())?;
        OracleLogMiner::require_logging(&mut root).map_err(|e| e.to_string())?;
    }
    let mut setup = prepare(&mut engine, source, tables, target, store, budget, &config).await?;
    if let Some(code) = setup.1.replay {
        setup.0.close().await.map_err(|_| "sync_target_close")?;
        return c1::outcome(code);
    }
    for table in setup.1.repairs.clone() {
        let root = OracleConnectConfig::new(
            &source.host,
            source.port,
            &mining.service,
            &mining.user,
            zeroize::Zeroizing::new(store.read(&mining.password.secret_ref)?),
        );
        let root = match worker {
            Some(worker) => {
                OracleEngine::connect_worker(root, &mut std::process::Command::new(worker))
            }
            None => OracleEngine::connect(root),
        }
        .map_err(|e| e.to_string())?;
        let mut repair_changes =
            OracleLogMiner::new(root, &mut engine, std::slice::from_ref(&table))
                .map_err(|e| e.to_string())?;
        let Watermark::OracleScn { scn } = config.initial_position else {
            return Err("sync_position_kind".into());
        };
        repair_changes
            .resume_from_floor(
                config
                    .oracle_retained_floor
                    .ok_or("oracle_retained_floor_required")?,
                scn,
            )
            .map_err(|e| e.to_string())?;
        let lineage = serde_json::to_string(&(&config.source_epoch, config.oracle_retained_floor))
            .map_err(|_| "sync_epoch")?;
        let repaired = c1_repair::run(
            &mut engine,
            &mut repair_changes,
            &mut setup.0,
            source,
            store,
            tables,
            &table,
            target,
            budget,
            &lineage,
            &config.initial_position,
        )
        .await;
        c1::finish_repair(
            &mut setup.0,
            source,
            tables,
            &lineage,
            &config.initial_position,
            &mut setup.1,
            table,
            repaired,
        )
        .await?;
    }
    if setup.1.quarantined.len() == tables.len() {
        let lineage = serde_json::to_string(&(&config.source_epoch, config.oracle_retained_floor))
            .map_err(|_| "sync_epoch")?;
        let mut sink = PgSync::open(
            &mut setup.0,
            &binding(source)?,
            &lineage,
            tables,
            &config.initial_position,
        )
        .await?;
        return c1::report(&mut sink, source, &setup.1).await;
    }
    let active: Vec<_> = tables
        .iter()
        .filter(|t| !setup.1.quarantined.contains(*t))
        .cloned()
        .collect();
    let root = OracleConnectConfig::new(
        &source.host,
        source.port,
        &mining.service,
        &mining.user,
        zeroize::Zeroizing::new(store.read(&mining.password.secret_ref)?),
    );
    let root = match worker {
        Some(worker) => OracleEngine::connect_worker(root, &mut std::process::Command::new(worker)),
        None => OracleEngine::connect(root),
    }
    .map_err(|e| e.to_string())?;
    let mut changes = OracleLogMiner::new(root, &mut engine, &active).map_err(|e| e.to_string())?;
    changes.allow_classified_ddl();
    let lineage = serde_json::to_string(&(&config.source_epoch, config.oracle_retained_floor))
        .map_err(|_| "sync_epoch")?;
    let mut positions = PgSync::open(
        &mut setup.0,
        &binding(source)?,
        &lineage,
        tables,
        &config.initial_position,
    )
    .await?;
    changes.table_positions(
        positions
            .table_positions()
            .await?
            .into_iter()
            .map(|table| {
                let Watermark::OracleScn { scn } = table.position else {
                    return Err("sync_position_kind");
                };
                Ok((table.table, scn))
            })
            .collect::<Result<_, &str>>()?,
    );
    drop(positions);
    validate(&config, changes.source_epoch(), true)?;
    let Watermark::OracleScn { scn } = config.initial_position else {
        return Err("sync_position_kind".into());
    };
    changes
        .resume_from_floor(
            config
                .oracle_retained_floor
                .ok_or("oracle_retained_floor_required")?,
            scn,
        )
        .map_err(|e| e.to_string())?;
    let mut changes = OracleData(changes);
    run(
        &mut engine,
        &mut changes,
        source,
        tables,
        store,
        budget,
        config,
        setup,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrelated_oracle_scn_advance_does_not_publish_a_generation() {
        assert!(
            oracle_data_batch(ChangeBatch {
                position: Watermark::OracleScn { scn: 42 },
                changes: vec![],
                images: vec![],
            })
            .is_none()
        );
    }

    #[test]
    fn epoch_and_explicit_oracle_opt_in_are_required() {
        let mut config: Config = serde_json::from_value(serde_json::json!({
            "initial_position":{"kind":"oracle_scn","scn":42}, "source_epoch":"1:2",
            "baseline_aligned":true, "max_batches":3
        }))
        .unwrap();
        assert_eq!(
            validate(&config, "1:2", true),
            Err("oracle_logminer_explicit_opt_in_required")
        );
        config.oracle_logminer_opt_in = true;
        assert_eq!(
            validate(&config, "1:2", true),
            Err("oracle_retained_floor_required")
        );
        config.oracle_retained_floor = Some(1);
        assert!(validate(&config, "1:2", true).is_ok());
        assert_eq!(
            validate(&config, "1:3", true),
            Err("sync_rebaseline_required_lineage")
        );
        assert!(validate(&config, "1:2", false).is_err());
        config.initial_position = Watermark::MysqlFilePos {
            file: "bin.000001".into(),
            position: 4,
        };
        assert!(validate(&config, "1:2", false).is_ok());
        config.baseline_aligned = false;
        assert!(validate(&config, "1:2", false).is_err());
    }
}
