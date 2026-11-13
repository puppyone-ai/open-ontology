use super::*;
use oo_connectors::engine::{ChunkStrategy, KeyShape, RangeDigest, TableChunkPlan};
use oo_pipeline::engine_load::{Capture, Target};
use std::collections::BTreeSet;

#[path = "c5_keyless.rs"]
mod keyless;

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

async fn legacy_proven<E: epoch::Epoch>(
    engine: &mut E,
    source: &Source,
    store: &SecretStore,
    table: &TableRef,
    stored: &str,
) -> Result<bool, String> {
    Ok(engine
        .legacy_lineage(source, store, table)
        .await?
        .is_some_and(|legacy| legacy == stored))
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

pub(super) async fn run_cycle<E: epoch::Epoch>(
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
    use oo_pipeline::engine_isolation;
    let source_binding = binding(source)?;
    let mut cycle_conn = target_connection(config, store).await?;
    let pending =
        engine_isolation::begin(&mut cycle_conn, &source_binding, &config.run_id, tables).await?;
    // Probe failures are source-wide, even when their error kind is privilege.
    engine.probe().await.map_err(|e| e.to_string())?;
    if !pending {
        for mut output in
            engine_isolation::replay(&mut cycle_conn, &source_binding, &config.run_id).await?
        {
            output["protocol"] = "oo.sync/v1".into();
            output["source"] = source.id.clone().into();
            output["mode"] = "c5".into();
            output["run_id"] = config.run_id.clone().into();
            output["gateway_published"] = false.into();
            println!("{output}");
        }
    }
    for table in tables.iter().filter(|_| pending) {
        let mut attempt = 0;
        let output = loop {
            attempt += 1;
            let mut conn = target_connection(config, store).await?;
            let result = run_table(
                engine, source, table, config, store, budget, &policy, &mut conn,
            )
            .await;
            // Release the table lock before retry, including after failed reads.
            conn.close().await.map_err(|_| "c5_target_close")?;
            match result {
                Ok(output) => break output,
                Err(error) => match engine_isolation::stale_reason(&error) {
                    None => return Err(error),
                    Some(_) if attempt == 1 => continue,
                    Some(reason) => {
                        break serde_json::json!({
                            "protocol":"oo.sync/v1", "source":source.id,"mode":"c5","table":table,
                            "run_id":config.run_id,"status":"stale","stale_reason":reason,
                            "stale_detail":"Table failed twice; previous retained generation preserved.",
                            "attempts":attempt,"gateway_published":false
                        });
                    }
                },
            }
        };
        engine_isolation::record(
            &mut cycle_conn,
            &source_binding,
            &config.run_id,
            table,
            output["status"].as_str().ok_or("sync_table_state")?,
            output["stale_reason"].as_str(),
        )
        .await?;
        println!("{output}");
    }
    let code = engine_isolation::finish(&mut cycle_conn, &source_binding, &config.run_id).await?;
    cycle_conn.close().await.map_err(|_| "c5_target_close")?;
    match code {
        0 => Ok(()),
        3 => Err("sync_partial".into()),
        _ => Err("sync_all_stale".into()),
    }
}

async fn run_table<E: epoch::Epoch>(
    engine: &mut E,
    source: &Source,
    table: &TableRef,
    config: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    policy: &Config,
    conn: &mut PgConnection,
) -> Result<serde_json::Value, String> {
    let started = std::time::Instant::now();
    engine.prepare_table(table, budget).await?;
    let mut target = PgTarget::new(conn, &binding(source)?, &config.run_id, table).await?;
    target = documents::target(target, source)?;
    target.read_metadata(read_metadata(engine, budget));
    let mut sealed = target.sealed_columns().await?;
    let live_columns = engine
        .column_shape(table, budget)
        .await
        .map_err(|e| e.to_string())?;
    let mut schema_upgrade = None;
    if sealed.is_none()
        && let Some(live) = live_columns.as_deref()
    {
        // Upgrade from the pre-0129 layout: no catalog, but the stored C5
        // lineage still folds in the table shape the old fence accepted.
        // Seal only when that exact lineage is reproduced now; never infer.
        let previous = target.c5_baseline().await?;
        if let Some(old) = previous.lineage.as_deref()
            && legacy_proven(engine, source, store, table, old).await?
        {
            let lineage = engine
                .epoch(source, store, std::slice::from_ref(table))
                .await
                .map_err(|_| "c5_source_epoch")?;
            target
                .adopt_legacy_catalog(Some(&previous.run), live, Some((old, &lineage)))
                .await?;
            sealed = target.sealed_columns().await?;
            schema_upgrade = Some("legacy_lineage_sealed");
        }
    }
    let classification = oo_load::schema::classify(
        sealed.as_ref().map(|s| s.columns.as_slice()),
        live_columns.as_deref(),
        sealed
            .as_ref()
            .map(|s| s.projection.as_slice())
            .unwrap_or(&[]),
    );
    if let Some(reason) = classification.stale_reason() {
        return Err(reason);
    }
    let projection = sealed.as_ref().ok_or("schema_unproven")?.projection.clone();
    target.published_projection(projection);
    target.source_columns(live_columns.clone());
    let live_lineage = engine
        .epoch(source, store, std::slice::from_ref(table))
        .await
        .map_err(|_| "c5_source_epoch")?;
    let live_plan = engine
        .plan_chunks(table, budget)
        .await
        .map_err(|e| e.to_string())?;
    let mut previous = target.c5_baseline().await?;
    if let Some(old) = previous.lineage.clone()
        && old != live_lineage
    {
        // A catalog sealed before the lineage format changed: the same proof.
        if !legacy_proven(engine, source, store, table, &old).await? {
            return Err("c5_rebaseline_required_lineage".into());
        }
        target
            .adopt_legacy_lineage(&previous.run, &old, &live_lineage)
            .await?;
        previous = target.c5_baseline().await?;
        schema_upgrade = Some("legacy_lineage_rewritten");
    }
    if classification == oo_load::schema::Classification::Rebuild {
        let before = target.generations().await?;
        target.source_columns(live_columns);
        engine_load::load(engine, table, budget, oo_load::IpcSink::new, &mut target).await?;
        let after = target.generations().await?;
        return Ok(serde_json::json!({
            "protocol":"oo.sync/v1", "source":source.id, "mode":"c5", "table":table,
            "run_id":config.run_id, "status":"fresh", "schema_class":"rebuild",
            "retained_run":config.run_id, "new_generations":after-before,
            "generation":after, "gateway_published":false,
            "read_metadata":read_metadata(engine, budget),
            "target_write_bytes":target.write_bytes().await?
        }));
    }
    let mut plan = durable_ranges(&previous.plan, live_plan);
    let keyless = previous.plan.key == KeyShape::NoKey && plan.key == KeyShape::NoKey;
    if !keyless && !oo_pipeline::engine_load_pg::same_ranges(&previous.plan, &plan) {
        return Err("c5_rebaseline_required_plan".into());
    }
    if !keyless {
        engine.adopt_plan(&plan)?;
    }
    let before = target.generations().await?;
    let mut receipts = Vec::new();
    let mut payload_bytes = 0u64;
    let mut source_read_bytes = 0u64;
    let replay = target.completed().await?.is_some();
    if !replay && keyless {
        let (digest, replacements, read_bytes) =
            keyless::read(engine, &plan, &previous, budget, &mut target, policy).await?;
        receipts = replacements;
        source_read_bytes = read_bytes;
        payload_bytes = receipts.iter().map(|receipt| receipt.bytes).sum();
        if engine
            .epoch(source, store, std::slice::from_ref(table))
            .await
            .map_err(|_| "c5_source_epoch")?
            != live_lineage
            || engine
                .column_shape(table, budget)
                .await
                .map_err(|e| e.to_string())?
                != live_columns
        {
            return Err("c5_rebaseline_required_lineage".into());
        }
        target
            .publish_c5(&previous, &plan, vec![digest], &receipts, &live_lineage)
            .await?;
    } else if !replay {
        let mut source_digests = Vec::new();
        let mut read_chunks = Vec::new();
        let mut grown = Vec::new();
        for chunk in ordered_chunks(&plan, policy)? {
            let digest = engine
                .digest_range(table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await
                .map_err(|e| e.to_string())?;
            if digest.rows > budget.max_chunk_rows {
                // Durable ranges never move, so an append tail grows without
                // bound; reading it whole would fail the chunk budget forever.
                grown.push(chunk);
                continue;
            }
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
        let mut children = Vec::new();
        for chunk in grown {
            for (child, digest) in split_grown(engine, table, budget, &mut plan, chunk).await? {
                children.push(child);
                source_digests.push(digest);
            }
        }
        // Read split children first, then the open-ended tail: an append tail
        // keeps growing until it is read, and a long cycle (period 1 reads
        // every range at the paced rate) can push it past the chunk budget.
        read_chunks.sort_by_key(|c| c.hi.is_some());
        children.append(&mut read_chunks);
        let read_chunks = children;
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
            let receipt = match engine.read_chunk(&chunk, budget, &mut sink).await {
                Ok(receipt) => receipt,
                // The range outgrew the budget after discovery: the table's
                // retry discovers it again and splits it before reading.
                Err(error) if outgrew(&error) => return Err("c5_range_changed_retry".into()),
                Err(error) => return Err(error.to_string()),
            };
            let ipc = sink.finish()?;
            source_read_bytes += ipc.len() as u64;
            let decoded = oo_pipeline::engine_chunk::decode_chunk(&chunk, &receipt, &ipc, budget)?;
            let proof = engine.take_chunk_proof();
            if matches!(engine.consistency(), "chunk_consistent" | "single") && proof.is_none() {
                return Err("c5_chunk_proof_missing".into());
            }
            let proven = proof.is_some();
            let after = match proof {
                Some(proof) => proof,
                None => engine
                    .digest_range(table, chunk.lo.as_deref(), chunk.hi.as_deref())
                    .await
                    .map_err(|e| e.to_string())?,
            };
            let before_digest = digests
                .iter_mut()
                .find(|d| d.lo == chunk.lo && d.hi == chunk.hi)
                .ok_or("c5_digest_missing")?;
            accept_proof(&chunk, receipt.rows, before_digest, after, proven)?;
            let fresh = !previous
                .plan
                .chunks
                .iter()
                .any(|c| c.chunk_id == chunk.chunk_id);
            let old = previous
                .parts
                .iter()
                .find(|p| p.receipt.chunk_id == receipt.chunk_id);
            // A split child has no parent part; every kept range must have one.
            if fresh == old.is_some() {
                return Err("c5_parent_missing".into());
            }
            if old.is_none_or(|old| {
                old.receipt.sha256 != receipt.sha256 || old.receipt.rows != receipt.rows
            }) {
                if receipts.is_empty() {
                    target.begin(&plan).await?;
                }
                target.stage(&receipt, &decoded).await?;
                payload_bytes += receipt.bytes;
                let matches = match old {
                    Some(old) => target.staged_matches(old, receipt.chunk_id).await?,
                    None => false,
                };
                if classification == oo_load::schema::Classification::Unchanged || !matches {
                    receipts.push(receipt);
                }
            }
        }
        if engine
            .epoch(source, store, std::slice::from_ref(table))
            .await
            .map_err(|_| "c5_source_epoch")?
            != live_lineage
            || engine
                .column_shape(table, budget)
                .await
                .map_err(|e| e.to_string())?
                != live_columns
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
    tokio::time::sleep(cycle_rest(engine.family(), started.elapsed(), duty)).await;
    // Small changed ranges also need a floor between metadata bursts.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let metadata = if replay {
        target.publication_read_metadata().await?
    } else {
        Some(read_metadata(engine, budget))
    };
    let consistency = metadata
        .as_ref()
        .and_then(|m| m["consistency"].as_str())
        .unwrap_or("unmeasured");
    let output = serde_json::json!({
        "protocol":"oo.sync/v1","source":source.id,"mode":"c5","table":table,
        "read_metadata":metadata,
        "status":if replay || after == before {"noop"} else {"fresh"},
        "run_id":config.run_id,
        "retained_run":retained_run,
        "schema_class":classification.as_str(),
        "schema_upgrade":schema_upgrade,
        "target":target.relation(),"replay":replay,"ranges_replaced":receipts.len(),
        "payload_bytes":payload_bytes,"source_read_bytes":source_read_bytes,
        "target_write_bytes":if replay {0} else {target.write_bytes().await?},
        "generation":after,"new_generations":after-before,
        "write_basis":"committed_logical_stage_tuples_excluding_indexes_wal_metadata",
        "publication":"local_table_view","gateway_published":false,
        "consistency":consistency,"source_time":"unmeasured"
    });
    Ok(output)
}

/// A read refused because the range now holds more rows or bytes than one
/// chunk may carry, as opposed to any other budget or source failure.
fn outgrew(error: &oo_connectors::SourceError) -> bool {
    error.kind() == oo_connectors::SourceErrorKind::Budget
        && matches!(
            error.fact(),
            Some("chunk_rows_replan" | "chunk_bytes_replan")
        )
}

/// Bound on budget splits of grown ranges in one table cycle.
const MAX_GROWN_SPLITS: usize = 64;

/// Split a range whose rows exceed the chunk budget until every child fits in
/// half of it, leaving the newest (tail) child room to grow during the cycle.
/// Children replace the parent in the durable plan; all are read and staged.
async fn split_grown<E: epoch::Epoch>(
    engine: &mut E,
    table: &TableRef,
    budget: &ReadBudget,
    plan: &mut TableChunkPlan,
    grown: engine::ChunkSpec,
) -> Result<Vec<(engine::ChunkSpec, RangeDigest)>, String> {
    let fits = budget.max_chunk_rows.div_ceil(2).max(1);
    let mut pending = vec![grown];
    let mut done = Vec::new();
    for _ in 0..MAX_GROWN_SPLITS {
        let Some(chunk) = pending.pop() else {
            return Ok(done);
        };
        let children = engine
            .split_chunk(&chunk, budget)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("c5_split_exhausted")?;
        let children = refine(plan, &chunk, children)?;
        engine.adopt_plan(plan)?;
        for child in children {
            let digest = engine
                .digest_range(table, child.lo.as_deref(), child.hi.as_deref())
                .await
                .map_err(|e| e.to_string())?;
            if digest.rows > fits {
                pending.push(child);
            } else {
                done.push((child, digest));
            }
        }
    }
    if pending.is_empty() {
        Ok(done)
    } else {
        Err("c5_split_exhausted".into())
    }
}

/// Splice children over their parent with fresh ids and a newer revision.
/// Engines number splits from their live plan; durable ids must never repeat.
fn refine(
    plan: &mut TableChunkPlan,
    parent: &engine::ChunkSpec,
    children: Vec<engine::ChunkSpec>,
) -> Result<Vec<engine::ChunkSpec>, &'static str> {
    let index = plan
        .chunks
        .iter()
        .position(|c| c == parent)
        .ok_or("c5_split_scope")?;
    if children.len() < 2
        || children.first().is_none_or(|c| c.lo != parent.lo)
        || children.last().is_none_or(|c| c.hi != parent.hi)
    {
        return Err("c5_split_coverage");
    }
    let mut next = plan
        .chunks
        .iter()
        .map(|c| c.chunk_id)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or("c5_split_ids")?;
    let revision = parent.plan_revision.checked_add(1).ok_or("c5_split_ids")?;
    let mut fresh = Vec::new();
    for child in children {
        fresh.push(engine::ChunkSpec {
            table: parent.table.clone(),
            plan_revision: revision,
            chunk_id: next,
            ..child
        });
        next = next.checked_add(1).ok_or("c5_split_ids")?;
    }
    plan.chunks.splice(index..=index, fresh.clone());
    oo_pipeline::engine_load::validate_plan(plan, &plan.table)?;
    Ok(fresh)
}

fn cycle_rest(family: EngineFamily, elapsed: Duration, duty: u32) -> Duration {
    // These adapters already pace each operation. Counting target work and
    // their sleeps as source work again can add hours of idle source time.
    if matches!(
        family,
        EngineFamily::Postgres | EngineFamily::Mysql | EngineFamily::Kingbase
    ) {
        Duration::ZERO
    } else {
        elapsed * (100 - duty) / duty
    }
}

fn accept_proof(
    chunk: &engine::ChunkSpec,
    rows: u64,
    discovery: &mut RangeDigest,
    proof: RangeDigest,
    same_transaction: bool,
) -> Result<(), String> {
    if proof.table != chunk.table
        || proof.lo != chunk.lo
        || proof.hi != chunk.hi
        || proof.rows != rows
        || (!same_transaction && &proof != discovery)
    {
        return Err("c5_range_changed_retry".into());
    }
    // Discovery selects transfers; the payload's snapshot proves publication.
    *discovery = proof;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_connectors::engine::{ChunkSpec, KeyShape, KeyValue};

    #[test]
    fn a_range_that_outgrew_the_budget_is_retried_not_failed() {
        use oo_connectors::SourceError;
        assert!(outgrew(&SourceError::budget("chunk_rows_replan")));
        assert!(outgrew(&SourceError::budget("chunk_bytes_replan")));
        assert!(!outgrew(&SourceError::budget("client_timeout")));
        assert!(!outgrew(&SourceError::protocol("chunk_rows_replan")));
        assert_eq!(
            oo_pipeline::engine_isolation::stale_reason("c5_range_changed_retry"),
            Some("source_changed")
        );
    }

    #[test]
    fn paced_adapters_do_not_repay_duty_sleep_as_source_work() {
        for family in [
            EngineFamily::Postgres,
            EngineFamily::Mysql,
            EngineFamily::Kingbase,
        ] {
            assert_eq!(
                cycle_rest(family, Duration::from_secs(600), 25),
                Duration::ZERO
            );
        }
        assert_eq!(
            cycle_rest(EngineFamily::Oracle, Duration::from_secs(600), 25),
            Duration::from_secs(1800)
        );
        assert_eq!(
            cycle_rest(EngineFamily::Oracle, Duration::from_secs(600), 100),
            Duration::ZERO
        );
    }

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
    fn grown_tail_split_gets_fresh_ids_and_a_newer_revision() {
        let mut durable = plan(&[500, 1000], "id");
        durable.protocol = oo_connectors::engine::ENGINE_PROTOCOL.into();
        let tail = durable.chunks[2].clone();
        // Engines number children from their live plan: ids 1 and 2 collide.
        let key = |v: i64| Some(vec![KeyValue::Int(v.into())]);
        let engine_children = vec![
            ChunkSpec {
                chunk_id: 1,
                hi: key(1500),
                ..tail.clone()
            },
            ChunkSpec {
                chunk_id: 2,
                lo: key(1500),
                ..tail.clone()
            },
        ];
        let before = durable.clone();
        let children = refine(&mut durable, &tail, engine_children.clone()).unwrap();
        assert_eq!(
            children
                .iter()
                .map(|c| (c.chunk_id, c.plan_revision))
                .collect::<Vec<_>>(),
            [(3, 1), (4, 1)]
        );
        assert_eq!(durable.chunks.len(), 4);
        assert_eq!(durable.chunks[..2], before.chunks[..2]);
        let receipts: Vec<_> = children
            .iter()
            .map(|c| oo_connectors::engine::ChunkReceipt {
                table: c.table.clone(),
                plan_revision: c.plan_revision,
                chunk_id: c.chunk_id,
                rows: 1,
                bytes: 1,
                sha256: String::new(),
                low: None,
                high: None,
            })
            .collect();
        assert!(oo_pipeline::engine_load_pg::refines(
            &before, &durable, &receipts
        ));
        // Children must cover exactly the parent range.
        let mut gap = before.clone();
        let mut short = engine_children.clone();
        short[1].hi = key(2000);
        assert_eq!(refine(&mut gap, &tail, short), Err("c5_split_coverage"));
        assert_eq!(
            refine(&mut gap, &tail, engine_children[..1].to_vec()),
            Err("c5_split_coverage")
        );
        // A chunk that is not in the durable plan cannot be split.
        let mut foreign = tail.clone();
        foreign.chunk_id = 9;
        assert_eq!(
            refine(&mut gap, &foreign, engine_children),
            Err("c5_split_scope")
        );
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

    #[test]
    fn payload_proof_wins_over_discovery_and_next_cycle_corrects() {
        let plan = plan(&[5], "id");
        let chunk = &plan.chunks[0];
        let digest = |rows, value: &str| RangeDigest {
            table: chunk.table.clone(),
            lo: chunk.lo.clone(),
            hi: chunk.hi.clone(),
            rows,
            digest: value.into(),
        };
        for (rows, value) in [(2, "updated"), (1, "deleted"), (1, "key_moved")] {
            let a = digest(2, "original");
            let b = digest(rows, value);
            let mut published = a.clone();
            // Discovery A, payload B, source back to A: publish B's proof, not A.
            accept_proof(chunk, rows, &mut published, b.clone(), true).unwrap();
            assert_eq!(published, b);
            assert!(needs_read(Some(&published), &a, false, false));
            accept_proof(chunk, 2, &mut published, a.clone(), true).unwrap();
            assert_eq!(published, a);
            assert!(!needs_read(Some(&published), &a, false, false));
        }
    }

    #[test]
    fn mismatched_or_unbound_proof_cannot_replace_discovery() {
        let plan = plan(&[5], "id");
        let chunk = &plan.chunks[0];
        let before = RangeDigest {
            table: chunk.table.clone(),
            lo: chunk.lo.clone(),
            hi: chunk.hi.clone(),
            rows: 2,
            digest: "a".into(),
        };
        let mut changed = before.clone();
        changed.digest = "b".into();
        assert!(accept_proof(chunk, 2, &mut before.clone(), changed, false).is_err());
        assert!(accept_proof(chunk, 1, &mut before.clone(), before.clone(), true).is_err());
        let mut wrong_range = before.clone();
        wrong_range.hi = None;
        assert!(accept_proof(chunk, 2, &mut before.clone(), wrong_range, true).is_err());
    }
}
