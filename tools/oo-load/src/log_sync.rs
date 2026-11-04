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
}

impl<E: Epoch> engine_sync::Target for Target<'_, '_, E> {
    async fn position(&mut self) -> engine_load::Result<Watermark> {
        self.sink.position().await
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

struct NoFeedback;
impl engine_sync::Feedback for NoFeedback {
    async fn acknowledge(&mut self, _: &Watermark) -> engine_load::Result<()> {
        // These engines do not release source logs via a consumer ACK.
        Ok(())
    }
}

async fn run<E: Epoch, C: ChangeSource>(
    engine: &mut E,
    changes: &mut C,
    source: &Source,
    tables: &[TableRef],
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    config: Config,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let live = engine.epoch(source, store, tables).await?;
    validate(&config, &live, source.engine == "oracle")?;
    let duty = u32::from(budget.duty_cycle_percent);
    if !(1..=100).contains(&duty) {
        return Err("sync_budget".into());
    }
    let mut conn = target_connection(target, store).await?;
    let sink = PgSync::open(
        &mut conn,
        &binding(source)?,
        &live,
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
    };
    let before = target.sink.metrics().await?;
    tokio::time::sleep(started.elapsed() * (100 - duty) / duty).await;
    let mut batches = 0;
    while batches < config.max_batches {
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
        if !changed? {
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
            "oracle_internal_logminer_writes":source.engine == "oracle"
        })
    );
    drop(target);
    conn.close().await.map_err(|_| "sync_target_close")?;
    Ok(())
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
    let mut changes = engine
        .changes(tables, config.server_id)
        .await
        .map_err(|e| e.to_string())?;
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
        target,
        store,
        budget,
        config,
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
    let mut changes = OracleLogMiner::new(root, &mut engine, tables).map_err(|e| e.to_string())?;
    validate(&config, changes.source_epoch(), true)?;
    run(
        &mut engine,
        &mut changes,
        source,
        tables,
        target,
        store,
        budget,
        config,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
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
