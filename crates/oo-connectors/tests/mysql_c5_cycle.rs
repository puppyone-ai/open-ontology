#![cfg(feature = "mysql")]

use oo_connectors::SourceError;
use oo_connectors::engine::*;
use oo_connectors::mysql::c5::{Cycle, Hints};

struct Source {
    value: u64,
    collision: bool,
    race: bool,
    order: Vec<u32>,
}

fn chunks() -> Vec<ChunkSpec> {
    (0..3)
        .map(|id| ChunkSpec {
            table: TableRef {
                schema: "fixture".into(),
                name: "events".into(),
            },
            plan_revision: 1,
            chunk_id: id,
            lo: (id > 0).then(|| vec![KeyValue::Int(i128::from(id))]),
            hi: (id < 2).then(|| vec![KeyValue::Int(i128::from(id + 1))]),
            estimated_rows: None,
        })
        .collect()
}

impl SourceEngine for Source {
    fn family(&self) -> EngineFamily {
        EngineFamily::Mysql
    }
    async fn probe(&mut self) -> Result<ProbeReport, SourceError> {
        panic!("cycle must not probe")
    }
    async fn plan_chunks(
        &mut self,
        _: &TableRef,
        _: &ReadBudget,
    ) -> Result<TableChunkPlan, SourceError> {
        panic!("cycle must not replan")
    }
    async fn watermark(&mut self) -> Result<Option<Watermark>, SourceError> {
        Ok(None)
    }
    async fn digest_range(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest, SourceError> {
        let id = match lo {
            Some([KeyValue::Int(n)]) => *n as u32,
            _ => 0,
        };
        self.order.push(id);
        Ok(RangeDigest {
            table: table.clone(),
            lo: lo.map(<[_]>::to_vec),
            hi: hi.map(<[_]>::to_vec),
            rows: 0,
            digest: if self.collision { 0 } else { self.value }.to_string(),
        })
    }
    async fn read_chunk(
        &mut self,
        c: &ChunkSpec,
        _: &ReadBudget,
        _: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt, SourceError> {
        let result = ChunkReceipt {
            table: c.table.clone(),
            plan_revision: c.plan_revision,
            chunk_id: c.chunk_id,
            rows: 0,
            bytes: 0,
            sha256: self.value.to_string(),
            low: None,
            high: None,
        };
        if self.race {
            self.value += 1;
        }
        Ok(result)
    }
}

fn source() -> Source {
    Source {
        value: 0,
        collision: false,
        race: false,
        order: Vec::new(),
    }
}

#[tokio::test]
async fn hints_prioritize_without_skipping_and_noop_emits_nothing() {
    let mut source = source();
    let mut cycle = Cycle::new(chunks(), 10).unwrap();
    let hints = Hints {
        time: [2].into(),
        recent: [1].into(),
    };
    let budget = ReadBudget::default();
    let first = cycle.prepare(&mut source, &budget, &hints).await.unwrap();
    assert_eq!(source.order, [2, 2, 1, 1, 0, 0]);
    assert_eq!(first.replacements.len(), 3);
    cycle.confirm(first).unwrap();
    source.value = 1;
    let changed = cycle
        .prepare(&mut source, &budget, &Hints::default())
        .await
        .unwrap();
    assert_eq!(changed.replacements.len(), 3);
    cycle.confirm(changed).unwrap();
    source.order.clear();
    let noop = cycle.prepare(&mut source, &budget, &hints).await.unwrap();
    assert_eq!(noop.full_reads, 0);
    assert_eq!(noop.digest_reads, 3);
    assert!(noop.replacements.is_empty());
    assert_eq!(source.order, [2, 1, 0]);
    cycle.confirm(noop).unwrap();
}

#[tokio::test]
async fn rare_full_read_catches_digest_collision_and_keeps_noop_empty() {
    let mut source = source();
    source.collision = true;
    let mut cycle = Cycle::new(chunks(), 2).unwrap();
    let budget = ReadBudget::default();
    let hints = Hints::default();
    let baseline = cycle.prepare(&mut source, &budget, &hints).await.unwrap();
    cycle.confirm(baseline).unwrap();
    source.value = 1;
    let full = cycle.prepare(&mut source, &budget, &hints).await.unwrap();
    assert_eq!(full.full_reads, 3);
    assert_eq!(full.replacements.len(), 3);
    cycle.confirm(full).unwrap();
    for expected_reads in [0, 3] {
        let noop = cycle.prepare(&mut source, &budget, &hints).await.unwrap();
        assert_eq!(noop.full_reads, expected_reads);
        assert!(noop.replacements.is_empty());
        cycle.confirm(noop).unwrap();
    }
}

#[tokio::test]
async fn failed_abandoned_foreign_and_stale_proposals_do_not_advance_baseline() {
    let mut source = source();
    let mut cycle = Cycle::new(chunks(), 10).unwrap();
    let budget = ReadBudget::default();
    let hints = Hints::default();
    source.race = true;
    assert!(cycle.prepare(&mut source, &budget, &hints).await.is_err());
    source.race = false;
    let abandoned = cycle.prepare(&mut source, &budget, &hints).await.unwrap();
    assert_eq!(abandoned.replacements.len(), 3);
    drop(abandoned);
    let first = cycle.prepare(&mut source, &budget, &hints).await.unwrap();
    let stale = cycle.prepare(&mut source, &budget, &hints).await.unwrap();
    let other = Cycle::new(chunks(), 10).unwrap();
    let foreign = other.prepare(&mut source, &budget, &hints).await.unwrap();
    assert!(cycle.confirm(foreign).is_err());
    cycle.confirm(first).unwrap();
    assert!(cycle.confirm(stale).is_err());
    let noop = cycle.prepare(&mut source, &budget, &hints).await.unwrap();
    assert!(noop.replacements.is_empty());
}

#[test]
fn incomplete_overlapping_and_duplicate_plans_are_rejected() {
    assert!(Cycle::new(Vec::new(), 10).is_err());
    assert!(Cycle::new(chunks(), 0).is_err());
    assert!(Cycle::new(chunks()[1..].to_vec(), 10).is_err());
    let mut plan = chunks();
    plan[1].lo = None;
    assert!(Cycle::new(plan, 10).is_err());
    let mut plan = chunks();
    plan[1].chunk_id = plan[0].chunk_id;
    assert!(Cycle::new(plan, 10).is_err());
    let mut plan = chunks();
    plan[2].hi = Some(vec![KeyValue::Int(3)]);
    assert!(Cycle::new(plan, 10).is_err());
}
