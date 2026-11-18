use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::objectstore::{EntryKind, ListCursor, ListPage, ReadExpect};

fn hint(key: &str, op: ChangeOp, seq: Option<&str>, at: i64) -> ChangeHint {
    ChangeHint {
        raw_key: key.as_bytes().to_vec(),
        op,
        version_id: None,
        sequencer: seq.map(str::to_owned),
        observed_at_ms: at,
    }
}

#[test]
fn sequencers_compare_after_left_padding() {
    use std::cmp::Ordering::*;
    assert_eq!(sequencer_cmp("9", "10"), Less); // as text "9" > "10", padded "09" < "10"
    assert_eq!(sequencer_cmp("0A", "A"), Equal);
    assert_eq!(sequencer_cmp("00FF", "0100"), Less);
}

#[test]
fn a_repeat_and_an_older_event_do_not_ask_for_a_second_look() {
    let mut d = Deduper::default();
    assert_eq!(
        d.admit(&hint("a.md", ChangeOp::Put, Some("05"), 10)),
        Verdict::Fresh
    );
    assert_eq!(
        d.admit(&hint("a.md", ChangeOp::Put, Some("05"), 11)),
        Verdict::Duplicate
    );
    assert_eq!(
        d.admit(&hint("a.md", ChangeOp::Delete, Some("04"), 9)),
        Verdict::Stale
    );
    assert_eq!(
        d.admit(&hint("a.md", ChangeOp::Delete, Some("06"), 12)),
        Verdict::Fresh
    );
    // Another key has its own order.
    assert_eq!(
        d.admit(&hint("b.md", ChangeOp::Put, Some("01"), 13)),
        Verdict::Fresh
    );
    assert_eq!(d.cursor().0, "13");
}

#[test]
fn the_memory_is_bounded_and_forgets_the_oldest() {
    let mut d = Deduper::with_capacity(3);
    for i in 0..5 {
        assert_eq!(
            d.admit(&hint(&format!("k{i}"), ChangeOp::Put, None, i)),
            Verdict::Fresh
        );
    }
    assert_eq!(
        d.admit(&hint("k4", ChangeOp::Put, None, 4)),
        Verdict::Duplicate
    );
    assert_eq!(
        d.admit(&hint("k0", ChangeOp::Put, None, 0)),
        Verdict::Fresh,
        "evicted"
    );
}

#[test]
fn a_restored_watermark_never_moves_back() {
    let mut d = Deduper::default();
    d.restore(&ChangeCursor("500".into()));
    d.admit(&hint("a", ChangeOp::Put, None, 100));
    assert_eq!(d.watermark_ms(), 500);
    d.restore(&ChangeCursor("not a number".into()));
    assert_eq!(d.watermark_ms(), 500);
}

/// A store that is a map; `stat` answers from it. It changes while events are in flight.
struct MapStore(std::sync::Mutex<BTreeMap<Vec<u8>, u64>>);

impl MapStore {
    fn of<const N: usize>(items: [(Vec<u8>, u64); N]) -> Self {
        Self(std::sync::Mutex::new(items.into()))
    }
}

impl DocumentSource for MapStore {
    fn backend(&self) -> crate::objectstore::DocBackend {
        crate::objectstore::DocBackend::S3
    }
    fn capabilities(&self) -> BTreeSet<crate::objectstore::DocCapability> {
        BTreeSet::new()
    }
    async fn list(&self, _: Option<&ListCursor>, _: u32) -> Result<ListPage, DocSourceError> {
        Err(DocSourceError::Unavailable)
    }
    async fn stat(&self, raw_key: &[u8]) -> Result<Option<ObjectMeta>, DocSourceError> {
        Ok(self.0.lock().unwrap().get(raw_key).map(|size| ObjectMeta {
            raw_key: raw_key.to_vec(),
            path: None,
            kind: EntryKind::File,
            size: *size,
            etag: None,
            last_modified_ns: None,
            version_id: None,
            crc64: None,
            fs_ino: None,
            fs_ctime_ns: None,
        }))
    }
    async fn read(
        &self,
        _: &[u8],
        _: u64,
        _: u64,
        _: &ReadExpect,
    ) -> Result<Vec<u8>, DocSourceError> {
        Err(DocSourceError::Unavailable)
    }
}

/// Small deterministic generator (no dependency).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// The store changes step by step; every event is delivered late, in a different order than it was
/// made, some twice, and the consumer answers each with a `stat` of the live store. Once the last
/// event of a key is in, the consumer's view of that key equals the store.
#[tokio::test]
async fn out_of_order_and_duplicate_events_leave_the_store_state() {
    for seed in 0..300u64 {
        let mut rng = Lcg(seed + 1);
        let store = MapStore::of([]);
        let mut dedupe = Deduper::default();
        let mut view: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
        // (delivery tick, event)
        let mut in_flight: Vec<(u64, ChangeHint)> = Vec::new();
        let mut touched: BTreeSet<Vec<u8>> = BTreeSet::new();
        let (steps, max_lag) = (40u64, 12u64);
        for tick in 0..steps + max_lag + 1 {
            if tick < steps {
                let key = format!("d/{}.md", rng.next() % 4);
                let op = if rng.next().is_multiple_of(3) {
                    ChangeOp::Delete
                } else {
                    ChangeOp::Put
                };
                {
                    let mut m = store.0.lock().unwrap();
                    match op {
                        ChangeOp::Put => {
                            m.insert(key.as_bytes().to_vec(), tick + 1);
                        }
                        ChangeOp::Delete => {
                            m.remove(key.as_bytes());
                        }
                    }
                }
                touched.insert(key.as_bytes().to_vec());
                let h = hint(&key, op, Some(&format!("{:x}", 0x100 + tick)), tick as i64);
                in_flight.push((tick + rng.next() % (max_lag + 1), h.clone()));
                if rng.next().is_multiple_of(4) {
                    in_flight.push((tick + rng.next() % (max_lag + 1), h));
                }
            }
            let (now, later): (Vec<_>, Vec<_>) =
                in_flight.drain(..).partition(|(at, _)| *at == tick);
            in_flight = later;
            let batch: Vec<ChangeHint> = now.into_iter().map(|(_, h)| h).collect();
            let (observed, _) = apply(&store, &mut dedupe, &batch).await.unwrap();
            for o in observed {
                match o.meta {
                    Some(m) => {
                        view.insert(o.raw_key, m.size);
                    }
                    None => {
                        view.remove(&o.raw_key);
                    }
                }
            }
        }
        let truth = store.0.lock().unwrap().clone();
        for key in touched {
            assert_eq!(
                view.get(&key),
                truth.get(&key),
                "seed {seed} key {}",
                String::from_utf8_lossy(&key)
            );
        }
    }
}

#[tokio::test]
async fn one_look_per_key_per_batch_and_counts_add_up() {
    let store = MapStore::of([(b"a".to_vec(), 7)]);
    let mut d = Deduper::default();
    let batch = [
        hint("a", ChangeOp::Put, Some("1"), 1),
        hint("a", ChangeOp::Put, Some("2"), 2),
        hint("a", ChangeOp::Put, Some("2"), 2),
        hint("a", ChangeOp::Delete, Some("1"), 3),
    ];
    let (obs, n) = apply(&store, &mut d, &batch).await.unwrap();
    assert_eq!(obs.len(), 1);
    assert_eq!(obs[0].meta.as_ref().map(|m| m.size), Some(7));
    assert_eq!(
        n,
        Applied {
            fresh: 2,
            duplicates: 1,
            stale: 1
        }
    );
}
