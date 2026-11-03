//! C5 scheduling over source range digests; never a timestamp CDC cursor.
use std::collections::BTreeSet;

use super::PostgresEngine;
use crate::SourceError;
use crate::engine::{ChunkSpec, ChunkStrategy, RangeDigest, SourceEngine, TableRef};

/// Caller persists this policy's cycle number only after successful application.
pub struct C5Cycle {
    pub cycle: u64,
    pub full_read_every: u64,
    /// Positive timestamp evidence only. NULL and no match must not be inserted.
    pub positive_hints: BTreeSet<u32>,
    /// Recently active chunks are checked first, never instead of other chunks.
    pub recent_chunks: BTreeSet<u32>,
}

pub struct C5Inspection {
    /// Persist as source evidence only after all required reads/applications pass.
    pub source_digests: Vec<RangeDigest>,
    /// Empty means no row payload, rewrite or generation should be requested.
    pub read_chunks: Vec<ChunkSpec>,
    pub full_read_due: bool,
}

impl PostgresEngine {
    /// Inspect every issued range; compare only with the last successful source
    /// digests. Target digests and timestamp cursors are not accepted substitutes.
    /// This returns a plan, not an atomic target commit or publication receipt.
    pub async fn inspect_c5(
        &mut self,
        table: &TableRef,
        previous_source: &[RangeDigest],
        cycle: &C5Cycle,
    ) -> Result<C5Inspection, SourceError> {
        if cycle.full_read_every == 0 || previous_source.iter().any(|d| d.table != *table) {
            return Err(SourceError::config("c5_policy_or_source_scope"));
        }
        for (i, a) in previous_source.iter().enumerate() {
            if previous_source[..i]
                .iter()
                .any(|b| a.lo == b.lo && a.hi == b.hi)
            {
                return Err(SourceError::config("c5_duplicate_source_range"));
            }
        }
        let plan = &self
            .plans
            .get(table)
            .ok_or_else(|| SourceError::protocol("unknown_plan"))?
            .plan;
        if matches!(plan.strategy, ChunkStrategy::PhysicalRange { .. }) {
            return Err(SourceError::protocol("c5_physical_ranges_not_durable"));
        }
        let mut chunks = plan.chunks.clone();
        if cycle
            .positive_hints
            .union(&cycle.recent_chunks)
            .any(|id| !chunks.iter().any(|c| c.chunk_id == *id))
        {
            return Err(SourceError::config("c5_unknown_hint_chunk"));
        }
        chunks.sort_by_key(|c| {
            (
                !cycle.positive_hints.contains(&c.chunk_id),
                !cycle.recent_chunks.contains(&c.chunk_id),
                c.chunk_id,
            )
        });
        let full_read_due = cycle.cycle.is_multiple_of(cycle.full_read_every);
        let mut inspection = C5Inspection {
            source_digests: Vec::new(),
            read_chunks: Vec::new(),
            full_read_due,
        };
        for chunk in chunks {
            let digest = self
                .digest_range(table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await?;
            let previous = previous_source
                .iter()
                .find(|d| d.lo == digest.lo && d.hi == digest.hi);
            if needs_read(
                previous,
                &digest,
                cycle.positive_hints.contains(&chunk.chunk_id),
                full_read_due,
            ) {
                inspection.read_chunks.push(chunk);
            }
            inspection.source_digests.push(digest);
        }
        Ok(inspection)
    }
}

fn needs_read(
    previous: Option<&RangeDigest>,
    current: &RangeDigest,
    positive: bool,
    full: bool,
) -> bool {
    positive || full || previous != Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(value: &str) -> RangeDigest {
        RangeDigest {
            table: TableRef {
                schema: "fixture".into(),
                name: "items".into(),
            },
            lo: None,
            hi: None,
            rows: 3,
            digest: value.into(),
        }
    }

    #[test]
    fn equal_counts_and_absent_hints_never_prove_unchanged() {
        let old = digest("old");
        let new = digest("new");
        assert!(needs_read(Some(&old), &new, false, false));
        assert!(needs_read(None, &old, false, false));
        assert!(!needs_read(Some(&old), &old, false, false));
        assert!(needs_read(Some(&old), &old, true, false));
        assert!(needs_read(Some(&old), &old, false, true));
    }
}
