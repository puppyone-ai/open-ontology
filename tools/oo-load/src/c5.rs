use super::*;
use oo_connectors::engine::{ChunkStrategy, RangeDigest, TableChunkPlan};
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

fn ordered_chunks(
    plan: &TableChunkPlan,
    policy: &Config,
) -> Result<Vec<oo_connectors::engine::ChunkSpec>, &'static str> {
    if policy.cycle == 0 || policy.full_read_every == 0 {
        return Err("c5_policy");
    }
    if matches!(plan.strategy, ChunkStrategy::PhysicalRange { .. }) {
        return Err("c5_physical_ranges_not_durable");
    }
    let mut chunks = plan.chunks.clone();
    if policy
        .positive_hints
        .union(&policy.recent_chunks)
        .any(|id| !chunks.iter().any(|c| c.chunk_id == *id))
    {
        return Err("c5_unknown_hint_chunk");
    }
    chunks.sort_by_key(|c| {
        (
            !policy.positive_hints.contains(&c.chunk_id),
            !policy.recent_chunks.contains(&c.chunk_id),
            c.chunk_id,
        )
    });
    Ok(chunks)
}

/// Keep the baseline's key ranges while the key shape is unchanged. Live
/// boundaries follow optimizer row estimates and MIN/MAX, so replanning every
/// cycle turned ordinary growth or a drifting estimate into a spurious
/// rebaseline. The durable ranges still partition the whole key space (first
/// and last bounds are open); a changed key or strategy must rebaseline.
fn durable_ranges(previous: &TableChunkPlan, live: TableChunkPlan) -> TableChunkPlan {
    if previous.protocol == live.protocol
        && previous.table == live.table
        && previous.key == live.key
        && previous.strategy == live.strategy
        && matches!(live.strategy, ChunkStrategy::Keyset { .. })
        && previous.chunks.first().is_some_and(|c| c.lo.is_none())
        && previous.chunks.last().is_some_and(|c| c.hi.is_none())
    {
        TableChunkPlan {
            estimated_rows: live.estimated_rows,
            ..previous.clone()
        }
    } else {
        live
    }
}

fn needs_read(old: Option<&RangeDigest>, new: &RangeDigest, hint: bool, full: bool) -> bool {
    hint || full || old != Some(new)
}

pub(super) async fn run<E: epoch::Epoch>(
    mut engine: E,
    source: &Source,
    tables: &[TableRef],
    config: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    policy: Config,
) -> Result<(), String> {
    let result = run_cycle(&mut engine, source, tables, config, store, budget, policy).await;
    // MySQL-wire disconnect must finish before the short-lived runtime exits.
    engine.close_source().await;
    result
}

async fn run_cycle<E: epoch::Epoch>(
    engine: &mut E,
    source: &Source,
    tables: &[TableRef],
    config: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    policy: Config,
) -> Result<(), String> {
    if policy.cycle == 0
        || policy.full_read_every == 0
        || !(1..=100).contains(&budget.duty_cycle_percent)
    {
        return Err("c5_policy".into());
    }
    for table in tables {
        let started = std::time::Instant::now();
        let live_lineage = epoch::c5_lineage(
            engine
                .epoch(source, store, std::slice::from_ref(table))
                .await?,
            engine.shape(table).await?,
        );
        let live_plan = engine
            .plan_chunks(table, budget)
            .await
            .map_err(|e| e.to_string())?;
        let mut conn = target_connection(config, store).await?;
        let mut target = PgTarget::new(&mut conn, &binding(source)?, &config.run_id, table).await?;
        target = documents::target(target, source)?;
        let previous = target.c5_baseline().await?;
        if previous
            .lineage
            .as_ref()
            .is_some_and(|old| old != &live_lineage)
        {
            return Err("c5_rebaseline_required_lineage".into());
        }
        let plan = durable_ranges(&previous.plan, live_plan);
        if !oo_pipeline::engine_load_pg::same_ranges(&previous.plan, &plan) {
            return Err("c5_rebaseline_required_plan".into());
        }
        engine.adopt_plan(&plan)?;
        let before = target.generations().await?;
        let mut receipts = Vec::new();
        let mut payload_bytes = 0u64;
        let mut source_read_bytes = 0u64;
        let replay = target.completed().await?.is_some();
        if !replay {
            let mut source_digests = Vec::new();
            let mut read_chunks = Vec::new();
            for chunk in ordered_chunks(&plan, &policy)? {
                let digest = engine
                    .digest_range(table, chunk.lo.as_deref(), chunk.hi.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                let old = previous
                    .source_digests
                    .iter()
                    .find(|d| d.lo == digest.lo && d.hi == digest.hi);
                if needs_read(
                    old,
                    &digest,
                    policy.positive_hints.contains(&chunk.chunk_id),
                    policy.cycle.is_multiple_of(policy.full_read_every),
                ) {
                    read_chunks.push(chunk);
                }
                source_digests.push(digest);
            }
            let mut digests = Vec::new();
            for chunk in &plan.chunks {
                digests.push(
                    source_digests
                        .iter()
                        .find(|d| d.lo == chunk.lo && d.hi == chunk.hi)
                        .ok_or("c5_digest_missing")?
                        .clone(),
                );
            }
            for chunk in read_chunks {
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
            if epoch::c5_lineage(
                engine
                    .epoch(source, store, std::slice::from_ref(table))
                    .await?,
                engine.shape(table).await?,
            ) != live_lineage
            {
                return Err("c5_rebaseline_required_lineage".into());
            }
            target
                .publish_c5(&previous, &plan, digests, &receipts, &live_lineage)
                .await?;
        }
        let after = target.generations().await?;
        let retained_run = target.c5_baseline().await?.run;
        let duty = u32::from(budget.duty_cycle_percent);
        if !(1..=100).contains(&duty) {
            return Err("c5_budget".into());
        }
        tokio::time::sleep(started.elapsed() * (100 - duty) / duty).await;
        // Small changed ranges also need a floor between metadata bursts.
        tokio::time::sleep(Duration::from_secs(1)).await;
        println!(
            "{}",
            serde_json::json!({
                "protocol":"oo.sync/v1","source":source.id,"mode":"c5","table":table,
                "run_id":config.run_id,
                "retained_run":retained_run,
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_connectors::engine::{ChunkSpec, KeyShape, KeyValue};

    fn plan(bounds: &[i64], key: &str) -> TableChunkPlan {
        let table = TableRef {
            schema: "fixture".into(),
            name: "bulk".into(),
        };
        let mut edges: Vec<Option<Vec<KeyValue>>> = vec![None];
        edges.extend(
            bounds
                .iter()
                .map(|b| Some(vec![KeyValue::Int((*b).into())])),
        );
        edges.push(None);
        TableChunkPlan {
            protocol: "p".into(),
            table: table.clone(),
            key: KeyShape::IntegerPrimaryKey { column: key.into() },
            strategy: ChunkStrategy::Keyset {
                columns: vec![key.into()],
            },
            estimated_rows: Some(1),
            chunks: edges
                .windows(2)
                .enumerate()
                .map(|(i, w)| ChunkSpec {
                    table: table.clone(),
                    plan_revision: 0,
                    chunk_id: i as u32,
                    lo: w[0].clone(),
                    hi: w[1].clone(),
                    estimated_rows: None,
                })
                .collect(),
        }
    }

    #[test]
    fn estimate_drift_keeps_durable_ranges_but_key_change_rebaselines() {
        let baseline = plan(&[500, 1000], "id");
        // A drifting optimizer estimate or a new MAX(id) moves live boundaries.
        let kept = durable_ranges(&baseline, plan(&[507, 1014, 1521], "id"));
        assert!(oo_pipeline::engine_load_pg::same_ranges(&baseline, &kept));
        let changed = durable_ranges(&baseline, plan(&[500, 1000], "other"));
        assert!(!oo_pipeline::engine_load_pg::same_ranges(
            &baseline, &changed
        ));
        let mut single = baseline.clone();
        single.strategy = ChunkStrategy::SingleRead;
        assert!(!oo_pipeline::engine_load_pg::same_ranges(
            &baseline,
            &durable_ranges(&baseline, single)
        ));
    }
    #[test]
    fn hints_never_replace_digest_comparison_and_full_reads_detect_collisions() {
        let old = RangeDigest {
            table: TableRef {
                schema: "fixture".into(),
                name: "items".into(),
            },
            lo: None,
            hi: None,
            rows: 1,
            digest: "old".into(),
        };
        let mut new = old.clone();
        assert!(!needs_read(Some(&old), &new, false, false));
        assert!(needs_read(Some(&old), &new, true, false));
        assert!(needs_read(Some(&old), &new, false, true));
        new.digest = "changed_same_count".into();
        assert!(needs_read(Some(&old), &new, false, false));
        assert!(needs_read(None, &new, false, false));
    }
}
