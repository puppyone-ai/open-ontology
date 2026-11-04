use super::*;
use oo_connectors::postgres::reconcile::C5Cycle;
use oo_pipeline::engine_load::{Capture, Target};
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    cycle: u64,
    full_read_every: u64,
    #[serde(default)]
    positive_hints: BTreeSet<u32>,
    #[serde(default)]
    recent_chunks: BTreeSet<u32>,
}

pub(super) async fn run(
    mut engine: PostgresEngine,
    source: &Source,
    tables: &[TableRef],
    config: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    policy: Config,
) -> Result<(), String> {
    if policy.cycle == 0 || policy.full_read_every == 0 {
        return Err("c5_policy".into());
    }
    for table in tables {
        let started = std::time::Instant::now();
        let mut control = source_control_connection(source, store).await?;
        let live_lineage = sync::lineage(&mut control, std::slice::from_ref(table)).await?;
        let plan = engine
            .plan_chunks(table, budget)
            .await
            .map_err(|e| e.to_string())?;
        let mut conn = target_connection(config, store).await?;
        let mut target = PgTarget::new(&mut conn, &binding(source)?, &config.run_id, table).await?;
        let previous = target.c5_baseline().await?;
        if previous
            .lineage
            .as_ref()
            .is_some_and(|old| old != &live_lineage)
        {
            return Err("c5_rebaseline_required_lineage".into());
        }
        if !oo_pipeline::engine_load_pg::same_ranges(&previous.plan, &plan) {
            return Err("c5_rebaseline_required_plan".into());
        }
        let before = target.generations().await?;
        let mut receipts = Vec::new();
        let mut payload_bytes = 0u64;
        let mut source_read_bytes = 0u64;
        let replay = target.completed().await?.is_some();
        if !replay {
            let inspection = engine
                .inspect_c5(
                    table,
                    &previous.source_digests,
                    &C5Cycle {
                        cycle: policy.cycle,
                        full_read_every: policy.full_read_every,
                        positive_hints: policy.positive_hints.clone(),
                        recent_chunks: policy.recent_chunks.clone(),
                    },
                )
                .await
                .map_err(|e| e.to_string())?;
            let mut digests = Vec::new();
            for chunk in &plan.chunks {
                digests.push(
                    inspection
                        .source_digests
                        .iter()
                        .find(|d| d.lo == chunk.lo && d.hi == chunk.hi)
                        .ok_or("c5_digest_missing")?
                        .clone(),
                );
            }
            for chunk in inspection.read_chunks {
                let mut sink = oo_load::IpcSink::new(budget);
                let receipt = engine
                    .read_chunk(&chunk, budget, &mut sink)
                    .await
                    .map_err(|e| e.to_string())?;
                let ipc = sink.finish()?;
                source_read_bytes += ipc.len() as u64;
                let decoded =
                    oo_pipeline::engine_chunk::decode_chunk(&chunk, &receipt, &ipc, budget)?;
                let after = engine
                    .digest_range(table, chunk.lo.as_deref(), chunk.hi.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                let before_digest = digests
                    .iter()
                    .find(|d| d.lo == chunk.lo && d.hi == chunk.hi)
                    .ok_or("c5_digest_missing")?;
                if &after != before_digest || after.rows != receipt.rows {
                    return Err("c5_range_changed_retry".into());
                }
                let old = previous
                    .parts
                    .iter()
                    .find(|p| p.receipt.chunk_id == receipt.chunk_id)
                    .ok_or("c5_parent_missing")?;
                if old.receipt.sha256 != receipt.sha256 || old.receipt.rows != receipt.rows {
                    if receipts.is_empty() {
                        target.begin(&plan).await?;
                    }
                    target.stage(&receipt, &decoded).await?;
                    payload_bytes += receipt.bytes;
                    receipts.push(receipt);
                }
            }
            if sync::lineage(&mut control, std::slice::from_ref(table)).await? != live_lineage {
                return Err("c5_rebaseline_required_lineage".into());
            }
            target
                .publish_c5(&previous, &plan, digests, &receipts, &live_lineage)
                .await?;
        }
        let after = target.generations().await?;
        let duty = u32::from(budget.duty_cycle_percent);
        if !(1..=100).contains(&duty) {
            return Err("c5_budget".into());
        }
        tokio::time::sleep(started.elapsed() * (100 - duty) / duty).await;
        println!(
            "{}",
            serde_json::json!({
                "protocol":"oo.sync/v1","source":source.id,"mode":"c5","table":table,
                "target":target.relation(),"replay":replay,"ranges_replaced":receipts.len(),
                "payload_bytes":payload_bytes,"source_read_bytes":source_read_bytes,
                "target_write_bytes":if replay {0} else {target.write_bytes().await?},
                "generation":after,"new_generations":after-before,
                "write_basis":"committed_logical_stage_tuples_excluding_indexes_wal_metadata",
                "publication":"local_table_view","gateway_published":false,"consistency":"B","source_time":"unmeasured"
            })
        );
        drop(target);
        conn.close().await.map_err(|_| "c5_target_close")?;
        control.close().await.map_err(|_| "c5_source_close")?;
    }
    Ok(())
}
