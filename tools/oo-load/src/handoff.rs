//! Repair every selected binding before one atomic multi-source target switch.
use super::*;
use oo_connectors::{
    engine::{ChunkReceipt, ChunkSink, ChunkSpec, Watermark},
    mysql::engine::repair::{HandoffBarrier, RepairBinding, RepairBudget},
};
use oo_pipeline::{
    engine_load::{Capture, Target},
    engine_sync_pg::PgSync,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    sources: Vec<Member>,
    max_keys: u64,
    max_bytes: u64,
    required_history_seconds: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    source: String,
    server_id: u32,
}

fn scope(
    config: &Config,
    primary: &Source,
    sources: &[Source],
) -> Result<BTreeMap<String, BTreeSet<String>>, &'static str> {
    if config.sources.is_empty()
        || config.sources.len() > 16
        || config.sources[0].source != primary.id
        || config.max_keys == 0
        || config.max_keys > 100_000
        || config.max_bytes == 0
        || config.max_bytes > 32 * 1024 * 1024
    {
        return Err("handoff_scope_budget");
    }
    let mut scope = BTreeMap::new();
    for member in &config.sources {
        let source = sources
            .iter()
            .find(|s| s.id == member.source)
            .ok_or("handoff_source")?;
        if source.engine != "mysql"
            || source.ssh.is_some()
            || member.server_id == 0
            || scope.contains_key(&source.id)
        {
            return Err("handoff_scope");
        }
        scope.insert(
            source.id.clone(),
            selected(source, None)?
                .iter()
                .map(ToString::to_string)
                .collect(),
        );
    }
    Ok(scope)
}

pub(super) async fn run(
    sources: &[Source],
    primary: &Source,
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    opts: &BTreeMap<String, String>,
    config: Config,
) -> Result<(), String> {
    if opts.contains_key("--tables") {
        return Err("handoff_uses_complete_stack_scope".into());
    }
    let started = std::time::Instant::now();
    let mut barrier =
        HandoffBarrier::new(scope(&config, primary, sources)?).map_err(|e| e.to_string())?;
    let mut engines = Vec::new();
    let mut plans = BTreeMap::new();
    let mut epochs = BTreeMap::new();
    for member in &config.sources {
        let source = sources
            .iter()
            .find(|s| s.id == member.source)
            .ok_or("handoff_source")?;
        let mut engine = mysql_engine(EngineFamily::Mysql, source, store, opts)?;
        engine
            .use_file_positions()
            .await
            .map_err(|e| e.to_string())?;
        let epoch = engine.source_epoch().await.map_err(|e| e.to_string())?;
        let mut bindings = Vec::new();
        for table in selected(source, None)? {
            let plan = engine
                .plan_chunks(&table, budget)
                .await
                .map_err(|e| e.to_string())?;
            bindings.push(RepairBinding {
                binding_id: table.to_string(),
                plan: plan.clone(),
            });
            plans.insert((source.id.clone(), table), plan);
        }
        let repair = engine
            .repair_to_c1(
                &bindings,
                member.server_id,
                budget,
                RepairBudget {
                    max_keys: config.max_keys,
                    max_bytes: config.max_bytes,
                    required_history_seconds: config.required_history_seconds,
                },
            )
            .await
            .map_err(|e| e.to_string())?;
        if !matches!(repair.end(), Watermark::MysqlFilePos { .. }) {
            return Err("handoff_mysql_file_position_required".into());
        }
        if engine.source_epoch().await.map_err(|e| e.to_string())? != epoch {
            return Err("handoff_rebaseline_required_epoch".into());
        }
        epochs.insert(source.id.clone(), epoch);
        barrier.stage(repair).map_err(|e| e.to_string())?;
        engine.close().await;
        engines.push((source, engine));
    }
    let mut conn = target_connection(target, store).await?;
    let mut tx = conn.begin().await.map_err(|_| "handoff_target")?;
    let mut receipts = Vec::new();
    let mut confirmations = Vec::new();
    // PgTarget/PgSync use nested sqlx transactions (savepoints). None of their
    // publications or positions become visible until this outer commit.
    for (source, engine) in &mut engines {
        let repair = barrier.repair(&source.id).ok_or("handoff_missing_source")?;
        let tables = selected(source, None)?;
        for table in &tables {
            let mut sink = PgTarget::new(&mut tx, &binding(source)?, &target.run_id, table).await?;
            if sink.completed().await?.is_some() {
                return Err("handoff_run_already_published".into());
            }
            // Admit only a complete C5/snapshot baseline, never an active C1 binding.
            let previous = sink.c5_baseline().await?;
            if previous
                .lineage
                .as_ref()
                .is_some_and(|old| old != &epochs[&source.id])
            {
                return Err("handoff_rebaseline_required_epoch".into());
            }
            let mut plan = plans[&(source.id.clone(), table.clone())].clone();
            let chunk = ChunkSpec {
                table: table.clone(),
                plan_revision: 1,
                chunk_id: 0,
                lo: None,
                hi: None,
                estimated_rows: None,
            };
            plan.chunks = vec![chunk.clone()];
            let mut capture = oo_load::IpcSink::new(budget);
            let empty = repair
                .empty_batches()
                .get(table)
                .ok_or("handoff_empty_schema")?;
            capture.push(empty.clone()).map_err(|e| e.to_string())?;
            let mut rows = 0;
            for ((row_table, _), image) in repair.rows() {
                if row_table == table {
                    if let Some(image) = image {
                        rows += image.num_rows() as u64;
                        capture.push(image.clone()).map_err(|e| e.to_string())?;
                    }
                }
            }
            let ipc = capture.finish()?;
            let receipt = ChunkReceipt {
                table: table.clone(),
                plan_revision: 1,
                chunk_id: 0,
                rows,
                bytes: ipc.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&ipc)),
                low: Some(repair.l0().clone()),
                high: Some(repair.end().clone()),
            };
            let decoded = oo_pipeline::engine_chunk::decode_chunk(&chunk, &receipt, &ipc, budget)?;
            sink.begin(&plan).await?;
            sink.stage(&receipt, &decoded).await?;
            sink.publish(&plan).await?;
            receipts.push(
                serde_json::json!({"source":source.id,"table":table,"target":sink.relation(),
                "target_write_bytes":sink.write_bytes().await?, "receipt":receipt}),
            );
            confirmations.push((source.id.clone(), table.to_string(), repair.end().clone()));
        }
        let mut sync = PgSync::open(
            &mut tx,
            &binding(source)?,
            &epochs[&source.id],
            &tables,
            repair.end(),
        )
        .await?;
        if sync.position().await? != *repair.end() {
            return Err("handoff_position".into());
        }
        drop(sync);
        if engine.source_epoch().await.map_err(|e| e.to_string())? != epochs[&source.id] {
            return Err("handoff_rebaseline_required_epoch".into());
        }
        let earliest = engine.earliest_binlog().await.map_err(|e| e.to_string())?;
        if !oo_pipeline::engine_sync::at_or_after(repair.end(), &earliest)? {
            return Err("handoff_rebaseline_required_retention".into());
        }
        engine.close().await;
    }
    tx.commit().await.map_err(|_| "handoff_commit")?;
    for (source, table, end) in confirmations {
        barrier
            .confirm_applied(&source, &table, &end)
            .map_err(|e| e.to_string())?;
    }
    let positions = barrier.switch().map_err(|e| e.to_string())?;
    conn.close().await.map_err(|_| "handoff_target_close")?;
    let duty = u32::from(budget.duty_cycle_percent);
    tokio::time::sleep(started.elapsed() * (100 - duty) / duty).await;
    println!(
        "{}",
        serde_json::json!({"protocol":"oo.handoff/v1", "mode":"c1", "positions":positions,
        "source_epochs":epochs, "bindings":receipts, "scope_committed_atomically":true,
        "publication":"local_table_view", "gateway_published":false, "consistency":"B"})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handoff_scope_is_complete_unique_and_explicit() {
        let source: Source = serde_json::from_value(serde_json::json!({
            "id":"fixture","engine":"mysql","host":"localhost","port":3306,
            "database":"fixture","user":"reader","password":{"secret_ref":"reader"},
            "scope":{"include":["fixture.items","fixture.other"],"exclude":[]}
        }))
        .unwrap();
        let mut config: Config = serde_json::from_value(serde_json::json!({
            "sources":[{"source":"fixture","server_id":42}],"max_keys":100,
            "max_bytes":10000,"required_history_seconds":2
        }))
        .unwrap();
        assert_eq!(
            scope(&config, &source, std::slice::from_ref(&source)).unwrap()["fixture"].len(),
            2
        );
        config.sources.push(Member {
            source: "fixture".into(),
            server_id: 43,
        });
        assert!(scope(&config, &source, std::slice::from_ref(&source)).is_err());
        config.sources.pop();
        config.max_keys = 100001;
        assert!(scope(&config, &source, std::slice::from_ref(&source)).is_err());
    }
}
