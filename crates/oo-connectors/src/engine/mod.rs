//! Engine-neutral source interface for v0.2 (REQ-ENG-01).
//!
//! Every database connector implements [`SourceEngine`]; engines with a log
//! or change feed also implement [`ChangeSource`]. Version differences are
//! declared as [`VersionRule`] rows instead of scattered branches, and
//! [`ProbeReport`] is what `oo doctor` renders, including statements a DBA
//! can run. The protocol is [`ENGINE_PROTOCOL`].
//!
//! Chunking follows docs/product/02: keyset bounds are compared by the source
//! under the column's own collation ([`ChunkStrategy::Keyset`]); a physical
//! range ([`ChunkStrategy::PhysicalRange`]) is only valid inside one snapshot.
//! Chunk sizes are internal resource budgets, never business limits.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;

use arrow_array::RecordBatch;
use serde::{Deserialize, Serialize};

use crate::SourceError;

pub(crate) mod declared;
pub(crate) mod shape;
mod version;

pub use version::{EngineVersion, VersionRange};

/// Wire protocol of probe reports, chunk plans and receipts.
pub const ENGINE_PROTOCOL: &str = "oo.source-engine/v1";

/// Database family. A compatible product is its own family (D10): MariaDB,
/// TiDB and OceanBase are not MySQL, openGauss is not PostgreSQL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineFamily {
    Mysql,
    Mariadb,
    Tidb,
    Oceanbase,
    Postgres,
    Opengauss,
    Oracle,
    Sqlserver,
    Db2,
    Dameng,
    Kingbase,
    Clickhouse,
    Doris,
    Starrocks,
    Mongodb,
    Opensearch,
}

/// One capability of a target, graded per version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Catalog,
    ChunkedSnapshot,
    Watermark,
    /// C1: log-based change data capture.
    LogCdc,
    /// C2: engine-native change reading (change tracking, change streams).
    NativeChanges,
    /// C4: partition refresh.
    PartitionRefresh,
    /// C5: range-digest reconciliation. Not CDC.
    DigestDiff,
    ReplicaRead,
}

/// Whether a capability is usable on the probed instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CapabilityState {
    Available,
    /// Usable once every listed requirement is met.
    NeedsSetup {
        requirements: Vec<Requirement>,
    },
    /// Not offered by this version or variant.
    Unsupported {
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementKind {
    Privilege,
    Parameter,
    Feature,
    Retention,
}

/// A missing privilege, parameter or feature. `dba_statement` is something a
/// DBA can run as is; it never contains credentials.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub kind: RequirementKind,
    pub name: String,
    pub observed: Option<String>,
    pub required: String,
    pub dba_statement: Option<String>,
}

/// One row of an engine's version dispatch table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRule {
    pub family: EngineFamily,
    pub versions: VersionRange,
    pub capability: Capability,
    /// Short statement of the version-specific behavior, e.g.
    /// "SHOW BINARY LOG STATUS replaces SHOW MASTER STATUS".
    pub note: String,
}

impl VersionRule {
    /// Rules that apply to `version`, in table order.
    pub fn applicable<'a>(
        rules: &'a [VersionRule],
        family: EngineFamily,
        version: &'a EngineVersion,
    ) -> impl Iterator<Item = &'a VersionRule> + 'a {
        rules
            .iter()
            .filter(move |r| r.family == family && r.versions.contains(version))
    }
}

/// Result of a read-only probe; the body of `oo doctor`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeReport {
    pub protocol: String,
    pub family: EngineFamily,
    pub version: EngineVersion,
    pub capabilities: BTreeMap<Capability, CapabilityState>,
    /// Observed settings that drive decisions (log format, retention, ...).
    /// Values are settings, never secrets.
    pub settings: BTreeMap<String, String>,
}

impl ProbeReport {
    /// Every requirement across capabilities, deduplicated by name.
    pub fn requirements(&self) -> Vec<&Requirement> {
        let mut seen = std::collections::BTreeSet::new();
        self.capabilities
            .values()
            .filter_map(|s| match s {
                CapabilityState::NeedsSetup { requirements } => Some(requirements),
                _ => None,
            })
            .flatten()
            .filter(|r| seen.insert(r.name.as_str()))
            .collect()
    }
}

/// Fully qualified table name as the source spells it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TableRef {
    pub schema: String,
    pub name: String,
}

impl fmt::Display for TableRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.schema, self.name)
    }
}

/// Key shape found by the probe; it selects the chunk strategy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum KeyShape {
    IntegerPrimaryKey {
        column: String,
    },
    StringPrimaryKey {
        column: String,
        collation: Option<String>,
    },
    CompositePrimaryKey {
        columns: Vec<String>,
    },
    UniqueIndex {
        index: String,
        columns: Vec<String>,
    },
    NoKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalRangeKind {
    /// PostgreSQL 14+ TID range scan.
    Ctid,
    /// Oracle ROWID ranges built from extents, no source-side task.
    Rowid,
}

/// How a table is cut into chunks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "strategy", rename_all = "snake_case")]
pub enum ChunkStrategy {
    /// Keyset bounds compared by the source under the column collation.
    /// The client never compares strings.
    Keyset { columns: Vec<String> },
    /// Physical page or row ranges, valid only inside one snapshot.
    PhysicalRange { kind: PhysicalRangeKind },
    /// No usable key: one bounded read of the whole table.
    SingleRead,
}

/// One key value as the source returned it, kept lossless.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum KeyValue {
    Null,
    Int(i128),
    /// Decimal or other exact numeric in canonical text.
    Decimal(String),
    Text(String),
    /// Hex of raw bytes.
    Bytes(String),
    /// Opaque physical locator (ctid, ROWID).
    Locator(String),
}

/// Inclusive-exclusive bound `[lo, hi)`; `None` is unbounded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkSpec {
    pub table: TableRef,
    pub plan_revision: u32,
    pub chunk_id: u32,
    pub lo: Option<Vec<KeyValue>>,
    pub hi: Option<Vec<KeyValue>>,
    pub estimated_rows: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableChunkPlan {
    pub protocol: String,
    pub table: TableRef,
    pub key: KeyShape,
    pub strategy: ChunkStrategy,
    pub estimated_rows: Option<u64>,
    pub chunks: Vec<ChunkSpec>,
}

/// Read budget for one engine session. Internal resource limits only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadBudget {
    pub max_chunk_rows: u64,
    pub max_chunk_bytes: u64,
    pub target_chunk_ms: u64,
    /// Read time over wall time, in percent. 25 means sleep ≥ 3× read time.
    pub duty_cycle_percent: u8,
    pub statement_timeout_ms: u64,
}

impl Default for ReadBudget {
    fn default() -> Self {
        Self {
            max_chunk_rows: 100_000,
            max_chunk_bytes: 32 * 1024 * 1024,
            target_chunk_ms: 500,
            duty_cycle_percent: 25,
            statement_timeout_ms: 30_000,
        }
    }
}

/// Read-only position used as a chunk watermark and a CDC start point.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Watermark {
    MysqlGtidSet {
        gtid_set: String,
    },
    MysqlFilePos {
        file: String,
        position: u64,
    },
    PgLsn {
        lsn: u64,
    },
    OracleScn {
        scn: u64,
    },
    /// Engines without a log position; the value is engine-defined.
    Opaque {
        value: String,
    },
}

/// Receipt of one chunk read. `sha256` covers the canonical Arrow IPC bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkReceipt {
    pub table: TableRef,
    pub plan_revision: u32,
    pub chunk_id: u32,
    pub rows: u64,
    pub bytes: u64,
    pub sha256: String,
    pub low: Option<Watermark>,
    pub high: Option<Watermark>,
}

/// Count and order-independent digest of one key range (C5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeDigest {
    pub table: TableRef,
    pub lo: Option<Vec<KeyValue>>,
    pub hi: Option<Vec<KeyValue>>,
    pub rows: u64,
    pub digest: String,
}

/// Receives Arrow batches of one chunk. Returning an error stops the read.
pub trait ChunkSink: Send {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError>;
}

/// Declared metadata only. Comments are untrusted text, never instructions.
#[derive(Clone, Serialize, Deserialize)]
pub struct DeclaredSourceCatalog {
    pub protocol: String,
    pub source_id: String,
    pub tables: Vec<DeclaredSourceTable>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DeclaredSourceTable {
    pub schema: String,
    pub name: String,
    pub comment: Option<String>,
    pub columns: Vec<DeclaredSourceColumn>,
    pub primary_key: Vec<crate::KeyColumn>,
    pub foreign_keys: Vec<crate::ForeignKey>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DeclaredSourceColumn {
    pub name: String,
    pub not_null: bool,
    pub comment: Option<String>,
}

/// Fresh source catalog facts, before hashing or target projection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceColumnShape {
    pub name: String,
    pub ordinal: u32,
    /// Complete declaration, including length, precision, scale and signedness.
    pub source_type: String,
    /// Closed mapped type, using the same mapping as the chunk reader.
    pub target_type: String,
    pub nullable: bool,
    pub charset: Option<String>,
    pub collation: Option<String>,
    pub primary_key_ordinal: Option<u32>,
    /// Unique index name to this column's position in that index.
    pub unique_key_ordinals: BTreeMap<String, u32>,
}

/// One table found by bounded catalog discovery. Every field comes from
/// catalog metadata and optimizer statistics; no table row is read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredTable {
    pub table: TableRef,
    /// Statistics estimate; `None` when the table was never analyzed.
    pub estimated_rows: Option<u64>,
    pub has_primary_key: bool,
    /// A valid, non-partial unique index or constraint besides the primary key.
    pub has_unique_key: bool,
    /// A partition of a partitioned table (read through its parent).
    pub partition_child: bool,
    /// Distinct targets of outgoing foreign keys, sorted.
    pub references: Vec<TableRef>,
}

/// Upper bound on the tables one discovery call returns.
pub const DISCOVERY_TABLE_LIMIT: u64 = 10_000;

/// Read-only source engine. Implementations open no write transaction,
/// create no object and take no lock.
pub trait SourceEngine: Send {
    /// None is unproven, never an empty observed table or inferred equality.
    /// Concrete engines must refresh this from their native metadata adapters.
    fn column_shape(
        &mut self,
        _table: &TableRef,
        _budget: &ReadBudget,
    ) -> impl Future<Output = Result<Option<Vec<SourceColumnShape>>, SourceError>> + Send {
        async { Ok(None) }
    }

    /// Selected-table metadata within the source read budget. None means
    /// unavailable, not an observed empty catalog. No new source privileges.
    /// Implementations return protocol oo.builder.source-catalog/v1.
    fn declared_catalog(
        &mut self,
        _tables: &[TableRef],
        _budget: &ReadBudget,
    ) -> impl Future<Output = Result<Option<DeclaredSourceCatalog>, SourceError>> + Send {
        async { Ok(None) }
    }

    fn family(&self) -> EngineFamily;

    /// Bounded catalog discovery; never enumerate rows from source tables.
    fn discover_tables(
        &mut self,
        _budget: &ReadBudget,
    ) -> impl Future<Output = Result<Vec<TableRef>, SourceError>> + Send {
        async { Err(SourceError::config("discovery_unavailable")) }
    }

    /// Advisory byte estimate. None means unavailable, never zero.
    fn estimate_bytes(
        &mut self,
        _table: &TableRef,
    ) -> impl Future<Output = Result<Option<u64>, SourceError>> + Send {
        async { Ok(None) }
    }

    /// Identify the version and grade capabilities without reading rows.
    fn probe(&mut self) -> impl Future<Output = Result<ProbeReport, SourceError>> + Send;

    /// Key shape, strategy and chunks for one table under `budget`.
    fn plan_chunks(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> impl Future<Output = Result<TableChunkPlan, SourceError>> + Send;

    /// Read one chunk into `sink`, bracketed by watermarks when available.
    /// Empty chunks must push a schema-bearing empty batch, included in the IPC hash.
    fn read_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> impl Future<Output = Result<ChunkReceipt, SourceError>> + Send;

    /// Optional source-authenticated replacement for a chunk that outgrew its
    /// budget. Children must exactly cover the parent's bounds, have distinct
    /// IDs and a newer revision, and be registered for subsequent read_chunk.
    /// None means the caller must replan the whole table. Physical ranges must
    /// never escape their owning snapshot.
    fn split_chunk(
        &mut self,
        _chunk: &ChunkSpec,
        _budget: &ReadBudget,
    ) -> impl Future<Output = Result<Option<Vec<ChunkSpec>>, SourceError>> + Send {
        async { Ok(None) }
    }

    /// Current read-only position, or `None` when the engine has none.
    fn watermark(&mut self) -> impl Future<Output = Result<Option<Watermark>, SourceError>> + Send;

    /// C5 digest of one key range.
    fn digest_range(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> impl Future<Output = Result<RangeDigest, SourceError>> + Send;
}

/// One decoded row change from a log or change feed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RowChange {
    Insert {
        table: TableRef,
        key: Vec<KeyValue>,
    },
    Update {
        table: TableRef,
        key: Vec<KeyValue>,
        old_key: Option<Vec<KeyValue>>,
    },
    Delete {
        table: TableRef,
        key: Vec<KeyValue>,
    },
}

/// Changes up to and including `position`, in source commit order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangeBatch {
    pub position: Watermark,
    pub changes: Vec<RowChange>,
    /// Row images as Arrow batches, aligned with `changes` per table.
    #[serde(skip)]
    pub images: Vec<RecordBatch>,
}

/// Log CDC (C1) or native change reading (C2).
pub trait ChangeSource: Send {
    /// Earliest position the source can still serve. Retention is checked
    /// against this, not against configured parameters.
    fn earliest_available(&mut self)
    -> impl Future<Output = Result<Watermark, SourceError>> + Send;

    /// Next batch after `from`, or `None` when caught up.
    fn next_batch(
        &mut self,
        from: &Watermark,
        budget: &ReadBudget,
    ) -> impl Future<Output = Result<Option<ChangeBatch>, SourceError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watermark_round_trips() {
        for w in [
            Watermark::MysqlGtidSet {
                gtid_set: "3e11fa47-71ca-11e1-9e33-c80aa9429562:1-5".into(),
            },
            Watermark::MysqlFilePos {
                file: "binlog.000002".into(),
                position: 4,
            },
            Watermark::PgLsn { lsn: 0x16B3748 },
            Watermark::OracleScn { scn: 2_451_901 },
        ] {
            let json = serde_json::to_string(&w).unwrap();
            assert_eq!(serde_json::from_str::<Watermark>(&json).unwrap(), w);
        }
    }

    #[test]
    fn requirements_are_deduplicated() {
        let need = |name: &str| Requirement {
            kind: RequirementKind::Parameter,
            name: name.into(),
            observed: Some("OFF".into()),
            required: "ON".into(),
            dba_statement: None,
        };
        let report = ProbeReport {
            protocol: ENGINE_PROTOCOL.into(),
            family: EngineFamily::Mysql,
            version: EngineVersion::parse(EngineFamily::Mysql, "5.7.44-log").unwrap(),
            capabilities: BTreeMap::from([
                (
                    Capability::LogCdc,
                    CapabilityState::NeedsSetup {
                        requirements: vec![need("log_bin"), need("binlog_row_image")],
                    },
                ),
                (
                    Capability::Watermark,
                    CapabilityState::NeedsSetup {
                        requirements: vec![need("log_bin")],
                    },
                ),
                (Capability::DigestDiff, CapabilityState::Available),
            ]),
            settings: BTreeMap::new(),
        };
        let names: Vec<_> = report
            .requirements()
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, ["log_bin", "binlog_row_image"]);
    }

    #[test]
    fn default_budget_matches_playbook() {
        let b = ReadBudget::default();
        assert_eq!(
            (b.max_chunk_rows, b.max_chunk_bytes),
            (100_000, 32 * 1024 * 1024)
        );
        assert_eq!(b.duty_cycle_percent, 25);
    }
}
