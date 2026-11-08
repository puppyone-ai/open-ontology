//! Commit-order CDC driver. Source feedback is deliberately outside target
//! transactions: a crash between commit and feedback replays only feedback.
use crate::engine_load::Result;
use oo_connectors::engine::{ChangeBatch, ChangeSource, ReadBudget, Watermark};

/// The target atomically commits row changes, tombstones and the position.
pub trait Target {
    fn position(&mut self) -> impl std::future::Future<Output = Result<Watermark>>;
    /// May lag the source cursor when a table is quarantined. Releasing logs
    /// past this point would erase unapplied history.
    fn feedback_position(&mut self) -> impl std::future::Future<Output = Result<Watermark>> {
        self.position()
    }
    fn apply(
        &mut self,
        from: &Watermark,
        batch: &ChangeBatch,
    ) -> impl std::future::Future<Output = Result<()>>;
}

/// Engine-specific feedback must be idempotent. Implementations must not
/// acknowledge beyond this supplied, already durable position.
pub trait Feedback {
    fn acknowledge(&mut self, durable: &Watermark)
    -> impl std::future::Future<Output = Result<()>>;
}

/// Numeric positions only. GTID containment and lineage are engine-specific;
/// never substitute lexical ordering or silently change position families.
pub fn at_or_after(next: &Watermark, previous: &Watermark) -> Result<bool> {
    match (next, previous) {
        (Watermark::PgLsn { lsn: a }, Watermark::PgLsn { lsn: b }) => Ok(a >= b),
        (Watermark::OracleScn { scn: a }, Watermark::OracleScn { scn: b }) => Ok(a >= b),
        (
            Watermark::MysqlFilePos {
                file: a,
                position: ap,
            },
            Watermark::MysqlFilePos {
                file: b,
                position: bp,
            },
        ) => {
            let split = |file: &str| {
                let (prefix, number) = file.rsplit_once('.').ok_or("sync_file_position")?;
                let number = number.parse::<u64>().map_err(|_| "sync_file_position")?;
                Ok::<_, &'static str>((prefix.to_owned(), number))
            };
            let (a, an) = split(a)?;
            let (b, bn) = split(b)?;
            if a != b {
                return Err("sync_rebaseline_required_lineage");
            }
            Ok((an, ap) >= (bn, bp))
        }
        _ => Err("sync_position_requires_engine_comparator"),
    }
}

/// One bounded source batch. A no-batch poll does not touch the target.
/// `compare` must check monotonicity within an externally verified lineage.
pub async fn step<S: ChangeSource, T: Target, F: Feedback>(
    source: &mut S,
    target: &mut T,
    feedback: &mut F,
    budget: &ReadBudget,
    compare: impl Fn(&Watermark, &Watermark) -> Result<bool>,
) -> Result<bool> {
    let from = target.position().await?;
    let earliest = source
        .earliest_available()
        .await
        .map_err(|e| e.fact().unwrap_or("sync_retention"))?;
    if !compare(&from, &earliest)? {
        return Err("sync_rebaseline_required_retention");
    }
    let floor = target.feedback_position().await?;
    if !compare(&from, &floor)? || !compare(&floor, &earliest)? {
        return Err("sync_rebaseline_required_retention");
    }
    // Also repairs feedback lost after the previous successful target commit.
    feedback.acknowledge(&floor).await?;
    let Some(batch) = source
        .next_batch(&from, budget)
        .await
        .map_err(|e| e.fact().unwrap_or("sync_capture"))?
    else {
        return Ok(false);
    };
    if !compare(&batch.position, &from)? || compare(&from, &batch.position)? {
        return Err("sync_position_not_advanced");
    }
    if batch.changes.len() as u64 > budget.max_chunk_rows.min(100_000)
        || batch.images.len() != batch.changes.len()
    {
        return Err("sync_batch_shape");
    }
    target.apply(&from, &batch).await?;
    if target.position().await? != batch.position {
        return Err("sync_target_position");
    }
    let floor = target.feedback_position().await?;
    if !compare(&batch.position, &floor)? {
        return Err("sync_feedback_ahead");
    }
    feedback.acknowledge(&floor).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_connectors::SourceError;
    use std::sync::{Arc, Mutex};

    fn pos(lsn: u64) -> Watermark {
        Watermark::PgLsn { lsn }
    }
    struct Source {
        floor: u64,
        next: u64,
    }
    impl ChangeSource for Source {
        async fn earliest_available(&mut self) -> std::result::Result<Watermark, SourceError> {
            Ok(pos(self.floor))
        }
        async fn next_batch(
            &mut self,
            from: &Watermark,
            _: &ReadBudget,
        ) -> std::result::Result<Option<ChangeBatch>, SourceError> {
            Ok((from != &pos(self.next)).then(|| ChangeBatch {
                position: pos(self.next),
                changes: vec![],
                images: vec![],
            }))
        }
    }
    struct Store {
        position: u64,
        fail: bool,
        events: Arc<Mutex<Vec<u64>>>,
        feedback_floor: Option<u64>,
    }
    impl Target for Store {
        async fn position(&mut self) -> Result<Watermark> {
            Ok(pos(self.position))
        }
        async fn feedback_position(&mut self) -> Result<Watermark> {
            Ok(pos(self.feedback_floor.unwrap_or(self.position)))
        }
        async fn apply(&mut self, _: &Watermark, batch: &ChangeBatch) -> Result<()> {
            if self.fail {
                return Err("target_failed");
            }
            let Watermark::PgLsn { lsn } = batch.position else {
                unreachable!()
            };
            self.position = lsn;
            self.events.lock().unwrap().push(lsn);
            Ok(())
        }
    }
    struct Ack {
        events: Arc<Mutex<Vec<u64>>>,
        fail: bool,
    }
    impl Feedback for Ack {
        async fn acknowledge(&mut self, position: &Watermark) -> Result<()> {
            let Watermark::PgLsn { lsn } = position else {
                unreachable!()
            };
            if self.fail && *lsn == 20 {
                return Err("feedback_failed");
            }
            self.events.lock().unwrap().push(*lsn + 100);
            Ok(())
        }
    }
    #[tokio::test]
    async fn commit_precedes_ack_and_resume_only_repeats_feedback() {
        let events = Arc::new(Mutex::new(vec![]));
        let mut source = Source {
            floor: 10,
            next: 20,
        };
        let mut store = Store {
            position: 10,
            fail: false,
            events: events.clone(),
            feedback_floor: None,
        };
        let mut ack = Ack {
            events: events.clone(),
            fail: true,
        };
        let budget = ReadBudget::default();
        assert_eq!(
            step(&mut source, &mut store, &mut ack, &budget, at_or_after).await,
            Err("feedback_failed")
        );
        assert_eq!(*events.lock().unwrap(), vec![110, 20]);
        ack.fail = false;
        assert!(
            !step(&mut source, &mut store, &mut ack, &budget, at_or_after)
                .await
                .unwrap()
        );
        assert_eq!(*events.lock().unwrap(), vec![110, 20, 120]);
    }
    #[tokio::test]
    async fn failures_never_ack_uncommitted_positions() {
        let events = Arc::new(Mutex::new(vec![]));
        let mut source = Source {
            floor: 10,
            next: 20,
        };
        let mut store = Store {
            position: 10,
            fail: true,
            events: events.clone(),
            feedback_floor: None,
        };
        let mut ack = Ack {
            events: events.clone(),
            fail: false,
        };
        let budget = ReadBudget::default();
        assert_eq!(
            step(&mut source, &mut store, &mut ack, &budget, at_or_after).await,
            Err("target_failed")
        );
        assert_eq!(store.position, 10);
        assert_eq!(*events.lock().unwrap(), vec![110]);
        source.floor = 11;
        assert_eq!(
            step(&mut source, &mut store, &mut ack, &budget, at_or_after).await,
            Err("sync_rebaseline_required_retention")
        );
        assert_eq!(*events.lock().unwrap(), vec![110]);
        source.floor = 10;
        source.next = 9;
        assert_eq!(
            step(&mut source, &mut store, &mut ack, &budget, at_or_after).await,
            Err("sync_position_not_advanced")
        );
    }
    #[tokio::test]
    async fn quarantine_retains_history_despite_healthy_cursor_progress() {
        let events = Arc::new(Mutex::new(vec![]));
        let mut source = Source {
            floor: 10,
            next: 30,
        };
        let mut store = Store {
            position: 20,
            fail: false,
            events: events.clone(),
            feedback_floor: Some(10),
        };
        let mut ack = Ack {
            events: events.clone(),
            fail: false,
        };
        let budget = ReadBudget::default();
        assert!(
            step(&mut source, &mut store, &mut ack, &budget, at_or_after)
                .await
                .unwrap()
        );
        assert_eq!(store.position, 30);
        assert_eq!(*events.lock().unwrap(), vec![110, 30, 110]);
        source.floor = 11;
        assert_eq!(
            step(&mut source, &mut store, &mut ack, &budget, at_or_after).await,
            Err("sync_rebaseline_required_retention")
        );
        assert_eq!(*events.lock().unwrap(), vec![110, 30, 110]);
        store.feedback_floor = Some(31);
        assert_eq!(
            step(&mut source, &mut store, &mut ack, &budget, at_or_after).await,
            Err("sync_rebaseline_required_retention")
        );
        assert_eq!(*events.lock().unwrap(), vec![110, 30, 110]);
    }

    #[test]
    fn file_numbers_are_numeric_and_prefix_changes_require_rebaseline() {
        let p = |file: &str| Watermark::MysqlFilePos {
            file: file.into(),
            position: 4,
        };
        assert!(at_or_after(&p("bin.10"), &p("bin.9")).unwrap());
        assert!(at_or_after(&p("other.10"), &p("bin.9")).is_err());
        assert!(at_or_after(&pos(1), &Watermark::OracleScn { scn: 1 }).is_err());
    }
}
