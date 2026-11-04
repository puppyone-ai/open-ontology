//! SQL Server dispatch and lossless positions for the shared durable target.
use super::*;
use oo_connectors::engine::{ChangeBatch, ChangeSource, ChunkSink, Watermark};
use oo_pipeline::{
    engine_load::Capture, engine_sync_pg::PgSync, native_arrow::decode_arrow_stream,
};
use serde::Serialize;
use sqlserver::{ChangeMode, SqlServerEngine};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Position {
    protocol: String,
    source_epoch: String,
    mode: ChangeMode,
    value: String,
    schema: String,
}

impl Position {
    fn parse(w: &Watermark) -> Result<Self, &'static str> {
        let Watermark::Opaque { value } = w else {
            return Err("sqlserver_position");
        };
        let p: Self = serde_json::from_str(value).map_err(|_| "sqlserver_position")?;
        if p.protocol != "oo.sqlserver.position/v1"
            || p.source_epoch.is_empty()
            || p.schema.is_empty()
        {
            return Err("sqlserver_position_identity");
        }
        p.numeric()?;
        Ok(p)
    }

    fn lineage(&self) -> String {
        serde_json::to_string(&(&self.protocol, &self.source_epoch, &self.schema, self.mode))
            .expect("string tuple serialization")
    }

    fn numeric(&self) -> Result<Watermark, &'static str> {
        match self.mode {
            ChangeMode::Ct => Ok(Watermark::PgLsn {
                lsn: self.value.parse().map_err(|_| "sqlserver_ct_position")?,
            }),
            ChangeMode::Cdc => {
                if self.value.len() != 20 || !self.value.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("sqlserver_cdc_position");
                }
                // SQL Server LSN is 80 bits: VLF (32), offset (32), slot (16).
                // File/offset storage avoids truncation and uses numeric ordering.
                let file = u32::from_str_radix(&self.value[..8], 16)
                    .map_err(|_| "sqlserver_cdc_position")?;
                let position = u64::from_str_radix(&self.value[8..], 16)
                    .map_err(|_| "sqlserver_cdc_position")?;
                Ok(Watermark::MysqlFilePos {
                    file: format!("sqlserver-cdc.{file}"),
                    position,
                })
            }
        }
    }

    fn restore(&self, durable: &Watermark) -> Result<Watermark, &'static str> {
        let mut p = self.clone();
        p.value = match (self.mode, durable) {
            (ChangeMode::Ct, Watermark::PgLsn { lsn }) => lsn.to_string(),
            (ChangeMode::Cdc, Watermark::MysqlFilePos { file, position })
                if *position < (1u64 << 48) =>
            {
                let file: u32 = file
                    .strip_prefix("sqlserver-cdc.")
                    .ok_or("sqlserver_durable_position")?
                    .parse()
                    .map_err(|_| "sqlserver_durable_position")?;
                format!("{file:08X}{position:012X}")
            }
            _ => return Err("sqlserver_durable_position"),
        };
        Ok(Watermark::Opaque {
            value: serde_json::to_string(&p).map_err(|_| "sqlserver_position")?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    mode: ChangeMode,
    initial_position: Watermark,
    source_epoch: String,
    baseline_aligned: bool,
    max_batches: u32,
}

impl epoch::Epoch for SqlServerEngine {
    async fn epoch(
        &mut self,
        _: &Source,
        _: &SecretStore,
        _: &[TableRef],
    ) -> Result<String, String> {
        self.source_lineage().await.map_err(|e| e.to_string())
    }
}

pub(super) async fn run(
    source: &Source,
    tables: &[TableRef],
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    opts: &BTreeMap<String, String>,
) -> Result<(), String> {
    let sync: Option<Config> = opts
        .get("--sync")
        .map(|path| serde_json::from_slice(&read(path)?).map_err(|_| "sqlserver_sync_config"))
        .transpose()?;
    if sync
        .as_ref()
        .is_some_and(|c| !c.baseline_aligned || !(1..=10_000).contains(&c.max_batches))
    {
        return Err("sync_requires_aligned_baseline_and_bounded_batches".into());
    }
    let ca = source.tls_ca.as_ref().ok_or("source_tls_required")?;
    let mut config = sqlserver_config(
        source,
        tables,
        store.read(&source.password.secret_ref)?,
        store.read(&ca.secret_ref)?.into_bytes(),
    )?;
    config.mode = sync.as_ref().map(|c| c.mode);
    let mut engine = SqlServerEngine::connect(config, budget.clone())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(path) = opts.get("--c5") {
        return c5::run(
            engine,
            source,
            tables,
            target,
            store,
            budget,
            serde_json::from_slice(&read(path)?).map_err(|_| "c5_config")?,
        )
        .await;
    }
    let result = if let Some(config) = sync {
        synchronize(&mut engine, source, tables, target, store, budget, config).await
    } else {
        run_engine(&mut engine, source, tables, target, store, budget).await
    };
    eprintln!(
        "{}",
        serde_json::json!({
            "protocol":"oo.load.resources/v1", "source":source.id,
            "resources":engine.resource_receipt()
        })
    );
    result
}

async fn synchronize(
    engine: &mut SqlServerEngine,
    source: &Source,
    tables: &[TableRef],
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    config: Config,
) -> Result<(), String> {
    let initial = Position::parse(&config.initial_position)?;
    let live = Position::parse(
        &engine
            .watermark()
            .await
            .map_err(|e| e.to_string())?
            .ok_or("sqlserver_watermark")?,
    )?;
    if initial.lineage() != live.lineage()
        || config.source_epoch != live.source_epoch
        || config.mode != live.mode
    {
        return Err("sync_rebaseline_required_lineage".into());
    }
    let mut conn = target_connection(target, store).await?;
    let mut sink = PgSync::open(
        &mut conn,
        &binding(source)?,
        &live.lineage(),
        tables,
        &initial.numeric()?,
    )
    .await?;
    let before = sink.metrics().await?;
    let mut batches = 0;
    while batches < config.max_batches {
        let from = sink.position().await?;
        // The connector rechecks epoch/schema and retention in each fresh
        // SNAPSHOT transaction before returning any changes.
        let Some(batch) = engine
            .next_batch(&live.restore(&from)?, budget)
            .await
            .map_err(|e| e.to_string())?
        else {
            break;
        };
        let position = Position::parse(&batch.position)?;
        if position.lineage() != live.lineage() {
            return Err("sync_rebaseline_required_lineage".into());
        }
        if batch.changes.is_empty() {
            break;
        }
        let mut images = Vec::new();
        let mut bytes = 0u64;
        for image in &batch.images {
            let mut capture = oo_load::IpcSink::new(budget);
            capture
                .push(image.clone())
                .map_err(|_| "sync_image_budget")?;
            let ipc = capture.finish()?;
            bytes = bytes
                .checked_add(ipc.len() as u64)
                .ok_or("sync_image_budget")?;
            if bytes > budget.max_chunk_bytes {
                return Err("sync_image_budget".into());
            }
            images.push(decode_arrow_stream(&ipc).map_err(|_| "sync_image_decode")?);
        }
        let batch = ChangeBatch {
            position: position.numeric()?,
            changes: batch.changes,
            images: batch.images,
        };
        sink.apply(&from, &batch, &images).await?;
        batches += 1;
    }
    let after = sink.metrics().await?;
    println!(
        "{}",
        serde_json::json!({
            "protocol":"oo.sync/v1", "source":source.id, "mode":config.mode,
            "batches":batches, "source_epoch":live.source_epoch,
            "durable_position":live.restore(&sink.position().await?)?,
            "position_storage":"numeric_lossless_with_epoch_schema_mode_binding",
            "generation":after.0, "new_generations":after.0-before.0,
            "target_write_bytes":after.1-before.1,
            "baseline_alignment":"caller_asserted", "source_time":"unmeasured",
            "publication":"local_table_view", "gateway_published":false
        })
    );
    drop(sink);
    conn.close().await.map_err(|_| "sync_target_close")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn position(mode: ChangeMode, value: &str) -> Position {
        Position {
            protocol: "oo.sqlserver.position/v1".into(),
            source_epoch: "database:fork".into(),
            schema: "schema-hash".into(),
            mode,
            value: value.into(),
        }
    }
    #[test]
    fn durable_position_round_trip_is_lossless() {
        for p in [
            position(ChangeMode::Ct, "18446744073709551615"),
            position(ChangeMode::Cdc, "FFFFFFFFFFFFFFFFFFFF"),
            position(ChangeMode::Cdc, "00000001000000020003"),
        ] {
            let restored = Position::parse(&p.restore(&p.numeric().unwrap()).unwrap()).unwrap();
            assert_eq!(restored.value, p.value);
            assert_eq!(restored.lineage(), p.lineage());
        }
        assert!(position(ChangeMode::Cdc, "0000").numeric().is_err());
        assert!(position(ChangeMode::Ct, "-1").numeric().is_err());
    }
    #[test]
    fn rollover_is_numeric_and_mode_schema_epoch_are_bound() {
        let a = position(ChangeMode::Cdc, "00000009FFFFFFFFFFFF");
        let mut b = position(ChangeMode::Cdc, "0000000A000000000000");
        assert!(
            oo_pipeline::engine_sync::at_or_after(&b.numeric().unwrap(), &a.numeric().unwrap())
                .unwrap()
        );
        assert_eq!(a.lineage(), b.lineage());
        b.schema.push('x');
        assert_ne!(a.lineage(), b.lineage());
        b = a.clone();
        b.source_epoch.push('x');
        assert_ne!(a.lineage(), b.lineage());
        b = a.clone();
        b.mode = ChangeMode::Ct;
        assert_ne!(a.lineage(), b.lineage());
    }
}
