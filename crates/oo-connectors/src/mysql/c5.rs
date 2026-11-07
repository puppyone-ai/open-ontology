//! Source-side C5 cycles. Hints change priority, never coverage. A proposal
//! becomes the comparison baseline only after its consumer confirms it.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use arrow_array::RecordBatch;

use crate::SourceError;
use crate::engine::{ChunkSink, ChunkSpec, RangeDigest, ReadBudget, SourceEngine};

type Result<T> = std::result::Result<T, SourceError>;

/// These are positive hints supplied by the time/recent-window observer.
/// Missing or NULL timestamps mean no hint, not permission to skip a range.
#[derive(Default)]
pub struct Hints {
    pub time: BTreeSet<u32>,
    pub recent: BTreeSet<u32>,
}

#[derive(Clone)]
struct Baseline {
    digest: RangeDigest,
    full_sha256: String,
}

/// One binding's fixed, complete range plan. Replanning starts a new baseline.
pub struct Cycle {
    identity: Arc<()>,
    chunks: Vec<ChunkSpec>,
    baseline: BTreeMap<u32, Baseline>,
    revision: u64,
    full_every: u64,
}

/// A replacement range, including an empty replacement after deletion.
pub struct Replacement {
    pub chunk: ChunkSpec,
    pub batches: Vec<RecordBatch>,
    pub rows: u64,
}

/// No replacement means no payload to load or publish. This is not a target
/// receipt: target writes and generation publication belong to the loader.
pub struct Proposal {
    identity: Arc<()>,
    revision: u64,
    chunks: Vec<ChunkSpec>,
    next: BTreeMap<u32, Baseline>,
    pub replacements: Vec<Replacement>,
    pub digest_reads: usize,
    pub full_reads: usize,
}

#[derive(Default)]
struct Buffer(Vec<RecordBatch>);
impl ChunkSink for Buffer {
    fn push(&mut self, batch: RecordBatch) -> Result<()> {
        self.0.push(batch);
        Ok(())
    }
}

impl Cycle {
    pub fn new(chunks: Vec<ChunkSpec>, full_every: u64) -> Result<Self> {
        if full_every == 0 || chunks.is_empty() {
            return Err(SourceError::config("c5_plan"));
        }
        let first = &chunks[0];
        let mut ids = BTreeSet::new();
        for (i, chunk) in chunks.iter().enumerate() {
            if chunk.table != first.table
                || chunk.plan_revision != first.plan_revision
                || !ids.insert(chunk.chunk_id)
                || (i == 0 && chunk.lo.is_some())
                || (i > 0 && chunks[i - 1].hi != chunk.lo)
                || (i + 1 < chunks.len() && chunk.hi.is_none())
            {
                return Err(SourceError::config("c5_plan"));
            }
        }
        if chunks.last().is_some_and(|c| c.hi.is_some()) {
            return Err(SourceError::config("c5_plan"));
        }
        Ok(Self {
            identity: Arc::new(()),
            chunks,
            baseline: BTreeMap::new(),
            revision: 0,
            full_every,
        })
    }

    pub async fn prepare(
        &self,
        engine: &mut impl SourceEngine,
        budget: &ReadBudget,
        hints: &Hints,
    ) -> Result<Proposal> {
        if hints
            .time
            .union(&hints.recent)
            .any(|id| !self.chunks.iter().any(|c| c.chunk_id == *id))
        {
            return Err(SourceError::config("c5_hint_range"));
        }
        let mut order: Vec<_> = self.chunks.iter().collect();
        order.sort_by_key(|c| {
            (
                if hints.time.contains(&c.chunk_id) {
                    0
                } else if hints.recent.contains(&c.chunk_id) {
                    1
                } else {
                    2
                },
                c.chunk_id,
            )
        });
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| SourceError::budget("c5_revision"))?;
        let full = next_revision.is_multiple_of(self.full_every);
        let mut proposal = Proposal {
            identity: self.identity.clone(),
            revision: self.revision,
            chunks: self.chunks.clone(),
            next: self.baseline.clone(),
            replacements: Vec::new(),
            digest_reads: 0,
            full_reads: 0,
        };
        for chunk in order {
            let digest = engine
                .digest_range(&chunk.table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await?;
            proposal.digest_reads += 1;
            let old = self.baseline.get(&chunk.chunk_id);
            let changed = old.is_none_or(|old| old.digest != digest);
            if !changed && !full {
                continue;
            }
            let mut buffer = Buffer::default();
            let receipt = engine.read_chunk(chunk, budget, &mut buffer).await?;
            proposal.full_reads += 1;
            let after = engine
                .digest_range(&chunk.table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await?;
            proposal.digest_reads += 1;
            // Never acknowledge a digest from before a concurrent source edit.
            // This detects drift, but does not claim a cross-table snapshot.
            if digest != after || receipt.rows != digest.rows {
                return Err(SourceError::protocol("c5_range_changed_retry"));
            }
            if changed || old.is_none_or(|old| old.full_sha256 != receipt.sha256) {
                proposal.replacements.push(Replacement {
                    chunk: chunk.clone(),
                    batches: buffer.0,
                    rows: receipt.rows,
                });
            }
            proposal.next.insert(
                chunk.chunk_id,
                Baseline {
                    digest,
                    full_sha256: receipt.sha256,
                },
            );
        }
        Ok(proposal)
    }

    /// Call only after durable application (or durable confirmation of no-op).
    /// A failed or abandoned proposal leaves the previous baseline intact.
    pub fn confirm(&mut self, proposal: Proposal) -> Result<()> {
        if !Arc::ptr_eq(&proposal.identity, &self.identity)
            || proposal.revision != self.revision
            || proposal.chunks != self.chunks
        {
            return Err(SourceError::protocol("c5_stale_proposal"));
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| SourceError::budget("c5_revision"))?;
        self.baseline = proposal.next;
        Ok(())
    }
}
