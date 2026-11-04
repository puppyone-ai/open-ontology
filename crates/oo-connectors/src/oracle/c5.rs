//! Oracle-local C5 scheduling. Hints only reorder work, never exclude ranges.
//! A cycle is staged in memory and its state advances only after all reads pass.
use super::*;
use arrow_array::RecordBatch;
#[cfg(test)]
use arrow_array::{ArrayRef, StringArray};

#[derive(Default, Serialize)]
pub struct C5Metrics {
    pub time_hints: usize,
    pub recent_ranges: usize,
    pub recent_unknown: usize,
    pub digests: usize,
    pub full_reads: usize,
    /// Row bytes read from Oracle, including unchanged full-read verification.
    pub source_read_bytes: u64,
    /// Row bytes handed to the consumer as replacements.
    pub payload_bytes: u64,
    pub rewrites: usize,
    pub generation: u64,
}

pub struct C5Cycle {
    pub metrics: C5Metrics,
    pub replacements: Vec<(ChunkSpec, Vec<RecordBatch>)>,
}

#[derive(Clone)]
struct SavedRange {
    digest: RangeDigest,
    content: [u8; 32],
}

pub struct OracleC5 {
    plan: TableChunkPlan,
    time_column: Option<String>,
    full_every: u64,
    cycles: u64,
    generation: u64,
    previous_scn: Option<u64>,
    previous_hints: BTreeMap<u32, Option<String>>,
    saved: BTreeMap<u32, SavedRange>,
}

impl OracleC5 {
    /// `full_every` bounds digest-collision detection. Use a validated time
    /// column when available; NULL/absent hints and lost undo mean unknown.
    pub fn new(
        plan: TableChunkPlan,
        time_column: Option<String>,
        full_every: u64,
    ) -> Result<Self, SourceError> {
        if full_every < 2
            || plan.chunks.is_empty()
            || !matches!(plan.strategy, ChunkStrategy::Keyset { .. })
            || plan.chunks.iter().any(|c| c.table != plan.table)
        {
            // Physical plans pin an old SCN and must be replanned by the caller.
            return Err(SourceError::config("c5_requires_keyset_and_full_interval"));
        }
        Ok(Self {
            plan,
            time_column,
            full_every,
            cycles: 0,
            generation: 0,
            previous_scn: None,
            previous_hints: BTreeMap::new(),
            saved: BTreeMap::new(),
        })
    }

    pub async fn cycle(
        &mut self,
        engine: &mut OracleEngine,
        budget: &ReadBudget,
    ) -> Result<C5Cycle, SourceError> {
        validate_budget(budget)?;
        let state = engine
            .tables
            .get(&self.plan.table)
            .ok_or(SourceError::catalog("table_not_planned"))?;
        if self
            .plan
            .chunks
            .iter()
            .any(|chunk| chunk.plan_revision != state.facts.plan_revision)
        {
            return Err(SourceError::catalog("c5_plan_changed_rebaseline"));
        }
        let meta = state.meta.clone();
        engine.set_timeout(Duration::from_millis(budget.statement_timeout_ms))?;
        let high = engine.current_scn()?;
        if self.previous_scn.is_some_and(|low| low > high) {
            return Err(SourceError::catalog("c5_scn_regressed_rebaseline"));
        }
        let mut metrics = C5Metrics::default();
        let time_column = self
            .time_column
            .as_ref()
            .map(|name| {
                meta.columns
                    .iter()
                    .find(|c| &c.name == name)
                    .filter(|c| {
                        matches!(
                            c.kind,
                            ColKind::Date | ColKind::Timestamp | ColKind::TimestampTz
                        )
                    })
                    .ok_or(SourceError::config("c5_time_column"))
            })
            .transpose()?;
        let mut hints = BTreeMap::new();
        let mut ordered = Vec::new();
        for chunk in &self.plan.chunks {
            let hint = if let Some(column) = time_column {
                let keys = meta.keys();
                let mut sql = Sql::default();
                let predicate = keyset::range_predicate(
                    &mut sql,
                    &keys,
                    chunk.lo.as_deref().map(|values| Bound {
                        values,
                        inclusive: true,
                    }),
                    chunk.hi.as_deref().map(|values| Bound {
                        values,
                        inclusive: false,
                    }),
                )?;
                sql.text = format!(
                    "SELECT MAX({}) FROM {} WHERE {}",
                    keyset::projection(column)?,
                    keyset::table_name(&chunk.table.schema, &chunk.table.name)?,
                    if predicate.is_empty() {
                        "1=1"
                    } else {
                        &predicate
                    }
                );
                engine.protect().await?;
                let started = Instant::now();
                let hint = engine.scalar(&sql.text, &sql.binds)?;
                engine.schedule_next(started.elapsed(), budget);
                engine.pace().await;
                hint
            } else {
                None
            };
            let hinted = hint.is_some() && self.previous_hints.get(&chunk.chunk_id) != Some(&hint);
            metrics.time_hints += usize::from(hinted);
            hints.insert(chunk.chunk_id, hint);
            let recent = if let Some(low) = self.previous_scn {
                engine.protect().await?;
                let started = Instant::now();
                let keys = meta.keys();
                let mut sql = Sql::default();
                let predicate = keyset::range_predicate(
                    &mut sql,
                    &keys,
                    chunk.lo.as_deref().map(|values| Bound {
                        values,
                        inclusive: true,
                    }),
                    chunk.hi.as_deref().map(|values| Bound {
                        values,
                        inclusive: false,
                    }),
                )?;
                sql.text = format!(
                    "SELECT 'changed' FROM {} VERSIONS BETWEEN SCN {low} AND {high} \
                     WHERE (versions_startscn > {low} OR versions_endscn <= {high}) \
                     AND ROWNUM=1{}",
                    keyset::table_name(&chunk.table.schema, &chunk.table.name)?,
                    if predicate.is_empty() {
                        String::new()
                    } else {
                        format!(" AND {predicate}")
                    }
                );
                engine.last_ora = None;
                let recent = engine.query(&sql.text, &sql.binds, 1);
                engine.schedule_next(started.elapsed(), budget);
                engine.pace().await;
                match recent {
                    Ok(rows) => !rows.is_empty(),
                    Err(error)
                        if matches!(engine.last_ora_code(), Some(1555 | 8180 | 8181))
                            || error.fact() == Some("changed_keys_needs_key") =>
                    {
                        metrics.recent_unknown += 1;
                        false
                    }
                    Err(error) => return Err(error),
                }
            } else {
                metrics.recent_unknown += 1;
                false
            };
            metrics.recent_ranges += usize::from(recent);
            ordered.push((!hinted, !recent, chunk.clone()));
        }
        ordered.sort_by_key(|(not_hinted, not_recent, chunk)| {
            (*not_hinted, *not_recent, chunk.chunk_id)
        });
        let full = self.cycles > 0 && self.cycles % self.full_every == 0;
        let mut saved = self.saved.clone();
        let mut replacements = Vec::new();
        for (_, _, chunk) in ordered {
            let digest = engine
                .digest_range(&chunk.table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await?;
            metrics.digests += 1;
            let old = self.saved.get(&chunk.chunk_id);
            if full || old.is_none_or(|s| s.digest != digest) {
                let mut sink = Capture::default();
                let receipt = engine.read_chunk(&chunk, budget, &mut sink).await?;
                metrics.source_read_bytes += receipt.bytes;
                metrics.full_reads += usize::from(full);
                if digest
                    != engine
                        .digest_range(&chunk.table, chunk.lo.as_deref(), chunk.hi.as_deref())
                        .await?
                {
                    return Err(SourceError::catalog("c5_range_changed_retry"));
                }
                let content = sink.content();
                if old.is_none_or(|s| s.content != content) {
                    metrics.payload_bytes += receipt.bytes;
                    replacements.push((chunk.clone(), sink.batches));
                }
                saved.insert(chunk.chunk_id, SavedRange { digest, content });
            }
        }
        metrics.rewrites = replacements.len();
        if metrics.rewrites > 0 {
            self.generation += 1;
        }
        metrics.generation = self.generation;
        self.saved = saved;
        self.previous_scn = Some(high);
        self.previous_hints = hints;
        self.cycles += 1;
        Ok(C5Cycle {
            metrics,
            replacements,
        })
    }
}

#[derive(Default)]
struct Capture {
    batches: Vec<RecordBatch>,
    hashes: Vec<[u8; 32]>,
}

impl Capture {
    fn content(&mut self) -> [u8; 32] {
        self.hashes.sort_unstable();
        let mut digest = Sha256::new();
        for hash in &self.hashes {
            digest.update(hash);
        }
        digest.finalize().into()
    }
}

impl ChunkSink for Capture {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        for row in 0..batch.num_rows() {
            let mut digest = Sha256::new();
            for col in batch.columns() {
                super::super::typed::hash_cell(&mut digest, col, row)?;
            }
            self.hashes.push(digest.finalize().into());
        }
        self.batches.push(batch);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_is_order_independent_and_preserves_multiplicity() {
        let mut a = Capture {
            hashes: vec![[1; 32], [2; 32]],
            ..Default::default()
        };
        let mut b = Capture {
            hashes: vec![[2; 32], [1; 32]],
            ..Default::default()
        };
        assert_eq!(a.content(), b.content());
        b.hashes.push([1; 32]);
        assert_ne!(a.content(), b.content());
    }

    #[test]
    fn content_distinguishes_null_empty_and_cell_boundaries() {
        fn content(cells: Vec<Option<&str>>) -> [u8; 32] {
            let arrays = cells
                .into_iter()
                .enumerate()
                .map(|(i, value)| {
                    (
                        i.to_string(),
                        Arc::new(StringArray::from(vec![value])) as ArrayRef,
                    )
                })
                .collect::<Vec<_>>();
            let batch = RecordBatch::try_from_iter(arrays).unwrap();
            let mut sink = Capture::default();
            sink.push(batch).unwrap();
            sink.content()
        }
        assert_ne!(content(vec![None]), content(vec![Some("")]));
        assert_ne!(
            content(vec![Some("ab"), Some("c")]),
            content(vec![Some("a"), Some("bc")])
        );
    }
}
