//! Change hints from bucket event notifications (10 §4.3.6, §11.3, B-S11-02).
//!
//! An event only says "look at this key". It never carries state: the hint is
//! answered with one `stat` of the key and the answer is what counts (D10), the
//! periodic reconciliation runs as before. So the two properties that matter are
//!
//! * duplicates and out-of-order delivery cannot change the result: the
//!   [`Deduper`] drops a repeat of `(raw_key, version_id, op, sequencer)` and an
//!   event whose sequencer is older than one already handled for the same key
//!   (a newer event already caused a `stat` that saw at least that state);
//! * a missing event cannot either: reconciliation finds it.
//!
//! Sequencer order is the AWS rule: left pad both with `0` to the same length and
//! compare as strings; it orders events of one key only.
//!
//! Event formats of the clouds live in [`parse`]; [`local`] is the local event
//! source the tests and the L7 script use (a spool directory of JSON files).

use std::collections::{BTreeMap, VecDeque};

use crate::objectstore::{
    ChangeCursor, ChangeHint, ChangeOp, DocSourceError, DocumentSource, ObjectMeta,
};

pub mod local;
pub mod parse;

/// Entries the dedupe memory keeps (10 §4.3.6).
pub const DEDUPE_CAPACITY: usize = 100_000;

/// AWS sequencer comparison: pad on the left with `0` to equal length, then
/// compare as text.
pub fn sequencer_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let n = a.len().max(b.len());
    let pad = |s: &str| format!("{s:0>n$}");
    pad(a).cmp(&pad(b))
}

type HintKey = (Vec<u8>, Option<String>, u8, Option<String>);

fn key_of(h: &ChangeHint) -> HintKey {
    (
        h.raw_key.clone(),
        h.version_id.clone(),
        u8::from(h.op == ChangeOp::Delete),
        // No sequencer (R2, OSS): the event time tells two real events of one key apart.
        h.sequencer
            .clone()
            .or_else(|| Some(format!("@{}", h.observed_at_ms))),
    )
}

/// What the deduper decided about one hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Look at the key.
    Fresh,
    /// The same event was seen before.
    Duplicate,
    /// An event of the same key with a later sequencer was already handled.
    Stale,
}

/// Bounded memory of the events seen: an LRU of full keys plus, per object key,
/// the highest sequencer handled. Also the event watermark the Drive stores
/// (`drive_sync`): the largest `observed_at_ms` handled, see [`Deduper::cursor`].
#[derive(Debug)]
pub struct Deduper {
    capacity: usize,
    order: VecDeque<HintKey>,
    seen: BTreeMap<HintKey, ()>,
    high: BTreeMap<Vec<u8>, String>,
    watermark_ms: i64,
}

impl Default for Deduper {
    fn default() -> Self {
        Self::with_capacity(DEDUPE_CAPACITY)
    }
}

impl Deduper {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::new(),
            seen: BTreeMap::new(),
            high: BTreeMap::new(),
            watermark_ms: 0,
        }
    }

    /// Judge a hint and remember it when it is fresh.
    pub fn admit(&mut self, hint: &ChangeHint) -> Verdict {
        let k = key_of(hint);
        if self.seen.contains_key(&k) {
            return Verdict::Duplicate;
        }
        if let Some(seq) = &hint.sequencer {
            if let Some(high) = self.high.get(&hint.raw_key)
                && sequencer_cmp(seq, high).is_lt()
            {
                return Verdict::Stale;
            }
            self.high.insert(hint.raw_key.clone(), seq.clone());
        }
        self.watermark_ms = self.watermark_ms.max(hint.observed_at_ms);
        self.order.push_back(k.clone());
        self.seen.insert(k, ());
        while self.order.len() > self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
                // `high` is bounded by the live keys; it is trimmed with the LRU.
                if self.high.len() > self.capacity {
                    self.high.remove(&old.0);
                }
            }
        }
        Verdict::Fresh
    }

    /// The event watermark, to be stored with the Drive's sync row.
    pub fn cursor(&self) -> ChangeCursor {
        ChangeCursor(self.watermark_ms.to_string())
    }

    /// Resume from a stored watermark: events older than it are not new.
    pub fn restore(&mut self, cursor: &ChangeCursor) {
        if let Ok(ms) = cursor.0.parse::<i64>() {
            self.watermark_ms = self.watermark_ms.max(ms);
        }
    }

    pub fn watermark_ms(&self) -> i64 {
        self.watermark_ms
    }
}

/// What one admitted hint turned into after the `stat` of its key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observed {
    pub raw_key: Vec<u8>,
    /// `None`: the object is gone.
    pub meta: Option<ObjectMeta>,
}

/// Counters of one [`apply`] call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub fresh: u32,
    pub duplicates: u32,
    pub stale: u32,
}

/// Run hints through `dedupe` and answer each fresh one with a `stat` (once per
/// key per call: several fresh events of one key need one look). The result is
/// the state of the store, never of the event.
pub async fn apply<S: DocumentSource>(
    source: &S,
    dedupe: &mut Deduper,
    hints: &[ChangeHint],
) -> Result<(Vec<Observed>, Applied), DocSourceError> {
    let mut counts = Applied::default();
    let mut keys: Vec<Vec<u8>> = Vec::new();
    for h in hints {
        match dedupe.admit(h) {
            Verdict::Fresh => {
                counts.fresh += 1;
                if !keys.contains(&h.raw_key) {
                    keys.push(h.raw_key.clone());
                }
            }
            Verdict::Duplicate => counts.duplicates += 1,
            Verdict::Stale => counts.stale += 1,
        }
    }
    let mut out = Vec::with_capacity(keys.len());
    for raw_key in keys {
        let meta = source.stat(&raw_key).await?;
        out.push(Observed { raw_key, meta });
    }
    Ok((out, counts))
}

#[cfg(test)]
mod tests;
