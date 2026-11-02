//! Reference model of the v2 single-write C1 baseline (DESIGN §3.1, §3.8;
//! plan §0.1 Q3 and Q6). Pure and in memory. It is a verification model for
//! tests and fixtures, not the streaming seal path.
//!
//! - [`Model`] applies fill chunks and CDC batches with the v2 rules. A fill row
//!   lands at `_oo_pos = 0`, unless a tombstone blocks it or the key exists. An
//!   upsert wins only if its position is newer and no newer tombstone exists. A
//!   delete removes older rows and records the maximum tombstone position.
//! - [`SourceHistory`] is the ground truth. It keeps the source state after each
//!   transaction and derives each transaction's net CDC events, one per key.
//! - [`Scenario`] generates a seeded random history, chunk reads, batches and an
//!   interleaving. The PRNG is local, so no dependency is added.
//! - [`expected_composed`], [`expected_digest`] and [`compare_readback`] serve
//!   T10 and T13. They use the `oo_pipeline::c1_parts` canonical encoding and
//!   composed digest, so both sides hash the same bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

use oo_core::canonical::LogicalType;
use oo_pipeline::c1_parts::{
    C1PartsError, Cell, StateDigest, bucket_key, canonical_row, composed_digest,
};

use crate::error::PreviewError;

const MODEL: &str = "c1_model";
const TEXT_WORDS: &[&str] = &["", "a", "bb", "ünï", "x y"];

/// One row in column order.
pub type Row = Vec<Cell>;
/// Result type of this module. The fact is a closed token, never a value.
pub type ModelResult<T> = Result<T, PreviewError>;

fn fail<T>(fact: &'static str) -> ModelResult<T> {
    Err(PreviewError::new(MODEL, fact))
}

fn lift(err: C1PartsError) -> PreviewError {
    PreviewError::new(MODEL, err.reason)
}

/// Shape of one table: column types, plus the index of its integer primary key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableShape {
    name: String,
    types: Vec<LogicalType>,
    key: usize,
}

impl TableShape {
    /// Table with an integer primary key at column `key`.
    pub fn new(name: &str, types: Vec<LogicalType>, key: usize) -> ModelResult<Self> {
        if name.is_empty() || name.contains('\n') {
            return fail("table_name");
        }
        if types.get(key) != Some(&LogicalType::Int) {
            return fail("key_type");
        }
        Ok(Self {
            name: name.to_owned(),
            types,
            key,
        })
    }

    /// Table name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Column types in order.
    pub fn types(&self) -> &[LogicalType] {
        &self.types
    }

    /// Index of the primary key column.
    pub fn key(&self) -> usize {
        self.key
    }

    /// Integer key and canonical bytes of one row. Fails on a width mismatch, a
    /// NULL or non-integer key, or a cell that does not fit its column type.
    fn encode(&self, row: &[Cell]) -> ModelResult<(i128, Vec<u8>)> {
        if row.len() != self.types.len() {
            return fail("row_width");
        }
        let key = bucket_key(&row[self.key]).map_err(lift)?;
        let bytes = canonical_row(&self.types, row).map_err(lift)?;
        Ok((key, bytes))
    }
}

/// Settings shared by the model, the ground truth and the digest helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelOptions {
    /// Bucket exponent of the state digest. It must match the loader and the oracle.
    pub bucket_bits: u8,
    /// `false` drops tombstones. Only the negative control uses this.
    pub tombstones: bool,
}

impl ModelOptions {
    /// Options with tombstones enabled, as in v2.
    pub fn new(bucket_bits: u8) -> Self {
        Self {
            bucket_bits,
            tombstones: true,
        }
    }
}

/// Operation of one CDC event after the net-per-key collapse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CdcOp {
    /// Post-image of the key after the transaction.
    Upsert(Row),
    /// The key is absent after a transaction that removed it.
    Delete,
}

/// One net CDC event. `pos` is the position of its transaction, which is 1-based.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CdcEvent {
    /// Table index in the schema.
    pub table: usize,
    /// Primary key of the affected row.
    pub key: i128,
    /// Position of the transaction.
    pub pos: u64,
    /// Operation.
    pub op: CdcOp,
}

/// One snapshot chunk. `key_hi = None` marks the terminal chunk, which has no rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FillChunk {
    /// Table index in the schema.
    pub table: usize,
    /// 1-based chunk number within the table.
    pub chunk_no: u32,
    /// Source prefix at which the chunk was read. Informational only.
    pub read_at: u64,
    /// Exclusive lower bound. `None` for the first chunk.
    pub key_lo: Option<i128>,
    /// Inclusive upper bound, the last key returned. `None` for the terminal chunk.
    pub key_hi: Option<i128>,
    /// Rows in key order.
    pub rows: Vec<Row>,
}

/// Counts reported for one fill chunk, as in the chunk acknowledgement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FillCounts {
    /// Rows inserted at `_oo_pos = 0`.
    pub inserted: u64,
    /// Rows blocked by a tombstone.
    pub skipped_tomb: u64,
    /// Rows that already existed.
    pub skipped_existing: u64,
}

/// Counters of the model over its lifetime. Used to show that the randomized
/// runs reach each case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelStats {
    /// Fill rows inserted.
    pub fill_inserted: u64,
    /// Fill rows blocked by a tombstone.
    pub fill_skipped_tomb: u64,
    /// Fill rows that already existed.
    pub fill_skipped_existing: u64,
    /// Upserts that inserted a new row.
    pub upsert_inserted: u64,
    /// Upserts that replaced an older row.
    pub upsert_overwrote: u64,
    /// Upserts blocked by a newer tombstone.
    pub upsert_blocked_tomb: u64,
    /// Upserts that found a row at the same or a newer position.
    pub upsert_stale: u64,
    /// Rows removed by deletes.
    pub delete_removed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stored {
    pos: u64,
    cells: Row,
}

#[derive(Clone, Debug, Default)]
struct TableState {
    rows: BTreeMap<i128, Stored>,
    tombs: BTreeMap<i128, u64>,
}

/// Target-side state of the v2 algorithm.
#[derive(Clone, Debug)]
pub struct Model {
    schema: Vec<TableShape>,
    tables: Vec<TableState>,
    options: ModelOptions,
    stats: ModelStats,
}

impl Model {
    /// Empty model for `schema`. Table names must be distinct.
    pub fn new(schema: Vec<TableShape>, options: ModelOptions) -> ModelResult<Self> {
        StateDigest::new(options.bucket_bits).map_err(lift)?;
        let mut names = BTreeSet::new();
        for shape in &schema {
            if !names.insert(shape.name()) {
                return fail("table_name");
            }
        }
        let tables = vec![TableState::default(); schema.len()];
        Ok(Self {
            schema,
            tables,
            options,
            stats: ModelStats::default(),
        })
    }

    /// Lifetime counters.
    pub fn stats(&self) -> ModelStats {
        self.stats
    }

    fn shape(&self, table: usize) -> ModelResult<&TableShape> {
        self.schema
            .get(table)
            .ok_or(PreviewError::new(MODEL, "table_index"))
    }

    /// Apply one fill chunk. Chunks of one table must arrive in order. The chunk
    /// is validated before any row is written.
    pub fn apply_fill(&mut self, chunk: &FillChunk) -> ModelResult<FillCounts> {
        let shape = self.shape(chunk.table)?.clone();
        let encoded: Vec<(i128, Vec<u8>)> = chunk
            .rows
            .iter()
            .map(|row| shape.encode(row))
            .collect::<ModelResult<_>>()?;
        let keys: Vec<i128> = encoded.iter().map(|(key, _)| *key).collect();
        check_chunk(chunk, &keys)?;
        let tombstones = self.options.tombstones;
        let Self { tables, stats, .. } = self;
        let state = tables
            .get_mut(chunk.table)
            .ok_or(PreviewError::new(MODEL, "table_index"))?;
        let mut counts = FillCounts::default();
        for (row, (key, _)) in chunk.rows.iter().zip(&encoded) {
            if tombstones && state.tombs.contains_key(key) {
                counts.skipped_tomb += 1;
            } else if state.rows.contains_key(key) {
                counts.skipped_existing += 1;
            } else {
                state.rows.insert(
                    *key,
                    Stored {
                        pos: 0,
                        cells: row.clone(),
                    },
                );
                counts.inserted += 1;
            }
        }
        stats.fill_inserted += counts.inserted;
        stats.fill_skipped_tomb += counts.skipped_tomb;
        stats.fill_skipped_existing += counts.skipped_existing;
        Ok(counts)
    }

    /// Apply one net CDC event.
    pub fn apply_event(&mut self, event: &CdcEvent) -> ModelResult<()> {
        if event.pos == 0 {
            return fail("cdc_pos");
        }
        let shape = self.shape(event.table)?.clone();
        let tombstones = self.options.tombstones;
        let Self { tables, stats, .. } = self;
        let state = tables
            .get_mut(event.table)
            .ok_or(PreviewError::new(MODEL, "table_index"))?;
        let key = event.key;
        match &event.op {
            CdcOp::Upsert(row) => {
                let (row_key, _) = shape.encode(row)?;
                if row_key != key {
                    return fail("event_key");
                }
                if tombstones && state.tombs.get(&key).is_some_and(|q| *q > event.pos) {
                    stats.upsert_blocked_tomb += 1;
                    return Ok(());
                }
                let existing = state.rows.get(&key).map(|stored| stored.pos);
                match existing {
                    Some(pos) if pos >= event.pos => stats.upsert_stale += 1,
                    Some(_) => {
                        stats.upsert_overwrote += 1;
                        state.rows.insert(key, stored(event.pos, row));
                    }
                    None => {
                        stats.upsert_inserted += 1;
                        state.rows.insert(key, stored(event.pos, row));
                    }
                }
            }
            CdcOp::Delete => {
                let older = state.rows.get(&key).is_some_and(|s| s.pos < event.pos);
                if older {
                    state.rows.remove(&key);
                    stats.delete_removed += 1;
                }
                if tombstones {
                    let tomb = state.tombs.entry(key).or_insert(0);
                    *tomb = (*tomb).max(event.pos);
                }
            }
        }
        Ok(())
    }

    /// Apply one CDC batch that covers positions `first_pos..=last_pos`. As in
    /// the v2 SQL, only the last event of each key in the batch is applied.
    pub fn apply_batch(
        &mut self,
        first_pos: u64,
        last_pos: u64,
        events: &[CdcEvent],
    ) -> ModelResult<()> {
        if first_pos == 0 || last_pos < first_pos {
            return fail("batch_range");
        }
        let mut seen = BTreeSet::new();
        let mut last_per_key: BTreeMap<(usize, i128), &CdcEvent> = BTreeMap::new();
        for event in events {
            if event.pos < first_pos || event.pos > last_pos {
                return fail("batch_events");
            }
            if !seen.insert((event.table, event.key, event.pos)) {
                return fail("batch_events");
            }
            let slot = last_per_key
                .entry((event.table, event.key))
                .or_insert(event);
            if event.pos > slot.pos {
                *slot = event;
            }
        }
        for event in last_per_key.values() {
            self.apply_event(event)?;
        }
        Ok(())
    }

    /// Rows of one table in key order. Empty when `table` is out of range.
    pub fn rows(&self, table: usize) -> Vec<Row> {
        self.tables
            .get(table)
            .map(|state| state.rows.values().map(|s| s.cells.clone()).collect())
            .unwrap_or_default()
    }

    /// Expected composed digest of the current state.
    pub fn expected(&self) -> ModelResult<ExpectedState> {
        let rows: Vec<Vec<Row>> = (0..self.schema.len()).map(|t| self.rows(t)).collect();
        let pairs = pair_up(&self.schema, &rows);
        expected_composed(self.options.bucket_bits, &pairs)
    }
}

fn stored(pos: u64, row: &[Cell]) -> Stored {
    Stored {
        pos,
        cells: row.to_vec(),
    }
}

fn check_chunk(chunk: &FillChunk, keys: &[i128]) -> ModelResult<()> {
    if chunk.chunk_no == 0 {
        return fail("chunk_no");
    }
    let Some(hi) = chunk.key_hi else {
        return if keys.is_empty() {
            Ok(())
        } else {
            fail("chunk_terminal")
        };
    };
    if keys.last() != Some(&hi) {
        return fail("chunk_keys");
    }
    let mut prev = chunk.key_lo;
    for &key in keys {
        if prev.is_some_and(|p| key <= p) {
            return fail("chunk_keys");
        }
        prev = Some(key);
    }
    Ok(())
}

/// Expected composed digest of a set of tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedState {
    /// Row count per table, in input order.
    pub tables: Vec<(String, u64)>,
    /// Total row count.
    pub rows: u64,
    /// Composed digest, lowercase hex.
    pub composed_digest: String,
}

/// Per-table digest state, as the loader and the oracle compute it.
pub fn table_digest(bucket_bits: u8, shape: &TableShape, rows: &[Row]) -> ModelResult<StateDigest> {
    let mut digest = StateDigest::new(bucket_bits).map_err(lift)?;
    for row in rows {
        let (key, bytes) = shape.encode(row)?;
        digest.add_row(key, &bytes).map_err(lift)?;
    }
    Ok(digest)
}

/// Expected composed digest of several tables. Each table comes with its rows,
/// in any order. The digest does not depend on that order.
pub fn expected_composed(
    bucket_bits: u8,
    tables: &[(&TableShape, &[Row])],
) -> ModelResult<ExpectedState> {
    let mut digests: Vec<(&str, StateDigest)> = Vec::with_capacity(tables.len());
    let mut counts = Vec::with_capacity(tables.len());
    for (shape, rows) in tables {
        digests.push((shape.name(), table_digest(bucket_bits, shape, rows)?));
        counts.push((shape.name().to_owned(), rows.len() as u64));
    }
    let refs: Vec<(&str, &StateDigest)> = digests.iter().map(|(name, d)| (*name, d)).collect();
    let composed = composed_digest(&refs).map_err(lift)?;
    let rows = counts.iter().map(|(_, n)| *n).sum();
    Ok(ExpectedState {
        tables: counts,
        rows,
        composed_digest: composed,
    })
}

/// Expected composed digest of one table.
pub fn expected_digest(
    bucket_bits: u8,
    shape: &TableShape,
    rows: &[Row],
) -> ModelResult<ExpectedState> {
    expected_composed(bucket_bits, &[(shape, rows)])
}

/// Comparison of a target readback with the expected rows of one table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadbackDiff {
    /// Expected row count.
    pub expected_rows: u64,
    /// Readback row count.
    pub readback_rows: u64,
    /// Expected keys absent from the readback.
    pub missing: u64,
    /// Readback keys that are not expected.
    pub extra: u64,
    /// Keys present on both sides whose canonical row differs.
    pub changed: u64,
    /// Readback rows that repeat a key.
    pub duplicate: u64,
    /// Smallest key that differs. This is key data, not a message.
    pub first_bad_key: Option<i128>,
    /// Whether the table digests are equal.
    pub digest_equal: bool,
}

impl ReadbackDiff {
    /// True when every row matches and the table digests are equal.
    pub fn is_match(&self) -> bool {
        self.missing == 0
            && self.extra == 0
            && self.changed == 0
            && self.duplicate == 0
            && self.digest_equal
    }
}

/// Compare a readback of one table with its expected rows. Readback cells must
/// use the target's logical types, for example from `pg_text_cell`.
pub fn compare_readback(
    bucket_bits: u8,
    shape: &TableShape,
    expected: &[Row],
    readback: &[Row],
) -> ModelResult<ReadbackDiff> {
    let mut want: BTreeMap<i128, Vec<u8>> = BTreeMap::new();
    for row in expected {
        let (key, bytes) = shape.encode(row)?;
        if want.insert(key, bytes).is_some() {
            return fail("duplicate_key");
        }
    }
    let mut got: BTreeMap<i128, Vec<u8>> = BTreeMap::new();
    let mut diff = ReadbackDiff {
        expected_rows: want.len() as u64,
        readback_rows: readback.len() as u64,
        ..ReadbackDiff::default()
    };
    let mut bad: BTreeSet<i128> = BTreeSet::new();
    for row in readback {
        let (key, bytes) = shape.encode(row)?;
        if got.contains_key(&key) {
            diff.duplicate += 1;
            bad.insert(key);
        } else {
            got.insert(key, bytes);
        }
    }
    for (key, bytes) in &want {
        match got.get(key) {
            None => {
                diff.missing += 1;
                bad.insert(*key);
            }
            Some(other) if other != bytes => {
                diff.changed += 1;
                bad.insert(*key);
            }
            Some(_) => {}
        }
    }
    for key in got.keys() {
        if !want.contains_key(key) {
            diff.extra += 1;
            bad.insert(*key);
        }
    }
    diff.first_bad_key = bad.first().copied();
    diff.digest_equal =
        table_digest(bucket_bits, shape, expected)? == table_digest(bucket_bits, shape, readback)?;
    Ok(diff)
}

fn pair_up<'a>(schema: &'a [TableShape], rows: &'a [Vec<Row>]) -> Vec<(&'a TableShape, &'a [Row])> {
    schema
        .iter()
        .zip(rows)
        .map(|(shape, rows)| (shape, rows.as_slice()))
        .collect()
}

/// Source operation of one transaction, before the net collapse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceOp {
    /// Insert a new key.
    Insert {
        /// Table index.
        table: usize,
        /// New row.
        row: Row,
    },
    /// Replace the row of an existing key. The key is taken from `row`.
    Update {
        /// Table index.
        table: usize,
        /// New row with the same key.
        row: Row,
    },
    /// Delete an existing key.
    Delete {
        /// Table index.
        table: usize,
        /// Key to remove.
        key: i128,
    },
    /// Change the primary key. The key in `row` must be new.
    PkChange {
        /// Table index.
        table: usize,
        /// Existing key that disappears.
        old_key: i128,
        /// Row under its new key.
        row: Row,
    },
}

/// Source table state at one prefix of the history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceState {
    tables: Vec<BTreeMap<i128, Row>>,
}

impl SourceState {
    /// Empty state with `table_count` tables.
    pub fn new(table_count: usize) -> Self {
        Self {
            tables: vec![BTreeMap::new(); table_count],
        }
    }

    /// Whether `key` exists in `table`.
    pub fn contains(&self, table: usize, key: i128) -> bool {
        self.tables
            .get(table)
            .is_some_and(|map| map.contains_key(&key))
    }

    /// Rows of one table in key order.
    pub fn rows(&self, table: usize) -> Vec<Row> {
        self.tables
            .get(table)
            .map(|map| map.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Keyset read: the first `limit` rows with key greater than `after`.
    fn range_after(&self, table: usize, after: Option<i128>, limit: usize) -> Vec<(i128, Row)> {
        let Some(map) = self.tables.get(table) else {
            return Vec::new();
        };
        let lower = match after {
            Some(key) => Bound::Excluded(key),
            None => Bound::Unbounded,
        };
        map.range((lower, Bound::Unbounded))
            .take(limit)
            .map(|(key, row)| (*key, row.clone()))
            .collect()
    }

    /// Apply one operation and return the keys it touched.
    fn apply(&mut self, schema: &[TableShape], op: &SourceOp) -> ModelResult<Vec<(usize, i128)>> {
        match op {
            SourceOp::Insert { table, row } => {
                let (key, _) = shape_of(schema, *table)?.encode(row)?;
                if self.contains(*table, key) {
                    return fail("source_insert");
                }
                self.put(*table, key, row.clone())?;
                Ok(vec![(*table, key)])
            }
            SourceOp::Update { table, row } => {
                let (key, _) = shape_of(schema, *table)?.encode(row)?;
                if !self.contains(*table, key) {
                    return fail("source_update");
                }
                self.put(*table, key, row.clone())?;
                Ok(vec![(*table, key)])
            }
            SourceOp::Delete { table, key } => {
                if !self.contains(*table, *key) {
                    return fail("source_delete");
                }
                self.remove(*table, *key)?;
                Ok(vec![(*table, *key)])
            }
            SourceOp::PkChange {
                table,
                old_key,
                row,
            } => {
                let (new_key, _) = shape_of(schema, *table)?.encode(row)?;
                if new_key == *old_key
                    || !self.contains(*table, *old_key)
                    || self.contains(*table, new_key)
                {
                    return fail("source_pk_change");
                }
                self.remove(*table, *old_key)?;
                self.put(*table, new_key, row.clone())?;
                Ok(vec![(*table, *old_key), (*table, new_key)])
            }
        }
    }

    fn put(&mut self, table: usize, key: i128, row: Row) -> ModelResult<()> {
        let map = self
            .tables
            .get_mut(table)
            .ok_or(PreviewError::new(MODEL, "table_index"))?;
        map.insert(key, row);
        Ok(())
    }

    fn remove(&mut self, table: usize, key: i128) -> ModelResult<()> {
        let map = self
            .tables
            .get_mut(table)
            .ok_or(PreviewError::new(MODEL, "table_index"))?;
        map.remove(&key);
        Ok(())
    }

    fn get(&self, table: usize, key: i128) -> Option<&Row> {
        self.tables.get(table).and_then(|map| map.get(&key))
    }
}

fn shape_of(schema: &[TableShape], table: usize) -> ModelResult<&TableShape> {
    schema
        .get(table)
        .ok_or(PreviewError::new(MODEL, "table_index"))
}

/// Switches on the source-side event rule. The correct rule is the default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventRules {
    /// If false, a primary key change emits only the new-key upsert. That is the
    /// v1 bug (DESIGN §2.5 P1). Only the negative control uses this.
    pub pk_change_deletes_old_key: bool,
}

impl EventRules {
    /// The v2 rule: a primary key change deletes the old key and upserts the new one.
    pub fn correct() -> Self {
        Self {
            pk_change_deletes_old_key: true,
        }
    }
}

/// The ground truth: source states S(0..=N) and the net CDC events of each
/// transaction. Transaction `g` (1-based) has position `g`. S(g) is the source
/// after `g` transactions.
#[derive(Clone, Debug)]
pub struct SourceHistory {
    schema: Vec<TableShape>,
    txns: Vec<Vec<SourceOp>>,
    states: Vec<SourceState>,
    events: Vec<Vec<CdcEvent>>,
}

impl SourceHistory {
    /// Apply the transactions in order and derive their net events.
    pub fn new(
        schema: Vec<TableShape>,
        initial: SourceState,
        txns: &[Vec<SourceOp>],
        rules: EventRules,
    ) -> ModelResult<Self> {
        let mut states = vec![initial];
        let mut events = Vec::with_capacity(txns.len());
        for (index, ops) in txns.iter().enumerate() {
            let pos = index as u64 + 1;
            let Some(before) = states.last().cloned() else {
                return fail("history");
            };
            let mut after = before.clone();
            let mut touched = BTreeSet::new();
            let mut pk_old = BTreeSet::new();
            for op in ops {
                touched.extend(after.apply(&schema, op)?);
                if let SourceOp::PkChange { table, old_key, .. } = op {
                    pk_old.insert((*table, *old_key));
                }
            }
            let mut txn_events = Vec::new();
            for (table, key) in touched {
                let skip_delete =
                    !rules.pk_change_deletes_old_key && pk_old.contains(&(table, key));
                match after.get(table, key) {
                    Some(row) => txn_events.push(CdcEvent {
                        table,
                        key,
                        pos,
                        op: CdcOp::Upsert(row.clone()),
                    }),
                    None if before.contains(table, key) && !skip_delete => {
                        txn_events.push(CdcEvent {
                            table,
                            key,
                            pos,
                            op: CdcOp::Delete,
                        })
                    }
                    None => {}
                }
            }
            events.push(txn_events);
            states.push(after);
        }
        Ok(Self {
            schema,
            txns: txns.to_vec(),
            states,
            events,
        })
    }

    /// Table shapes.
    pub fn schema(&self) -> &[TableShape] {
        &self.schema
    }

    /// Number of transactions N. Positions run from 1 to N.
    pub fn txn_count(&self) -> usize {
        self.events.len()
    }

    /// Operations of each transaction.
    pub fn txns(&self) -> &[Vec<SourceOp>] {
        &self.txns
    }

    /// Source state S(prefix).
    pub fn state_at(&self, prefix: usize) -> ModelResult<&SourceState> {
        self.states
            .get(prefix)
            .ok_or(PreviewError::new(MODEL, "prefix"))
    }

    /// Net events of the transaction at `pos`.
    pub fn events_of(&self, pos: u64) -> ModelResult<&[CdcEvent]> {
        let index = usize::try_from(pos.checked_sub(1).ok_or(PreviewError::new(MODEL, "pos"))?)
            .map_err(|_| PreviewError::new(MODEL, "pos"))?;
        self.events
            .get(index)
            .map(Vec::as_slice)
            .ok_or(PreviewError::new(MODEL, "pos"))
    }

    /// Ground truth S(L_end) as its composed digest.
    pub fn truth(&self, bucket_bits: u8, l_end: usize) -> ModelResult<ExpectedState> {
        let state = self.state_at(l_end)?;
        let rows: Vec<Vec<Row>> = (0..self.schema.len()).map(|t| state.rows(t)).collect();
        expected_composed(bucket_bits, &pair_up(&self.schema, &rows))
    }
}

/// Read one chunk from the source state at prefix `read_at`, with keyset
/// semantics: `pk > after ORDER BY pk LIMIT limit`. The chunk's `key_hi` is its
/// last key. A chunk with no rows is terminal.
pub fn read_chunk(
    history: &SourceHistory,
    table: usize,
    read_at: usize,
    after: Option<i128>,
    limit: usize,
    chunk_no: u32,
) -> ModelResult<FillChunk> {
    if limit == 0 {
        return fail("chunk_limit");
    }
    let state = history.state_at(read_at)?;
    let got = state.range_after(table, after, limit);
    let key_hi = got.last().map(|(key, _)| *key);
    Ok(FillChunk {
        table,
        chunk_no,
        read_at: read_at as u64,
        key_lo: after,
        key_hi,
        rows: got.into_iter().map(|(_, row)| row).collect(),
    })
}

/// One CDC batch: the events of the transactions `first_pos..=last_pos`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    /// First transaction position.
    pub first_pos: u64,
    /// Last transaction position.
    pub last_pos: u64,
    /// Net events, in position order.
    pub events: Vec<CdcEvent>,
}

/// One loader step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Apply chunk `index` of `table`.
    Fill {
        /// Table index.
        table: usize,
        /// Index in that table's chunk list.
        index: usize,
    },
    /// Apply batch `index`.
    Batch(usize),
}

/// Deterministic PRNG (SplitMix64). Only for tests and fixtures.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Seeded generator.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Next 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Value in `0..n`. Returns 0 when `n` is 0.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as usize
    }

    /// True with probability `num / den`.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        den != 0 && self.next_u64() % den < num
    }
}

/// Bounds of the random generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenParams {
    /// Maximum number of tables (at least 1).
    pub max_tables: usize,
    /// Key domain is `1..=keys`.
    pub keys: usize,
    /// Maximum number of rows before the first transaction.
    pub max_initial: usize,
    /// Maximum number of transactions.
    pub max_txns: usize,
    /// Maximum number of operations per transaction.
    pub max_ops: usize,
    /// Maximum rows per chunk (at least 1).
    pub max_chunk_rows: usize,
    /// Maximum gap between consecutive chunk reads, in transactions.
    pub max_read_gap: usize,
    /// Maximum transactions per batch (at least 1).
    pub max_batch_txns: usize,
}

impl GenParams {
    /// Parameters for randomized convergence runs.
    pub fn standard() -> Self {
        Self {
            max_tables: 2,
            keys: 24,
            max_initial: 10,
            max_txns: 24,
            max_ops: 3,
            max_chunk_rows: 5,
            max_read_gap: 2,
            max_batch_txns: 4,
        }
    }

    /// Small parameters, for enumerating every interleaving.
    pub fn exhaustive() -> Self {
        Self {
            max_tables: 2,
            keys: 4,
            max_initial: 3,
            max_txns: 3,
            max_ops: 2,
            max_chunk_rows: 4,
            max_read_gap: 1,
            max_batch_txns: 2,
        }
    }
}

/// Where chunk reads start relative to L0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadPolicy {
    /// Every read is at a prefix G_r ≥ L0. P1 holds.
    AfterL0,
    /// Each table's reads start at a random prefix before L0. This violates P1
    /// and is a negative control.
    BeforeL0,
}

/// A generated history with its chunk reads and batches.
#[derive(Clone, Debug)]
pub struct Scenario {
    /// Ground truth.
    pub history: SourceHistory,
    /// L0. The stream covers positions `l0 + 1..=N`.
    pub l0: u64,
    /// Chunks per table, in chunk order.
    pub chunks: Vec<Vec<FillChunk>>,
    /// Batches covering `l0 + 1..=N`, in order.
    pub batches: Vec<Batch>,
}

/// Result of applying one interleaving to a scenario.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunOutcome {
    /// Model state after all steps.
    pub model: ExpectedState,
    /// Ground truth S(L_end).
    pub truth: ExpectedState,
    /// Model counters.
    pub stats: ModelStats,
    /// Per-table comparison of the model rows (readback) with the truth (expected).
    pub tables: Vec<ReadbackDiff>,
}

impl RunOutcome {
    /// True when the model's composed digest equals the ground truth.
    pub fn matches(&self) -> bool {
        self.model == self.truth && self.tables.iter().all(ReadbackDiff::is_match)
    }
}

fn random_row(rng: &mut Rng, shape: &TableShape, key: i128) -> Row {
    shape
        .types()
        .iter()
        .enumerate()
        .map(|(col, ty)| {
            if col == shape.key() {
                return Cell::Int(key);
            }
            if rng.chance(1, 6) {
                return Cell::Null;
            }
            match ty {
                LogicalType::Int => Cell::Int(rng.below(7) as i128 - 3),
                LogicalType::Text => Cell::Text(TEXT_WORDS[rng.below(TEXT_WORDS.len())].to_owned()),
                LogicalType::Bool => Cell::Bool(rng.chance(1, 2)),
                _ => Cell::Null,
            }
        })
        .collect()
}

fn random_op(
    rng: &mut Rng,
    schema: &[TableShape],
    work: &SourceState,
    keys: usize,
) -> Option<SourceOp> {
    for _ in 0..8 {
        let table = rng.below(schema.len());
        let shape = schema.get(table)?;
        let key = 1 + rng.below(keys) as i128;
        let present = work.contains(table, key);
        match rng.below(4) {
            0 if !present => {
                return Some(SourceOp::Insert {
                    table,
                    row: random_row(rng, shape, key),
                });
            }
            1 if present => {
                return Some(SourceOp::Update {
                    table,
                    row: random_row(rng, shape, key),
                });
            }
            2 if present => return Some(SourceOp::Delete { table, key }),
            3 if present => {
                let new_key = 1 + rng.below(keys) as i128;
                if new_key != key && !work.contains(table, new_key) {
                    return Some(SourceOp::PkChange {
                        table,
                        old_key: key,
                        row: random_row(rng, shape, new_key),
                    });
                }
            }
            _ => {}
        }
    }
    None
}

fn shape_for(index: usize) -> ModelResult<TableShape> {
    match index {
        0 => TableShape::new(
            "t0",
            vec![
                LogicalType::Int,
                LogicalType::Text,
                LogicalType::Int,
                LogicalType::Bool,
            ],
            0,
        ),
        1 => TableShape::new(
            "t1",
            vec![LogicalType::Int, LogicalType::Int, LogicalType::Text],
            0,
        ),
        _ => TableShape::new(
            &format!("t{index}"),
            vec![LogicalType::Int, LogicalType::Text],
            0,
        ),
    }
}

impl Scenario {
    /// Generate a scenario from `seed`. The same seed always gives the same result.
    pub fn generate(seed: u64, params: &GenParams, policy: ReadPolicy) -> ModelResult<Self> {
        let mut rng = Rng::new(seed);
        let table_count = 1 + rng.below(params.max_tables.max(1));
        let schema = (0..table_count)
            .map(shape_for)
            .collect::<ModelResult<Vec<_>>>()?;

        let mut initial = SourceState::new(table_count);
        for _ in 0..rng.below(params.max_initial + 1) {
            let table = rng.below(table_count);
            let key = 1 + rng.below(params.keys) as i128;
            if !initial.contains(table, key) {
                let row = random_row(&mut rng, shape_of(&schema, table)?, key);
                initial.apply(&schema, &SourceOp::Insert { table, row })?;
            }
        }

        let mut work = initial.clone();
        let mut txns = Vec::new();
        for _ in 0..rng.below(params.max_txns + 1) {
            let mut ops = Vec::new();
            for _ in 0..rng.below(params.max_ops + 1) {
                if let Some(op) = random_op(&mut rng, &schema, &work, params.keys) {
                    work.apply(&schema, &op)?;
                    ops.push(op);
                }
            }
            txns.push(ops);
        }
        let history = SourceHistory::new(schema, initial, &txns, EventRules::correct())?;

        let total = history.txn_count();
        let l0 = rng.below(total + 1);
        let mut chunks = Vec::with_capacity(table_count);
        for table in 0..table_count {
            let limit = 1 + rng.below(params.max_chunk_rows.max(1));
            let mut read_at = match policy {
                ReadPolicy::AfterL0 => l0,
                ReadPolicy::BeforeL0 => rng.below(l0 + 1),
            };
            let mut after = None;
            let mut list: Vec<FillChunk> = Vec::new();
            loop {
                let chunk = read_chunk(
                    &history,
                    table,
                    read_at,
                    after,
                    limit,
                    list.len() as u32 + 1,
                )?;
                let next = chunk.key_hi;
                list.push(chunk);
                let Some(hi) = next else {
                    break;
                };
                after = Some(hi);
                read_at = (read_at + rng.below(params.max_read_gap + 1)).min(total);
            }
            chunks.push(list);
        }

        let mut batches = Vec::new();
        let mut pos = l0 as u64 + 1;
        let total_u64 = total as u64;
        while pos <= total_u64 {
            let last = (pos + rng.below(params.max_batch_txns.max(1)) as u64).min(total_u64);
            let mut events = Vec::new();
            for p in pos..=last {
                events.extend_from_slice(history.events_of(p)?);
            }
            batches.push(Batch {
                first_pos: pos,
                last_pos: last,
                events,
            });
            pos = last + 1;
        }

        Ok(Self {
            history,
            l0: l0 as u64,
            chunks,
            batches,
        })
    }

    /// Per-stream step lists: one per table for fills, then one for batches.
    /// Each list keeps its own order.
    pub fn streams(&self) -> Vec<Vec<Step>> {
        let mut out: Vec<Vec<Step>> = self
            .chunks
            .iter()
            .enumerate()
            .map(|(table, list)| {
                (0..list.len())
                    .map(|index| Step::Fill { table, index })
                    .collect()
            })
            .collect();
        out.push((0..self.batches.len()).map(Step::Batch).collect());
        out
    }

    /// A random interleaving that keeps each stream's order.
    pub fn random_steps(&self, rng: &mut Rng) -> Vec<Step> {
        let streams = self.streams();
        let mut cursors = vec![0usize; streams.len()];
        let mut out = Vec::new();
        loop {
            let live: Vec<usize> = (0..streams.len())
                .filter(|&s| cursors[s] < streams[s].len())
                .collect();
            if live.is_empty() {
                return out;
            }
            let s = live[rng.below(live.len())];
            out.push(streams[s][cursors[s]]);
            cursors[s] += 1;
        }
    }

    /// Every interleaving that keeps each stream's order, up to `cap`.
    pub fn interleavings(&self, cap: usize) -> Vec<Vec<Step>> {
        let streams = self.streams();
        let mut cursors = vec![0usize; streams.len()];
        let mut out = Vec::new();
        let mut current = Vec::new();
        collect_merges(&streams, &mut cursors, &mut current, &mut out, cap);
        out
    }

    /// Apply `steps` to a fresh model with `options` and compare the result with
    /// the ground truth S(N).
    pub fn run(&self, steps: &[Step], options: ModelOptions) -> ModelResult<RunOutcome> {
        let schema = self.history.schema();
        let mut model = Model::new(schema.to_vec(), options)?;
        for step in steps {
            match *step {
                Step::Fill { table, index } => {
                    let chunk = self
                        .chunks
                        .get(table)
                        .and_then(|list| list.get(index))
                        .ok_or(PreviewError::new(MODEL, "step_index"))?;
                    model.apply_fill(chunk)?;
                }
                Step::Batch(index) => {
                    let batch = self
                        .batches
                        .get(index)
                        .ok_or(PreviewError::new(MODEL, "step_index"))?;
                    model.apply_batch(batch.first_pos, batch.last_pos, &batch.events)?;
                }
            }
        }
        let total = self.history.txn_count();
        let truth = self.history.truth(options.bucket_bits, total)?;
        let truth_state = self.history.state_at(total)?;
        let truth_rows: Vec<Vec<Row>> = (0..schema.len()).map(|t| truth_state.rows(t)).collect();
        let model_rows: Vec<Vec<Row>> = (0..schema.len()).map(|t| model.rows(t)).collect();
        let mut tables = Vec::with_capacity(schema.len());
        for (shape, (want, got)) in schema.iter().zip(truth_rows.iter().zip(&model_rows)) {
            tables.push(compare_readback(options.bucket_bits, shape, want, got)?);
        }
        Ok(RunOutcome {
            model: model.expected()?,
            truth,
            stats: model.stats(),
            tables,
        })
    }
}

fn collect_merges(
    streams: &[Vec<Step>],
    cursors: &mut [usize],
    current: &mut Vec<Step>,
    out: &mut Vec<Vec<Step>>,
    cap: usize,
) {
    if out.len() >= cap {
        return;
    }
    if cursors
        .iter()
        .zip(streams)
        .all(|(cursor, stream)| *cursor >= stream.len())
    {
        out.push(current.clone());
        return;
    }
    for s in 0..streams.len() {
        if cursors[s] < streams[s].len() {
            current.push(streams[s][cursors[s]]);
            cursors[s] += 1;
            collect_merges(streams, cursors, current, out, cap);
            cursors[s] -= 1;
            current.pop();
            if out.len() >= cap {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_pipeline::c1_parts::{canonical_row, pg_text_cell};

    const BITS: u8 = 8;

    fn opts() -> ModelOptions {
        ModelOptions::new(BITS)
    }

    fn two_col() -> TableShape {
        TableShape::new("t", vec![LogicalType::Int, LogicalType::Text], 0).expect("shape")
    }

    fn r(key: i128, text: &str) -> Row {
        vec![Cell::Int(key), Cell::Text(text.to_owned())]
    }

    fn fill(
        table: usize,
        chunk_no: u32,
        lo: Option<i128>,
        hi: Option<i128>,
        rows: Vec<Row>,
    ) -> FillChunk {
        FillChunk {
            table,
            chunk_no,
            read_at: 0,
            key_lo: lo,
            key_hi: hi,
            rows,
        }
    }

    fn up(table: usize, key: i128, pos: u64, row: Row) -> CdcEvent {
        CdcEvent {
            table,
            key,
            pos,
            op: CdcOp::Upsert(row),
        }
    }

    fn del(table: usize, key: i128, pos: u64) -> CdcEvent {
        CdcEvent {
            table,
            key,
            pos,
            op: CdcOp::Delete,
        }
    }

    fn one_table_model() -> Model {
        Model::new(vec![two_col()], opts()).expect("model")
    }

    fn fact_of(result: ModelResult<impl Sized>) -> &'static str {
        match result {
            Ok(_) => "ok",
            Err(err) => err.fact,
        }
    }

    #[test]
    fn rng_is_deterministic_and_in_range() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..200 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut c = Rng::new(9);
        for _ in 0..500 {
            assert!(c.below(5) < 5);
        }
        assert_eq!(c.below(0), 0);
    }

    #[test]
    fn source_events_are_net_per_key_and_pk_change_is_delete_plus_upsert() {
        let schema = vec![two_col()];
        let mut initial = SourceState::new(1);
        initial
            .apply(
                &schema,
                &SourceOp::Insert {
                    table: 0,
                    row: r(1, "a"),
                },
            )
            .expect("insert");
        let txns = vec![
            vec![
                SourceOp::Insert {
                    table: 0,
                    row: r(5, "n"),
                },
                SourceOp::Delete { table: 0, key: 5 },
            ],
            vec![SourceOp::Update {
                table: 0,
                row: r(1, "b"),
            }],
            vec![SourceOp::PkChange {
                table: 0,
                old_key: 1,
                row: r(9, "c"),
            }],
            vec![
                SourceOp::Delete { table: 0, key: 9 },
                SourceOp::Insert {
                    table: 0,
                    row: r(9, "d"),
                },
            ],
        ];
        let history =
            SourceHistory::new(schema, initial, &txns, EventRules::correct()).expect("history");
        assert!(
            history.events_of(1).expect("pos 1").is_empty(),
            "insert then delete nets to nothing"
        );
        assert_eq!(
            history.events_of(2).expect("pos 2"),
            &[up(0, 1, 2, r(1, "b"))]
        );
        assert_eq!(
            history.events_of(3).expect("pos 3"),
            &[del(0, 1, 3), up(0, 9, 3, r(9, "c"))],
            "PK change is delete of the old key plus upsert of the new key"
        );
        assert_eq!(
            history.events_of(4).expect("pos 4"),
            &[up(0, 9, 4, r(9, "d"))],
            "delete then insert of one key nets to an upsert"
        );
        assert!(history.events_of(0).is_err());
        assert!(history.events_of(5).is_err());
    }

    #[test]
    fn fill_obeys_tombstone_and_existing_rule() {
        let mut m = one_table_model();
        let first = m
            .apply_fill(&fill(0, 1, None, Some(2), vec![r(1, "a"), r(2, "b")]))
            .expect("fill 1");
        assert_eq!(
            first,
            FillCounts {
                inserted: 2,
                skipped_tomb: 0,
                skipped_existing: 0
            }
        );
        m.apply_event(&del(0, 2, 5)).expect("delete 2 at 5");
        let second = m
            .apply_fill(&fill(0, 2, None, Some(3), vec![r(2, "x"), r(3, "d")]))
            .expect("fill 2");
        assert_eq!(
            second,
            FillCounts {
                inserted: 1,
                skipped_tomb: 1,
                skipped_existing: 0
            },
            "tombstoned key is not resurrected"
        );
        let third = m
            .apply_fill(&fill(0, 3, None, Some(1), vec![r(1, "z")]))
            .expect("fill 3");
        assert_eq!(
            third,
            FillCounts {
                inserted: 0,
                skipped_tomb: 0,
                skipped_existing: 1
            },
            "existing key is left alone"
        );
        assert_eq!(m.rows(0), vec![r(1, "a"), r(3, "d")]);
    }

    #[test]
    fn cdc_upsert_is_newest_wins_and_blocked_by_newer_tombstone() {
        let mut m = one_table_model();
        m.apply_event(&up(0, 1, 5, r(1, "new"))).expect("upsert 5");
        m.apply_event(&up(0, 1, 3, r(1, "old"))).expect("upsert 3");
        assert_eq!(
            m.rows(0),
            vec![r(1, "new")],
            "an older upsert does not overwrite"
        );
        m.apply_event(&del(0, 7, 9)).expect("delete 7 at 9");
        m.apply_event(&up(0, 7, 4, r(7, "x")))
            .expect("upsert 7 at 4");
        assert_eq!(
            m.rows(0),
            vec![r(1, "new")],
            "older upsert is blocked by the newer tombstone"
        );
        m.apply_event(&up(0, 7, 10, r(7, "y")))
            .expect("upsert 7 at 10");
        assert_eq!(m.rows(0), vec![r(1, "new"), r(7, "y")]);
        let stats = m.stats();
        assert_eq!(stats.upsert_stale, 1);
        assert_eq!(stats.upsert_blocked_tomb, 1);
        assert_eq!(stats.upsert_inserted, 2);
    }

    #[test]
    fn cdc_delete_removes_only_older_rows_and_keeps_max_tombstone() {
        let mut m = one_table_model();
        m.apply_fill(&fill(0, 1, None, Some(1), vec![r(1, "a")]))
            .expect("fill");
        m.apply_event(&up(0, 1, 4, r(1, "b"))).expect("upsert 4");
        m.apply_event(&del(0, 1, 3))
            .expect("delete at 3 keeps the newer row");
        assert_eq!(m.rows(0), vec![r(1, "b")]);
        m.apply_event(&del(0, 1, 6))
            .expect("delete at 6 removes it");
        assert!(m.rows(0).is_empty());
        m.apply_event(&del(0, 1, 5))
            .expect("older delete keeps tombstone at 6");
        let fill_counts = m
            .apply_fill(&fill(0, 1, None, Some(1), vec![r(1, "z")]))
            .expect("fill");
        assert_eq!(fill_counts.skipped_tomb, 1);
        m.apply_event(&up(0, 1, 5, r(1, "q")))
            .expect("upsert 5 is blocked by tombstone 6");
        assert!(m.rows(0).is_empty());
        assert_eq!(m.stats().delete_removed, 1);
    }

    #[test]
    fn pk_change_is_delete_old_then_upsert_new_and_the_bug_leaves_a_stale_row() {
        let schema = vec![two_col()];
        let mut initial = SourceState::new(1);
        initial
            .apply(
                &schema,
                &SourceOp::Insert {
                    table: 0,
                    row: r(1, "a"),
                },
            )
            .expect("insert");
        let txns = vec![vec![SourceOp::PkChange {
            table: 0,
            old_key: 1,
            row: r(9, "n"),
        }]];
        let truth = [r(9, "n")];

        let correct = SourceHistory::new(
            schema.clone(),
            initial.clone(),
            &txns,
            EventRules::correct(),
        )
        .expect("history");
        let steps = |h: &SourceHistory| -> ModelResult<Model> {
            let mut m = Model::new(schema.clone(), opts())?;
            m.apply_fill(&read_chunk(h, 0, 0, None, 10, 1)?)?;
            m.apply_batch(1, 1, h.events_of(1)?)?;
            m.apply_fill(&read_chunk(h, 0, 0, Some(1), 10, 2)?)?;
            Ok(m)
        };
        let good = steps(&correct).expect("run");
        assert_eq!(good.rows(0), truth.to_vec());

        let buggy = SourceHistory::new(
            schema.clone(),
            initial,
            &txns,
            EventRules {
                pk_change_deletes_old_key: false,
            },
        )
        .expect("history");
        let bad = steps(&buggy).expect("run");
        assert_eq!(
            bad.rows(0),
            vec![r(1, "a"), r(9, "n")],
            "without the old-key delete the old row survives"
        );
        let want = expected_digest(BITS, &two_col(), &truth).expect("digest");
        assert_ne!(
            bad.expected().expect("digest").composed_digest,
            want.composed_digest
        );
    }

    #[test]
    fn batch_keeps_only_last_event_per_key() {
        let mut m = one_table_model();
        let events = vec![up(0, 1, 2, r(1, "a")), del(0, 1, 3), up(0, 1, 4, r(1, "b"))];
        m.apply_batch(2, 4, &events).expect("batch");
        assert_eq!(m.rows(0), vec![r(1, "b")]);
        assert_eq!(
            m.stats().upsert_inserted,
            1,
            "only the last event of the key is applied"
        );
        assert_eq!(m.stats().delete_removed, 0);
    }

    #[test]
    fn batch_rejects_events_outside_range_or_repeated_key_position() {
        let mut m = one_table_model();
        assert_eq!(
            fact_of(m.apply_batch(2, 3, &[up(0, 1, 4, r(1, "a"))])),
            "batch_events"
        );
        assert_eq!(
            fact_of(m.apply_batch(2, 3, &[up(0, 1, 2, r(1, "a")), del(0, 1, 2)])),
            "batch_events"
        );
        assert_eq!(fact_of(m.apply_batch(0, 3, &[])), "batch_range");
        assert_eq!(fact_of(m.apply_event(&up(0, 1, 0, r(1, "a")))), "cdc_pos");
        assert_eq!(fact_of(m.apply_event(&up(0, 2, 1, r(1, "a")))), "event_key");
        assert!(m.rows(0).is_empty(), "rejected events change nothing");
    }

    #[test]
    fn fill_rejects_malformed_chunks_before_writing() {
        let mut m = one_table_model();
        assert_eq!(
            fact_of(m.apply_fill(&fill(0, 1, None, Some(2), vec![r(3, "c")]))),
            "chunk_keys"
        );
        assert_eq!(
            fact_of(m.apply_fill(&fill(0, 1, None, None, vec![r(1, "a")]))),
            "chunk_terminal"
        );
        assert_eq!(
            fact_of(m.apply_fill(&fill(0, 1, None, Some(3), vec![r(2, "b"), r(1, "a")]))),
            "chunk_keys"
        );
        assert_eq!(
            fact_of(m.apply_fill(&fill(0, 1, Some(2), Some(2), vec![]))),
            "chunk_keys"
        );
        assert_eq!(
            fact_of(m.apply_fill(&fill(0, 1, Some(1), Some(3), vec![r(1, "a")]))),
            "chunk_keys"
        );
        assert_eq!(
            fact_of(m.apply_fill(&fill(0, 0, None, None, vec![]))),
            "chunk_no"
        );
        assert!(m.rows(0).is_empty());
        assert_eq!(
            fact_of(m.apply_fill(&fill(5, 1, None, None, vec![]))),
            "table_index"
        );
    }

    #[test]
    fn source_ops_reject_invalid_transitions() {
        let schema = vec![two_col()];
        let mut s = SourceState::new(1);
        s.apply(
            &schema,
            &SourceOp::Insert {
                table: 0,
                row: r(1, "a"),
            },
        )
        .expect("insert");
        assert_eq!(
            fact_of(s.apply(
                &schema,
                &SourceOp::Insert {
                    table: 0,
                    row: r(1, "b")
                }
            )),
            "source_insert"
        );
        assert_eq!(
            fact_of(s.apply(&schema, &SourceOp::Delete { table: 0, key: 2 })),
            "source_delete"
        );
        assert_eq!(
            fact_of(s.apply(
                &schema,
                &SourceOp::Update {
                    table: 0,
                    row: r(2, "b")
                }
            )),
            "source_update"
        );
        s.apply(
            &schema,
            &SourceOp::Insert {
                table: 0,
                row: r(3, "c"),
            },
        )
        .expect("insert 3");
        assert_eq!(
            fact_of(s.apply(
                &schema,
                &SourceOp::PkChange {
                    table: 0,
                    old_key: 1,
                    row: r(3, "x")
                }
            )),
            "source_pk_change"
        );
    }

    #[test]
    fn model_rejects_duplicate_table_names() {
        let a = TableShape::new("dup", vec![LogicalType::Int], 0).expect("a");
        let b = TableShape::new("dup", vec![LogicalType::Int], 0).expect("b");
        assert_eq!(fact_of(Model::new(vec![a, b], opts())), "table_name");
        assert_eq!(
            fact_of(TableShape::new("k", vec![LogicalType::Text], 0)),
            "key_type"
        );
    }

    #[test]
    fn negative_control_read_before_l0_loses_updates() {
        let schema = vec![two_col()];
        let mut initial = SourceState::new(1);
        initial
            .apply(
                &schema,
                &SourceOp::Insert {
                    table: 0,
                    row: r(1, "a"),
                },
            )
            .expect("insert");
        let history = SourceHistory::new(
            schema,
            initial,
            &[vec![SourceOp::Update {
                table: 0,
                row: r(1, "b"),
            }]],
            EventRules::correct(),
        )
        .expect("history");
        // L0 = 1, so the stream has no batches. A chunk read at prefix 0 is before L0.
        let run = |read_at: usize| -> ModelResult<RunOutcome> {
            let chunks = vec![vec![
                read_chunk(&history, 0, read_at, None, 10, 1)?,
                read_chunk(&history, 0, read_at, Some(1), 10, 2)?,
            ]];
            let scenario = Scenario {
                history: history.clone(),
                l0: 1,
                chunks,
                batches: Vec::new(),
            };
            scenario.run(
                &[
                    Step::Fill { table: 0, index: 0 },
                    Step::Fill { table: 0, index: 1 },
                ],
                opts(),
            )
        };
        let stale = run(0).expect("stale run");
        assert!(
            !stale.matches(),
            "a chunk read before L0 keeps the old value"
        );
        assert_eq!(stale.tables[0].changed, 1);
        assert!(run(1).expect("read after L0").matches());
    }

    #[test]
    fn negative_control_without_tombstones_resurrects_deleted_key() {
        let schema = vec![two_col()];
        let mut initial = SourceState::new(1);
        initial
            .apply(
                &schema,
                &SourceOp::Insert {
                    table: 0,
                    row: r(1, "a"),
                },
            )
            .expect("insert");
        let history = SourceHistory::new(
            schema,
            initial,
            &[vec![SourceOp::Delete { table: 0, key: 1 }]],
            EventRules::correct(),
        )
        .expect("history");
        let chunks = vec![vec![
            read_chunk(&history, 0, 0, None, 10, 1).expect("chunk 1"),
            read_chunk(&history, 0, 0, Some(1), 10, 2).expect("chunk 2"),
        ]];
        let batches = vec![Batch {
            first_pos: 1,
            last_pos: 1,
            events: history.events_of(1).expect("events").to_vec(),
        }];
        let scenario = Scenario {
            history,
            l0: 0,
            chunks,
            batches,
        };
        let cdc_first = [
            Step::Batch(0),
            Step::Fill { table: 0, index: 0 },
            Step::Fill { table: 0, index: 1 },
        ];
        let fill_first = [
            Step::Fill { table: 0, index: 0 },
            Step::Fill { table: 0, index: 1 },
            Step::Batch(0),
        ];
        assert!(scenario.run(&cdc_first, opts()).expect("run").matches());
        assert!(scenario.run(&fill_first, opts()).expect("run").matches());
        let no_tomb = ModelOptions {
            bucket_bits: BITS,
            tombstones: false,
        };
        let bad = scenario.run(&cdc_first, no_tomb).expect("run");
        assert!(
            !bad.matches(),
            "without tombstones a fill after the delete resurrects the key"
        );
        assert_eq!(bad.tables[0].extra, 1);
        assert!(
            scenario.run(&fill_first, no_tomb).expect("run").matches(),
            "fill-first order needs no tombstone"
        );
    }

    #[test]
    fn property_batch_collapse_equals_per_event_application() {
        for seed in 0..200u64 {
            let scenario =
                Scenario::generate(seed, &GenParams::standard(), ReadPolicy::AfterL0).expect("gen");
            let steps = scenario.random_steps(&mut Rng::new(seed ^ 0xABCD));
            let batched = scenario.run(&steps, opts()).expect("run");
            let per_event = run_per_event(&scenario, &steps).expect("per event");
            assert!(
                batched.matches(),
                "seed {seed}: batch mode diverges from truth"
            );
            assert_eq!(
                per_event, batched.model,
                "seed {seed}: per-event and batch application differ"
            );
        }
    }

    fn run_per_event(scenario: &Scenario, steps: &[Step]) -> ModelResult<ExpectedState> {
        let mut model = Model::new(scenario.history.schema().to_vec(), opts())?;
        for step in steps {
            match *step {
                Step::Fill { table, index } => {
                    model.apply_fill(&scenario.chunks[table][index])?;
                }
                Step::Batch(index) => {
                    let mut events = scenario.batches[index].events.clone();
                    events.sort_by_key(|event| event.pos);
                    for event in &events {
                        model.apply_event(event)?;
                    }
                }
            }
        }
        model.expected()
    }

    #[test]
    fn event_log_replays_to_ground_truth() {
        for seed in 0..200u64 {
            let scenario =
                Scenario::generate(seed, &GenParams::standard(), ReadPolicy::AfterL0).expect("gen");
            let history = &scenario.history;
            let mut model = Model::new(history.schema().to_vec(), opts()).expect("model");
            for table in 0..history.schema().len() {
                let snapshot = read_chunk(history, table, 0, None, 1000, 1).expect("snapshot");
                model.apply_fill(&snapshot).expect("fill S(0)");
            }
            for pos in 1..=history.txn_count() as u64 {
                for event in history.events_of(pos).expect("events") {
                    model.apply_event(event).expect("apply");
                }
            }
            let truth = history.truth(BITS, history.txn_count()).expect("truth");
            assert_eq!(
                model.expected().expect("model"),
                truth,
                "seed {seed}: events do not replay to S(N)"
            );
        }
    }

    #[test]
    fn property_random_histories_converge_to_ground_truth() {
        let mut total = ModelStats::default();
        let mut runs = 0u64;
        let mut pk_changes = 0usize;
        let mut multi_chunk = 0usize;
        let mut multi_table = 0usize;
        for seed in 0..300u64 {
            let scenario =
                Scenario::generate(seed, &GenParams::standard(), ReadPolicy::AfterL0).expect("gen");
            pk_changes += scenario
                .history
                .txns()
                .iter()
                .flatten()
                .filter(|op| matches!(op, SourceOp::PkChange { .. }))
                .count();
            multi_chunk += scenario
                .chunks
                .iter()
                .filter(|list| list.len() >= 2)
                .count();
            multi_table += usize::from(scenario.history.schema().len() >= 2);
            let mut rng = Rng::new(seed.wrapping_mul(0x51));
            for _ in 0..3 {
                let steps = scenario.random_steps(&mut rng);
                let outcome = scenario.run(&steps, opts()).expect("run");
                assert!(
                    outcome.matches(),
                    "seed {seed}: model {:?} != truth {:?}",
                    outcome.model,
                    outcome.truth
                );
                runs += 1;
                let s = outcome.stats;
                total.fill_inserted += s.fill_inserted;
                total.fill_skipped_tomb += s.fill_skipped_tomb;
                total.fill_skipped_existing += s.fill_skipped_existing;
                total.upsert_inserted += s.upsert_inserted;
                total.upsert_overwrote += s.upsert_overwrote;
                total.upsert_blocked_tomb += s.upsert_blocked_tomb;
                total.delete_removed += s.delete_removed;
            }
        }
        assert_eq!(runs, 900);
        assert!(
            total.fill_skipped_tomb > 0,
            "tombstone blocks a fill: {total:?}"
        );
        assert!(
            total.fill_skipped_existing > 0,
            "fill meets an existing row: {total:?}"
        );
        assert!(
            total.upsert_overwrote > 0,
            "upsert overwrites a fill row: {total:?}"
        );
        assert!(total.delete_removed > 0, "delete removes a row: {total:?}");
        assert!(total.fill_inserted > 0 && total.upsert_inserted > 0);
        assert!(
            pk_changes > 0 && multi_chunk > 0 && multi_table > 0,
            "generator covers PK change, chunking, tables"
        );
    }

    #[test]
    fn exhaustive_small_histories_all_interleavings_converge() {
        const CAP: usize = 2000;
        let mut checked = 0usize;
        let mut skipped = 0usize;
        for seed in 0..150u64 {
            let scenario = Scenario::generate(seed, &GenParams::exhaustive(), ReadPolicy::AfterL0)
                .expect("gen");
            let merges = scenario.interleavings(CAP + 1);
            if merges.len() > CAP {
                skipped += 1;
                continue;
            }
            for steps in &merges {
                let outcome = scenario.run(steps, opts()).expect("run");
                assert!(
                    outcome.matches(),
                    "seed {seed}: interleaving {steps:?} diverges"
                );
                checked += 1;
            }
        }
        assert!(
            checked > 1000,
            "checked {checked} interleavings, skipped {skipped} seeds"
        );
        assert!(skipped < 150, "every seed was skipped");
    }

    #[test]
    fn negative_control_random_read_before_l0_produces_wrong_state() {
        let mut wrong = 0usize;
        for seed in 0..300u64 {
            let scenario = Scenario::generate(seed, &GenParams::standard(), ReadPolicy::BeforeL0)
                .expect("gen");
            let steps = scenario.random_steps(&mut Rng::new(seed));
            if !scenario.run(&steps, opts()).expect("run").matches() {
                wrong += 1;
            }
        }
        assert!(
            wrong > 0,
            "reads before L0 must produce a wrong state in some histories"
        );
    }

    #[test]
    fn negative_control_random_without_tombstones_produces_wrong_state() {
        let no_tomb = ModelOptions {
            bucket_bits: BITS,
            tombstones: false,
        };
        let mut wrong = 0usize;
        for seed in 0..300u64 {
            let scenario =
                Scenario::generate(seed, &GenParams::standard(), ReadPolicy::AfterL0).expect("gen");
            let steps = scenario.random_steps(&mut Rng::new(seed));
            if !scenario.run(&steps, no_tomb).expect("run").matches() {
                wrong += 1;
            }
        }
        assert!(
            wrong > 0,
            "dropping tombstones must produce a wrong state in some histories"
        );
    }

    #[test]
    fn expected_digest_is_order_independent_and_matches_manual_composition() {
        let shape = two_col();
        let forward = vec![r(1, "a"), r(2, "b"), r(40, "c")];
        let mut reversed = forward.clone();
        reversed.reverse();
        let a = expected_digest(BITS, &shape, &forward).expect("forward");
        let b = expected_digest(BITS, &shape, &reversed).expect("reversed");
        assert_eq!(a, b);
        let mut manual = StateDigest::new(BITS).expect("digest");
        for (key, row) in [(1, &forward[0]), (2, &forward[1]), (40, &forward[2])] {
            manual
                .add_row(key, &canonical_row(shape.types(), row).expect("canonical"))
                .expect("add");
        }
        let composed = composed_digest(&[("t", &manual)]).expect("composed");
        assert_eq!(a.composed_digest, composed);
        assert_eq!(a.rows, 3);
        assert_eq!(a.tables, vec![("t".to_owned(), 3)]);
    }

    #[test]
    fn expected_digest_rejects_malformed_rows() {
        let shape = two_col();
        assert!(
            expected_digest(BITS, &shape, &[vec![Cell::Int(1)]]).is_err(),
            "width"
        );
        assert!(
            expected_digest(BITS, &shape, &[vec![Cell::Null, Cell::Text("a".into())]]).is_err(),
            "null key"
        );
        assert!(
            expected_digest(
                BITS,
                &shape,
                &[vec![Cell::Text("1".into()), Cell::Text("a".into())]]
            )
            .is_err(),
            "text key"
        );
        assert!(
            expected_digest(BITS, &shape, &[vec![Cell::Int(1), Cell::Int(2)]]).is_err(),
            "type mismatch"
        );
    }

    #[test]
    fn compare_readback_detects_missing_extra_changed_and_duplicate() {
        let shape = two_col();
        let expected = vec![r(1, "a"), r(2, "b"), r(3, "c")];
        assert!(
            compare_readback(BITS, &shape, &expected, &expected)
                .expect("same")
                .is_match()
        );

        let missing =
            compare_readback(BITS, &shape, &expected, &[r(1, "a"), r(3, "c")]).expect("missing");
        assert_eq!((missing.missing, missing.extra, missing.changed), (1, 0, 0));
        assert_eq!(missing.first_bad_key, Some(2));
        assert!(!missing.is_match());

        let extra = compare_readback(
            BITS,
            &shape,
            &expected,
            &[r(1, "a"), r(2, "b"), r(3, "c"), r(9, "z")],
        )
        .expect("extra");
        assert_eq!((extra.missing, extra.extra), (0, 1));
        assert_eq!(extra.first_bad_key, Some(9));

        let changed = compare_readback(BITS, &shape, &expected, &[r(1, "a"), r(2, "b"), r(3, "x")])
            .expect("changed");
        assert_eq!(changed.changed, 1);
        assert_eq!(changed.first_bad_key, Some(3));
        assert!(!changed.digest_equal);

        let dup = compare_readback(
            BITS,
            &shape,
            &expected,
            &[r(1, "a"), r(1, "a"), r(2, "b"), r(3, "c")],
        )
        .expect("dup");
        assert_eq!(dup.duplicate, 1);
        assert_eq!(dup.readback_rows, 4);
        assert!(!dup.is_match());
    }

    #[test]
    fn compare_readback_accepts_postgres_text_readback() {
        let types = vec![LogicalType::Int, LogicalType::Text, LogicalType::Bool];
        let shape = TableShape::new("pg", types, 0).expect("shape");
        let expected = vec![
            vec![Cell::Int(1), Cell::Text("x".into()), Cell::Bool(true)],
            vec![Cell::Int(2), Cell::Null, Cell::Bool(false)],
        ];
        let readback = vec![
            vec![
                pg_text_cell(20, Some("1")).expect("int"),
                pg_text_cell(25, Some("x")).expect("text"),
                pg_text_cell(16, Some("t")).expect("bool"),
            ],
            vec![
                pg_text_cell(20, Some("2")).expect("int"),
                pg_text_cell(25, None).expect("null"),
                pg_text_cell(16, Some("f")).expect("bool"),
            ],
        ];
        let diff = compare_readback(BITS, &shape, &expected, &readback).expect("compare");
        assert!(diff.is_match(), "{diff:?}");
    }
}
