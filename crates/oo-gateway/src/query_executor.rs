//! Permission-bound `SELECT` for one trusted runtime.
//!
//! [`DedicatedSession::execute_trusted_select`] is the only entry. It calls
//! [`crate::sql_guard::guard_select`] on the caller's original SQL and runs
//! `GuardedPlan::executed_sql`. The caller has no field for executed SQL.
//!
//! Untrusted parser admission stays closed. The parser worker exists, but its
//! kernel sandbox is unsupported and this crate has no authentication gate, so
//! [`UNTRUSTED_ADMISSION`] is [`UntrustedAdmission::Blocked`]. There is no
//! public function that accepts an untrusted statement.
//!
//! The session is one dedicated `tokio_postgres` client. This slice has no
//! pool. The user statement is a prepared portal fetched one row at a time.
//! `query_portal(..., 0)` is forbidden because that asks the server for every
//! row. Platform `BEGIN` / `SET LOCAL` may use the transaction builder and
//! `batch_execute`. User SQL never uses `simple_query`.
//!
//! PostgreSQL restarts `statement_timeout` on each portal `Execute`. This
//! executor sets that timer once, to the remaining client budget minus about
//! 250 ms, and does not refresh it per page. The whole-query bound is one
//! client [`Instant`] taken at `execute_trusted_select` entry. It covers
//! admission, blocking, network, and drain.
//!
//! The first syntactically valid `oo_r_` role that matches `identity.role`
//! binds the session before the guard and before audit admission. An invalid
//! role does not bind. A different valid role is `RoleReuse` even when the
//! first request was refused by the guard or by admission. The `pg_roles`
//! flag check still runs later, inside the read-only transaction, before the
//! user statement.
//!
//! `Transaction`'s destructor requests a rollback through a private driver
//! call. That request is not a confirmed rollback. Success awaits `rollback`.
//! Dropping the stream sets shutdown. A call that has not been polled is not
//! started. A call that is already polled is cancelled on the bound backend.
//! Observe, cancel connect, TLS, the cancel send, the pinned response, and
//! rollback share one 750 ms deadline taken at the first cleanup action. A
//! later phase uses only the time still left and does not start another 750 ms.
//! A consumed answer is rolled back inside that remainder. A client deadline
//! spends the same budget on the cancel transport, then drops the unread future
//! and does not claim a rollback. If the budget expires, or the cancel transport
//! fails, the dedicated session is destroyed and rollback is not claimed.
//! [`JoinHandle::abort`] when the session is dropped stops the local producer
//! and driver. That abort is not `cancel_query` and does not prove the backend
//! stopped.
//! A [`CancelHandle`] reserves the accepting generation before its transport.
//! The session is not lent while that reservation stands. The future that
//! reserved owns the reservation. The handle is cloned and its destructor does
//! not clear it. Dropping that future before `cancel_query` releases only its
//! generation. Dropping it after the transport was entered fail-closes the
//! session on that thread, with no further await. A late handle carries the
//! generation captured when the query started. The next query bumps that
//! generation, so the old handle does not call `cancel_query`.
//!
//! The audit trait must return before the transaction begins. The hook in the
//! live fixture writes through a second client. That callback is not a
//! production audit WAL. Guard refusal does not call the hook. If the stream
//! is aborted, completion is `partial` or `failed`, never `succeeded`.
//!
//! Result objects are built here. Another crate cannot construct
//! [`QuerySummary`]. Cell text is the driver's typed value for int2, int4,
//! int8, text, bool, and null. Numeric OID 1700 is an [`ExactDecimal`].
//! `varchar`, `bpchar`, `float8`, `json`, `jsonb`, `bytea`, `uuid`, `date`,
//! `timestamp`, and `timestamptz` are canonical text in [`Cell::Text`].
//! `float4` and every other result OID are refused before the portal runs.
//! This module still has no authentication gate. [`UNTRUSTED_ADMISSION`]
//! stays blocked. A caller that already authenticated may pass
//! [`SelectRequest::read_quality`] to read fixed `_oo` quality relations in
//! the same transaction, after the portal and before rollback.

use std::error::Error as StdError;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use postgres_types::FromSql;
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_postgres::types::Type;
use tokio_postgres::{Client, Error as PgError, IsolationLevel, Row, Transaction};

use crate::policy::TlsConnection;
use crate::sql_guard::{GuardError, GuardedPlan, PolicyCatalog, PolicyIdentity, guard_select};
use oo_core::api::ErrorCode;
use oo_core::limits::CANONICAL_MAX_COEFFICIENT_DIGITS;
use oo_core::{DecimalError, ExactDecimal};

/// Largest accepted whole-query budget. One deadline, not a per-page reset.
pub const MAX_QUERY_WALL: Duration = Duration::from_secs(30);

/// Largest `max_rows` this slice accepts.
pub const MAX_QUERY_ROWS: u32 = 2_000;

/// Largest `max_result_bytes` this slice accepts.
pub const MAX_QUERY_BYTES: u64 = 256 * 1024;

/// Portal page size. Zero would fetch every row, so it is rejected.
pub const PORTAL_PAGE_ROWS: i32 = 1;

const _: () = assert!(PORTAL_PAGE_ROWS > 0);

const OID_BOOL: u32 = 16;
const OID_BYTEA: u32 = 17;
const OID_INT8: u32 = 20;
const OID_INT2: u32 = 21;
const OID_INT4: u32 = 23;
const OID_TEXT: u32 = 25;
const OID_JSON: u32 = 114;
const OID_FLOAT8: u32 = 701;
const OID_BPCHAR: u32 = 1042;
const OID_VARCHAR: u32 = 1043;
const OID_DATE: u32 = 1082;
const OID_TIME: u32 = 1083;
const OID_TIMESTAMP: u32 = 1114;
const OID_TIMESTAMPTZ: u32 = 1184;
const OID_INTERVAL: u32 = 1186;
const OID_NUMERIC: u32 = 1700;
const OID_UUID: u32 = 2950;
const OID_JSONB: u32 = 3802;
/// Microseconds from 1970-01-01 to 2000-01-01.
const PG_EPOCH_UNIX_US: i64 = 946_684_800_000_000;
/// Days from 1970-01-01 to 2000-01-01.
const PG_EPOCH_UNIX_DAYS: i64 = 10_957;
const NUMERIC_SIGN_POS: u16 = 0x0000;
const NUMERIC_SIGN_NEG: u16 = 0x4000;
const NUMERIC_SIGN_NAN: u16 = 0xC000;
const NUMERIC_SIGN_PINF: u16 = 0xD000;
const NUMERIC_SIGN_NINF: u16 = 0xF000;
const TIMEOUT_MARGIN: Duration = Duration::from_millis(250);

/// One cleanup deadline shared by observe, cancel transport, the pinned
/// response, and rollback. Later phases use only the time still left.
/// This is not a per-page query budget and does not refresh the whole-query
/// [`Instant`].
const CLEANUP_BOUND: Duration = Duration::from_millis(750);

/// Why an untrusted statement cannot be admitted by this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UntrustedAdmission {
    /// The parser worker's kernel sandbox is unsupported, and there is no authentication gate.
    Blocked,
}

/// Untrusted admission stays blocked while the kernel sandbox is unsupported and no authentication gate exists.
pub const UNTRUSTED_ADMISSION: UntrustedAdmission = UntrustedAdmission::Blocked;

/// One dedicated TLS session. Not a pool.
pub struct DedicatedSession {
    client: Option<Client>,
    tls: tokio_postgres_rustls::MakeRustlsConnect,
    token: tokio_postgres::CancelToken,
    shared: Arc<Mutex<Shared>>,
    driver: JoinHandle<()>,
    task: Option<JoinHandle<()>>,
    client_back: Option<oneshot::Receiver<Option<Client>>>,
}

/// Caller input. `original_sql` is the only SQL. The guard produces executed SQL.
pub struct SelectRequest<'a> {
    /// Trusted adapter identity. `role` must equal [`Self::role`].
    pub identity: &'a PolicyIdentity,
    /// Trusted catalog snapshot passed to the guard.
    pub catalog: &'a PolicyCatalog,
    /// Original statement. It is guarded, then the plan's executed SQL is run.
    pub original_sql: &'a str,
    /// Exact `NOLOGIN` role. Must equal `identity.role`.
    pub role: &'a str,
    /// Rows returned to the caller. `max_rows + 1` may be fetched.
    pub max_rows: u32,
    /// Canonical result bytes. A row that would exceed this is not appended.
    pub max_result_bytes: u64,
    /// Whole-query budget, from 1 ms through [`MAX_QUERY_WALL`], in whole milliseconds.
    pub wall_clock: Duration,
    /// When true, read fixed `_oo` quality relations in this transaction.
    /// Absence is recorded and does not fail a finished read.
    pub read_quality: bool,
    /// Exact reviewed collection vector for source-scoped quality rows.
    pub quality_vector: Option<&'a serde_json::Value>,
}

/// Trusted hook. `admit` must succeed before the query transaction starts.
pub trait AuditAdmission: Send {
    /// Durable admission on the caller's own client. Failure skips the query.
    fn admit(
        &mut self,
        admission: &Admission<'_>,
    ) -> impl Future<Output = Result<(), ExecError>> + Send;

    /// Completion notice. A failure does not rewrite a non-success status into success.
    fn complete(
        &mut self,
        completion: &CompletionRecord,
    ) -> impl Future<Output = Result<(), ExecError>> + Send;
}

/// What the executor tells the audit hook before `BEGIN`. No SQL text.
pub struct Admission<'a> {
    original_sha256: &'a str,
    executed_sha256: &'a str,
    fingerprint_hex: &'a str,
    role: &'a str,
    project: &'a str,
    policy_id: &'a str,
    backend_pid: i32,
    backend_start: &'a str,
}

impl Admission<'_> {
    pub fn original_sha256(&self) -> &str {
        self.original_sha256
    }
    pub fn executed_sha256(&self) -> &str {
        self.executed_sha256
    }
    pub fn fingerprint_hex(&self) -> &str {
        self.fingerprint_hex
    }
    pub fn role(&self) -> &str {
        self.role
    }
    pub fn project(&self) -> &str {
        self.project
    }
    pub fn policy_id(&self) -> &str {
        self.policy_id
    }
    pub fn backend_pid(&self) -> i32 {
        self.backend_pid
    }
    pub fn backend_start(&self) -> &str {
        self.backend_start
    }
}

/// What the executor observed when the stream ended. No SQL text.
pub struct CompletionRecord {
    status: CompletionStatus,
    truncated: bool,
    returned_rows: u32,
    result_bytes: u64,
    rollback_confirmed: bool,
    socket_dropped: bool,
    stop: Option<StopClass>,
    sqlstate: Option<String>,
    role: String,
    original_sha256: String,
    executed_sha256: String,
    fingerprint_hex: String,
    project: String,
    policy_id: String,
    backend_pid: i32,
    backend_start: String,
}

impl CompletionRecord {
    pub fn status(&self) -> CompletionStatus {
        self.status
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
    pub fn returned_rows(&self) -> u32 {
        self.returned_rows
    }
    pub fn result_bytes(&self) -> u64 {
        self.result_bytes
    }
    pub fn rollback_confirmed(&self) -> bool {
        self.rollback_confirmed
    }
    pub fn socket_dropped(&self) -> bool {
        self.socket_dropped
    }
    pub fn stop(&self) -> Option<StopClass> {
        self.stop
    }
    pub fn sqlstate(&self) -> Option<&str> {
        self.sqlstate.as_deref()
    }
    pub fn role(&self) -> &str {
        &self.role
    }
    pub fn original_sha256(&self) -> &str {
        &self.original_sha256
    }
    pub fn executed_sha256(&self) -> &str {
        &self.executed_sha256
    }
    pub fn fingerprint_hex(&self) -> &str {
        &self.fingerprint_hex
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn policy_id(&self) -> &str {
        &self.policy_id
    }
    pub fn backend_pid(&self) -> i32 {
        self.backend_pid
    }
    pub fn backend_start(&self) -> &str {
        &self.backend_start
    }
}

/// Stream of executor-decoded rows. Dropping it aborts an unread query.
pub struct SelectStream {
    rows: mpsc::Receiver<StreamItem>,
    shutdown: watch::Sender<bool>,
    shared: Arc<Mutex<Shared>>,
    cancel: CancelHandle,
}

/// Handle for the generation captured when this query became cancellable.
#[derive(Clone)]
pub struct CancelHandle {
    shared: Arc<Mutex<Shared>>,
    generation: u64,
    token: tokio_postgres::CancelToken,
    tls: tokio_postgres_rustls::MakeRustlsConnect,
    pid: i32,
}

/// Whether `cancel_query` was invoked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelOutcome {
    /// The gate passed and the cancel request was written.
    Sent,
    /// The gate refused. The server was not asked to cancel.
    NotSent,
}

/// Gate result for one observed backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelGate {
    /// Generation, acceptance, and backend identity all match.
    Send,
    /// The handle must not call `cancel_query`.
    Refuse,
}

/// One backend row from `pg_stat_activity`.
pub struct BackendView {
    pid: i32,
    backend_start: String,
    state: String,
}

impl BackendView {
    pub fn pid(&self) -> i32 {
        self.pid
    }
    pub fn backend_start(&self) -> &str {
        &self.backend_start
    }
    pub fn state(&self) -> &str {
        &self.state
    }
}

/// How a query stopped when it did not succeed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopClass {
    /// Client deadline or server `statement_timeout`.
    Timeout,
    /// Cancel requested for this generation.
    Canceled,
    /// Result OID is outside the initial typed set.
    TypeRefused,
    /// PostgreSQL completed an error response.
    Database,
    /// The connection could not be kept in a known state.
    Protocol,
}

/// Executor-built completion. Not constructible outside this crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuerySummary {
    status: CompletionStatus,
    truncated: bool,
    returned_rows: u32,
    result_bytes: u64,
    rollback_confirmed: bool,
    socket_dropped: bool,
    stop: Option<StopClass>,
    sqlstate: Option<String>,
    end_audit_ok: bool,
}

impl QuerySummary {
    pub fn status(&self) -> CompletionStatus {
        self.status
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
    pub fn returned_rows(&self) -> u32 {
        self.returned_rows
    }
    pub fn result_bytes(&self) -> u64 {
        self.result_bytes
    }
    pub fn rollback_confirmed(&self) -> bool {
        self.rollback_confirmed
    }
    pub fn socket_dropped(&self) -> bool {
        self.socket_dropped
    }
    pub fn stop(&self) -> Option<StopClass> {
        self.stop
    }
    pub fn sqlstate(&self) -> Option<&str> {
        self.sqlstate.as_deref()
    }
    pub fn end_audit_ok(&self) -> bool {
        self.end_audit_ok
    }
}

/// Final stream state. Aborted reads are never [`CompletionStatus::Succeeded`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionStatus {
    /// Rollback was confirmed and the bound backend was still the same one.
    Succeeded,
    /// The consumer dropped the stream before a clean end.
    Partial,
    /// The query did not finish as a confirmed read.
    Failed,
}

impl CompletionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }
}

/// One prepared result column. Names and OIDs come from the statement, not the client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultColumn {
    /// PostgreSQL result name.
    pub name: String,
    /// Result type OID.
    pub oid: u32,
    /// Result typmod. `-1` is unconstrained.
    pub typmod: i32,
}

/// One logical table the quality read may describe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityTable {
    /// Source authenticated by the native collection, when available.
    pub source: Option<String>,
    /// Logical schema.
    pub schema: String,
    /// Logical relation.
    pub name: String,
    /// Physical schema written by the guard.
    pub physical_schema: String,
    /// Physical relation written by the guard.
    pub physical_name: String,
}

/// What the same-snapshot quality read found. Empty strings mean the value was absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualitySnapshot {
    /// `skipped`, `absent`, `unreadable`, or `read`.
    ///
    /// `read` means the catalog probe ran. Each [`RelationProbe`] says whether
    /// that relation was absent, not readable, or read. An empty freshness list
    /// is not a claim that the source is ready.
    pub relations: &'static str,
    /// Rows whose names matched the guarded relations.
    pub freshness: Vec<FreshnessObserved>,
    /// Status warnings. Counts and hidden names are not copied.
    pub warnings: Vec<QualityWarning>,
    /// Table mappings visible to this role. Physical names are not stored.
    pub mappings: Vec<MappingObserved>,
    /// One state per probed relation.
    pub probes: Vec<RelationProbe>,
    /// Watermark rows whose source is one of `mappings`. Empty when unread.
    pub watermarks: Vec<WatermarkObserved>,
    /// Receipt rows matched to a visible mapping. Physical names are not stored.
    pub receipts: Vec<ReceiptObserved>,
    /// Import runs whose source is visible to this role.
    pub runs: Vec<ImportRunObserved>,
    /// Import tables matched to a visible mapping.
    pub imports: Vec<ImportTableObserved>,
    /// Quality totals for visible physical tables only.
    pub quality_counts: Vec<QualityCount>,
    /// Capture method from a readable quality row, or empty when unread.
    pub capture_method: String,
    /// Delete detection from a readable quality row, or empty when unread.
    pub delete_detection: String,
}

/// One logical mapping the login could read. `bound` compares the catalog's
/// physical relation and then drops that name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingObserved {
    /// Source identity from `_oo.physical_mappings`.
    pub source: String,
    /// Logical schema.
    pub schema: String,
    /// Logical table.
    pub table: String,
    /// Mapping `run_generation`.
    pub run_generation: i64,
    /// True when this row is the catalog's bound generation.
    pub bound: bool,
}

/// Observed state of one `_oo` relation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelationProbe {
    /// Relation name. One of the fixed probe list.
    pub name: &'static str,
    /// `absent`, `not_readable`, or `read`.
    pub state: &'static str,
}

/// One source watermark row filtered to a visible mapping source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatermarkObserved {
    /// Source identity.
    pub source: String,
    /// `transactional` or `per_table` text from the row.
    pub consistency: String,
    /// Cross-table flag from the row.
    pub cross_table_consistent: bool,
    /// Exact `applied_pos` text. Not a float.
    pub applied_pos: String,
    /// State text from the row.
    pub state: String,
    /// `position_codec` text. MariaDB is admitted only with its lineage.
    pub codec: String,
    /// `position_lineage` text. A MariaDB domain or server outside this value does not pass.
    pub lineage: String,
}

/// One apply receipt matched to a visible logical table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptObserved {
    /// Source identity.
    pub source: String,
    /// Logical schema.
    pub schema: String,
    /// Logical table.
    pub table: String,
    /// Mapping generation that matched the receipt's physical generation.
    pub run_generation: i64,
    /// Exact applied position text.
    pub applied_pos: String,
    /// Position codec text from the row.
    pub codec: String,
    /// `position_lineage` text from the receipt.
    pub lineage: String,
    /// `source_epoch` from the receipt. MariaDB admits one epoch for the watermark lineage.
    pub source_epoch: i32,
    /// Cross-table flag from the row.
    pub cross_table_consistent: bool,
    /// Partial transaction flag from the row.
    pub partial_txn: bool,
    /// `target_commit_at` rendered in UTC, or empty.
    pub target_commit_at: String,
}

/// One full-import run for a visible source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportRunObserved {
    /// Source identity.
    pub source: String,
    /// Source generation.
    pub source_generation: i64,
    /// Target generation.
    pub target_generation: i64,
    /// Run state. `awaiting_publish` is not ready.
    pub state: String,
    /// True when `sealed_at` is present.
    pub sealed: bool,
}

/// One import table row matched to a visible mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportTableObserved {
    /// Logical schema.
    pub schema: String,
    /// Logical table.
    pub table: String,
    /// Mapping generation.
    pub run_generation: i64,
    /// Applied row count from the import table.
    pub applied_rows: i64,
    /// Exact applied position text, or empty.
    pub applied_pos: String,
    /// Exhausted flag from the row.
    pub exhausted: bool,
}

/// Masked quality totals for the role's physical tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityCount {
    /// `batch` or `reject`.
    pub kind: String,
    /// Sum of `rejected_rows`.
    pub rejected_rows: i64,
    /// Sum of `accepted_rows`.
    pub accepted_rows: i64,
    /// Number of quality rows in the sum.
    pub batches: i64,
}

/// Raw freshness text from `_oo.table_freshness`. Not a client timestamp yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreshnessObserved {
    /// Logical schema when the row matched one.
    pub schema: String,
    /// Logical table when the row matched one.
    pub table: String,
    /// `state` text from the relation.
    pub state: String,
    /// `source_committed_at` rendered in UTC, or empty.
    pub source_committed_at: String,
    /// `applied_at` rendered in UTC, or empty.
    pub applied_at: String,
}

/// One quality status. `code` is a static warning name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityWarning {
    /// Static code such as `RECONCILE_MISMATCH`.
    pub code: &'static str,
    /// Logical schema.
    pub schema: String,
    /// Logical table.
    pub table: String,
    /// Message. No counts and no hidden column names.
    pub message: String,
}

impl QualitySnapshot {
    pub(crate) fn blank(relations: &'static str) -> Self {
        Self {
            relations,
            freshness: Vec::new(),
            warnings: Vec::new(),
            mappings: Vec::new(),
            probes: Vec::new(),
            watermarks: Vec::new(),
            receipts: Vec::new(),
            runs: Vec::new(),
            imports: Vec::new(),
            quality_counts: Vec::new(),
            capture_method: String::new(),
            delete_detection: String::new(),
        }
    }

    fn skipped() -> Self {
        Self::blank("skipped")
    }
}

/// One decoded cell. Integers are canonical decimal text from the typed value.
/// Numeric is an exact decimal, not a float and not driver text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    /// SQL NULL.
    Null,
    /// `bool`.
    Bool(bool),
    /// int2, int4, or int8 rendered with `Display`.
    Int(String),
    /// `text`, `varchar`, `bpchar`, or another allowlisted type rendered as text.
    Text(String),
    /// PostgreSQL `numeric` as an exact decimal. The coefficient has no exponent.
    Decimal(ExactDecimal),
}

/// Failure from the executor. Display text has no SQL and no password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecError {
    /// The guard refused the original SQL. The audit hook was not called.
    Guard(Box<GuardError>),
    /// `admit` failed. The query transaction was not started.
    AuditUnavailable,
    /// The role string is not the exact `oo_r_<slug>` NOLOGIN shape.
    RoleParameter,
    /// This session already bound a different syntactically valid role.
    RoleReuse,
    /// A limit is outside the slice budget or is not a whole millisecond count.
    Limit,
    /// The prepared result OID is not in the initial set.
    TypeRefused {
        /// PostgreSQL type OID.
        oid: u32,
    },
    /// Deadline. `sqlstate` is present when the server completed `57014`.
    Timeout {
        /// SQLSTATE, when the server sent one.
        sqlstate: Option<String>,
    },
    /// This generation was cancelled. The API code is still timeout.
    Canceled {
        /// SQLSTATE from the server. This path expects `57014`.
        sqlstate: String,
    },
    /// Server error. The message is discarded because a `42501` can name a column.
    Database {
        /// SQLSTATE only.
        sqlstate: String,
    },
    /// The dedicated session is already closed.
    SessionClosed,
    /// A previous stream has not finished. Read it, drop it, or `wait_idle`.
    SessionBusy,
    /// Driver or session invariant failed. `step` is a static label.
    Protocol {
        /// Static step name. Not SQL text.
        step: &'static str,
    },
}

impl ExecError {
    /// Existing API code. Cancel has no separate code, so it uses timeout.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Guard(err) => err.code(),
            Self::AuditUnavailable => ErrorCode::AuditUnavailable,
            Self::Timeout { .. } | Self::Canceled { .. } => ErrorCode::Timeout,
            Self::TypeRefused { .. } => ErrorCode::StatementNotAllowed,
            Self::RoleParameter
            | Self::RoleReuse
            | Self::Limit
            | Self::SessionClosed
            | Self::SessionBusy => ErrorCode::SchemaInvalid,
            Self::Database { .. } | Self::Protocol { .. } => ErrorCode::Internal,
        }
    }

    pub fn guard_error(&self) -> Option<&GuardError> {
        match self {
            Self::Guard(err) => Some(err),
            _ => None,
        }
    }

    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            Self::Timeout { sqlstate } => sqlstate.as_deref(),
            Self::Canceled { sqlstate } | Self::Database { sqlstate } => Some(sqlstate),
            _ => None,
        }
    }
}

impl fmt::Display for ExecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Guard(err) => write!(formatter, "{err}"),
            Self::AuditUnavailable => formatter.write_str("audit admission failed"),
            Self::RoleParameter => formatter.write_str("role parameter is not the nologin shape"),
            Self::RoleReuse => formatter.write_str("dedicated session cannot change role"),
            Self::Limit => formatter.write_str("query limit is outside the slice budget"),
            Self::TypeRefused { oid } => write!(formatter, "result type oid {oid} is refused"),
            Self::Timeout { sqlstate } => match sqlstate {
                Some(code) => write!(formatter, "query deadline exceeded sqlstate={code}"),
                None => formatter.write_str("query deadline exceeded"),
            },
            Self::Canceled { sqlstate } => write!(formatter, "query canceled sqlstate={sqlstate}"),
            Self::Database { sqlstate } => write!(formatter, "postgres sqlstate={sqlstate}"),
            Self::SessionClosed => formatter.write_str("dedicated session is closed"),
            Self::SessionBusy => formatter.write_str("dedicated session is still reading"),
            Self::Protocol { step } => write!(formatter, "session protocol failed at {step}"),
        }
    }
}

impl std::error::Error for ExecError {}

struct Shared {
    generation: u64,
    accepting_cancel: bool,
    backend_pid: i32,
    backend_start: String,
    bound_role: Option<String>,
    open: bool,
    summary: Option<QuerySummary>,
    /// Generation whose cancel transport has not finished. Blocks the next lend.
    cancel_reserved: Option<u64>,
    /// A cancel request was written. The same connection must not be lent.
    cancel_packet_sent: bool,
    /// First cleanup action. Later phases do not add another 750 ms.
    cleanup_deadline: Option<Instant>,
    cancel_notify: Arc<Notify>,
    destroy: watch::Sender<bool>,
    result_columns: Vec<ResultColumn>,
    quality: QualitySnapshot,
}

enum StreamItem {
    Row(Vec<Cell>),
    Failed(ExecError),
}

struct EndGeneration<'a> {
    shared: &'a Mutex<Shared>,
    armed: bool,
}

impl EndGeneration<'_> {
    fn arm(shared: &Mutex<Shared>) -> EndGeneration<'_> {
        EndGeneration {
            shared,
            armed: true,
        }
    }

    fn close_generation(&mut self) {
        if self.armed {
            end_generation(self.shared);
            self.armed = false;
        }
    }
}

impl Drop for EndGeneration<'_> {
    fn drop(&mut self) {
        self.close_generation();
    }
}

struct Attempt {
    status: CompletionStatus,
    truncated: bool,
    returned_rows: u32,
    result_bytes: u64,
    rollback_confirmed: bool,
    socket_dropped: bool,
    stop: Option<StopClass>,
    sqlstate: Option<String>,
    reuse: bool,
}

impl DedicatedSession {
    /// Bind one already-connected TLS client. The driver is spawned here.
    ///
    /// `tls` is the connector used later to open a cancel channel. The backend
    /// pid and `backend_start` are read before any user statement and before
    /// any cancel.
    pub async fn bind(
        client: Client,
        connection: TlsConnection,
        tls: tokio_postgres_rustls::MakeRustlsConnect,
    ) -> Result<Self, ExecError> {
        let token = client.cancel_token();
        let driver = tokio::spawn(async move {
            let _ = connection.await;
        });
        let bound = match read_backend_binding(&client).await {
            Ok(bound) => bound,
            Err(err) => {
                drop(client);
                driver.abort();
                return Err(err);
            }
        };
        let (destroy, _destroy_rx) = watch::channel(false);
        let shared = Arc::new(Mutex::new(Shared {
            generation: 0,
            accepting_cancel: false,
            backend_pid: bound.0,
            backend_start: bound.1,
            bound_role: None,
            open: true,
            summary: None,
            cancel_reserved: None,
            cancel_packet_sent: false,
            cleanup_deadline: None,
            cancel_notify: Arc::new(Notify::new()),
            destroy,
            result_columns: Vec::new(),
            quality: QualitySnapshot::skipped(),
        }));
        Ok(Self {
            client: Some(client),
            tls,
            token,
            shared,
            driver,
            task: None,
            client_back: None,
        })
    }

    /// Bind a valid role, guard the original SQL, require admission, then start the portal.
    ///
    /// `wait_idle` while this stream is unread and still held deadlocks: the
    /// producer blocks on a one-row channel. Read the stream or drop it first.
    pub async fn execute_trusted_select<A>(
        &mut self,
        request: SelectRequest<'_>,
        mut audit: A,
    ) -> Result<SelectStream, ExecError>
    where
        A: AuditAdmission + Send + 'static,
    {
        let started = Instant::now();
        self.reap_finished().await?;
        {
            let guard = lock(&self.shared);
            if guard.cancel_reserved.is_some() {
                return Err(ExecError::SessionBusy);
            }
            if !guard.open || self.client.is_none() {
                return Err(ExecError::SessionClosed);
            }
        }
        let wall = checked_wall(request.wall_clock)?;
        if request.max_rows > MAX_QUERY_ROWS || request.max_result_bytes > MAX_QUERY_BYTES {
            return Err(ExecError::Limit);
        }
        {
            let mut guard = lock(&self.shared);
            match role_bind_decision(
                guard.bound_role.as_deref(),
                request.role,
                &request.identity.role,
            ) {
                RoleBind::Invalid => return Err(ExecError::RoleParameter),
                RoleBind::Reuse => return Err(ExecError::RoleReuse),
                RoleBind::Proceed => {
                    if guard.bound_role.is_none() {
                        guard.bound_role = Some(request.role.to_string());
                    }
                }
            }
        }
        let plan = guard_select(request.identity, request.catalog, request.original_sql)
            .map_err(|err| ExecError::Guard(Box::new(err)))?;
        if remaining(started, wall).is_none() {
            return Err(ExecError::Timeout { sqlstate: None });
        }
        let (pid, backend_start) = {
            let guard = lock(&self.shared);
            (guard.backend_pid, guard.backend_start.clone())
        };
        let admission = Admission {
            original_sha256: &plan.original_sha256,
            executed_sha256: &plan.executed_sha256,
            fingerprint_hex: &plan.fingerprint_hex,
            role: request.role,
            project: &request.identity.project,
            policy_id: &request.identity.policy_id,
            backend_pid: pid,
            backend_start: &backend_start,
        };
        audit.admit(&admission).await?;
        let executed_sql = plan.executed_sql.clone();
        let audit_ids = AuditIds::from_plan(&plan, &request);
        let generation = begin_generation(&self.shared);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (row_tx, row_rx) = mpsc::channel(1);
        let (back_tx, back_rx) = oneshot::channel();
        let client = self.client.take().ok_or(ExecError::SessionClosed)?;
        let shared = Arc::clone(&self.shared);
        let token = self.token.clone();
        let tls = self.tls.clone();
        let destroy_rx = lock(&self.shared).destroy.subscribe();
        let tables = plan
            .bindings
            .relations
            .iter()
            .map(|relation| QualityTable {
                source: request.quality_vector.and_then(serde_json::Value::as_array).and_then(|items| {
                    items.iter().find(|item| item["dataset"] == format!("{}.{}", relation.schema, relation.name))
                }).and_then(|item| item["source_id"].as_str()).map(str::to_owned),
                schema: relation.schema.clone(),
                name: relation.name.clone(),
                physical_schema: relation.physical_schema.clone(),
                physical_name: relation.physical_name.clone(),
            })
            .collect();
        let limits = Limits {
            max_rows: request.max_rows,
            max_result_bytes: request.max_result_bytes,
            wall,
            read_quality: request.read_quality,
            tables,
        };
        let task = tokio::spawn(async move {
            run_producer(
                client,
                executed_sql,
                audit_ids,
                limits,
                started,
                token,
                tls,
                shared,
                audit,
                shutdown_rx,
                destroy_rx,
                row_tx,
                back_tx,
            )
            .await;
        });
        self.task = Some(task);
        self.client_back = Some(back_rx);
        let cancel = CancelHandle {
            shared: Arc::clone(&self.shared),
            generation,
            token: self.token.clone(),
            tls: self.tls.clone(),
            pid,
        };
        Ok(SelectStream {
            rows: row_rx,
            shutdown: shutdown_tx,
            shared: Arc::clone(&self.shared),
            cancel,
        })
    }

    /// Wait until the producer exits and the client slot is settled.
    ///
    /// Drop or finish the stream first. An unread stream can fill the one-row
    /// channel and stall this wait.
    pub async fn wait_idle(&mut self) -> Result<(), ExecError> {
        self.join_task().await
    }

    /// Refuse later cancels and stop the local producer and driver.
    ///
    /// Abort drops an in-flight cancel future owned by this session. That is
    /// local teardown. It does not prove the backend stopped, and it is not
    /// `cancel_query`. This does not claim a rollback.
    pub fn close(&mut self) {
        self.shutdown_local();
    }

    fn shutdown_local(&mut self) {
        let notify = {
            let mut guard = lock(&self.shared);
            guard.accepting_cancel = false;
            guard.open = false;
            guard.cancel_reserved = None;
            let _ = guard.destroy.send(true);
            Arc::clone(&guard.cancel_notify)
        };
        notify.notify_waiters();
        self.client.take();
        if let Some(task) = self.task.as_ref() {
            task.abort();
        }
        self.driver.abort();
    }

    pub fn cancel_handle(&self) -> Option<CancelHandle> {
        let guard = lock(&self.shared);
        if !guard.accepting_cancel {
            return None;
        }
        Some(CancelHandle {
            shared: Arc::clone(&self.shared),
            generation: guard.generation,
            token: self.token.clone(),
            tls: self.tls.clone(),
            pid: guard.backend_pid,
        })
    }

    pub fn backend_pid(&self) -> i32 {
        lock(&self.shared).backend_pid
    }

    pub fn backend_start(&self) -> String {
        lock(&self.shared).backend_start.clone()
    }

    pub fn generation(&self) -> u64 {
        lock(&self.shared).generation
    }

    pub fn is_open(&self) -> bool {
        lock(&self.shared).open && (self.client.is_some() || self.task.is_some())
    }

    pub fn bound_role(&self) -> Option<String> {
        lock(&self.shared).bound_role.clone()
    }

    pub fn summary(&self) -> Option<QuerySummary> {
        lock(&self.shared).summary.clone()
    }

    async fn reap_finished(&mut self) -> Result<(), ExecError> {
        let running = self.task.as_ref().is_some_and(|task| !task.is_finished());
        if running {
            return Err(ExecError::SessionBusy);
        }
        self.join_task().await
    }

    async fn join_task(&mut self) -> Result<(), ExecError> {
        let joined = if let Some(task) = self.task.take() {
            task.await
        } else {
            Ok(())
        };
        if let Some(incoming) = self.client_back.take() {
            match incoming.await {
                Ok(Some(client)) => self.client = Some(client),
                Ok(None) | Err(_) => {
                    self.client = None;
                    lock(&self.shared).open = false;
                }
            }
        }
        joined.map_err(|_| ExecError::Protocol { step: "task" })
    }
}

impl Drop for DedicatedSession {
    fn drop(&mut self) {
        self.shutdown_local();
    }
}

impl SelectStream {
    /// Next decoded row. `Ok(None)` is a clean end. Errors are executor failures.
    pub async fn next_row(&mut self) -> Result<Option<Vec<Cell>>, ExecError> {
        match self.rows.recv().await {
            Some(StreamItem::Row(cells)) => Ok(Some(cells)),
            Some(StreamItem::Failed(err)) => Err(err),
            None => Ok(None),
        }
    }

    pub fn summary(&self) -> Option<QuerySummary> {
        lock(&self.shared).summary.clone()
    }

    pub fn cancel_handle(&self) -> CancelHandle {
        self.cancel.clone()
    }

    /// Result columns stored when the statement was prepared.
    pub fn result_columns(&self) -> Vec<ResultColumn> {
        lock(&self.shared).result_columns.clone()
    }

    /// Quality snapshot stored before rollback. `skipped` when the request did not ask.
    pub fn quality(&self) -> QualitySnapshot {
        lock(&self.shared).quality.clone()
    }
}

impl Drop for SelectStream {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

impl CancelHandle {
    /// Cancel only when the observed backend is still this generation.
    ///
    /// The observer must be a different session. The query connection is busy.
    /// A matching accepting generation is reserved before any await, so the
    /// producer cannot lend that connection while this transport is in flight.
    /// The future that reserved owns that reservation through [`ReservationGuard`].
    /// This handle is [`Clone`] and has no [`Drop`]. Dropping the future before
    /// `cancel_query` releases only its generation. Dropping it after the
    /// transport is entered fail-closes the session with no await in the
    /// destructor. The gate is checked after observation returns. There is no
    /// await between that check and `cancel_query`. Observe and `cancel_query`
    /// share one cleanup deadline. Timeout or a transport error fail-closes
    /// the session. A handle that does not own the reservation does not
    /// release it and does not fail-close it.
    pub async fn cancel(&self, observer: &Client) -> Result<CancelOutcome, ExecError> {
        let owned = try_reserve_cancel(&self.shared, self.generation);
        let mut owner = if owned {
            Some(ReservationGuard::arm(
                Arc::clone(&self.shared),
                self.generation,
            ))
        } else {
            None
        };
        let budget = if owned {
            shared_cleanup_remaining(&self.shared)
        } else {
            CLEANUP_BOUND
        };
        let observed = match observe_within(observer, self.pid, budget).await {
            Ok(view) => view,
            Err(err) => {
                if let Some(guard) = owner.as_mut() {
                    guard.fail_close_on_drop();
                }
                return Err(err);
            }
        };
        let send = {
            let guard = lock(&self.shared);
            cancel_gate(
                self.generation,
                guard.generation,
                guard.accepting_cancel,
                guard.backend_pid,
                &guard.backend_start,
                observed.as_ref(),
            ) == CancelGate::Send
        };
        if !owned || !send {
            return Ok(CancelOutcome::NotSent);
        }
        let remain = shared_cleanup_remaining(&self.shared);
        if remain.is_zero() {
            if let Some(guard) = owner.as_mut() {
                guard.fail_close_on_drop();
            }
            return Err(ExecError::Timeout { sqlstate: None });
        }
        if let Some(guard) = owner.as_mut() {
            guard.enter_transport();
        }
        let tls = self.tls.clone();
        match tokio::time::timeout(remain, self.token.cancel_query(tls)).await {
            Ok(Ok(())) => {
                if let Some(guard) = owner.as_mut() {
                    guard.disarm();
                }
                mark_cancel_packet(&self.shared);
                Ok(CancelOutcome::Sent)
            }
            Ok(Err(_)) => Err(ExecError::Protocol { step: "cancel" }),
            Err(_) => Err(ExecError::Timeout { sqlstate: None }),
        }
    }
}

/// Decide whether a handle may send `cancel_query`.
pub fn cancel_gate(
    handle_generation: u64,
    current_generation: u64,
    accepting: bool,
    bound_pid: i32,
    bound_start: &str,
    observed: Option<&BackendView>,
) -> CancelGate {
    let Some(view) = observed else {
        return CancelGate::Refuse;
    };
    if !accepting
        || handle_generation != current_generation
        || bound_pid <= 0
        || bound_start.is_empty()
        || view.pid != bound_pid
        || view.backend_start != bound_start
    {
        return CancelGate::Refuse;
    }
    CancelGate::Send
}

/// Read one backend from `pg_stat_activity`. `Ok(None)` means that pid is gone.
pub async fn observe_backend(client: &Client, pid: i32) -> Result<Option<BackendView>, ExecError> {
    let rows = client
        .query(
            "SELECT pid, COALESCE(to_char(backend_start AT TIME ZONE 'UTC', 'YYYYMMDDHH24MISSUS'), ''), \
             COALESCE(state, '') \
             FROM pg_catalog.pg_stat_activity WHERE pid = $1",
            &[&pid],
        )
        .await
        .map_err(|_| ExecError::Protocol { step: "observe" })?;
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() != 1 {
        return Err(ExecError::Protocol { step: "observe" });
    }
    let view_pid: i32 = rows[0]
        .try_get(0)
        .map_err(|_| ExecError::Protocol { step: "observe" })?;
    let backend_start: String = rows[0]
        .try_get(1)
        .map_err(|_| ExecError::Protocol { step: "observe" })?;
    let state: String = rows[0]
        .try_get(2)
        .map_err(|_| ExecError::Protocol { step: "observe" })?;
    Ok(Some(BackendView {
        pid: view_pid,
        backend_start,
        state,
    }))
}

struct Limits {
    max_rows: u32,
    max_result_bytes: u64,
    wall: Duration,
    read_quality: bool,
    tables: Vec<QualityTable>,
}

struct AuditIds {
    role: String,
    original_sha256: String,
    executed_sha256: String,
    fingerprint_hex: String,
    project: String,
    policy_id: String,
    backend_pid: i32,
    backend_start: String,
}

impl AuditIds {
    fn from_plan(plan: &GuardedPlan, request: &SelectRequest<'_>) -> Self {
        Self {
            role: request.role.to_string(),
            original_sha256: plan.original_sha256.clone(),
            executed_sha256: plan.executed_sha256.clone(),
            fingerprint_hex: plan.fingerprint_hex.clone(),
            project: request.identity.project.clone(),
            policy_id: request.identity.policy_id.clone(),
            backend_pid: 0,
            backend_start: String::new(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_producer<A>(
    mut client: Client,
    executed_sql: String,
    mut audit_ids: AuditIds,
    limits: Limits,
    started: Instant,
    token: tokio_postgres::CancelToken,
    tls: tokio_postgres_rustls::MakeRustlsConnect,
    shared: Arc<Mutex<Shared>>,
    mut audit: A,
    mut shutdown: watch::Receiver<bool>,
    mut destroy: watch::Receiver<bool>,
    rows: mpsc::Sender<StreamItem>,
    back: oneshot::Sender<Option<Client>>,
) where
    A: AuditAdmission,
{
    let mut end = EndGeneration::arm(&shared);
    {
        let guard = lock(&shared);
        audit_ids.backend_pid = guard.backend_pid;
        audit_ids.backend_start = guard.backend_start.clone();
    }
    let attempt = drive(
        &mut client,
        &executed_sql,
        &audit_ids.role,
        &limits,
        started,
        &token,
        &tls,
        &shared,
        &mut shutdown,
        &mut destroy,
        &rows,
    )
    .await;
    let attempt = settle_lend(attempt, &shared).await;
    let returned = if attempt.reuse {
        Some(client)
    } else {
        drop(client);
        lock(&shared).open = false;
        None
    };
    let record = completion_from(&attempt, &audit_ids);
    store_summary(&shared, &attempt, false);
    end.close_generation();
    let end_ok = audit.complete(&record).await.is_ok();
    patch_audit(&shared, end_ok);
    let _ = back.send(returned);
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    client: &mut Client,
    executed_sql: &str,
    role: &str,
    limits: &Limits,
    started: Instant,
    token: &tokio_postgres::CancelToken,
    tls: &tokio_postgres_rustls::MakeRustlsConnect,
    shared: &Mutex<Shared>,
    shutdown: &mut watch::Receiver<bool>,
    destroy: &mut watch::Receiver<bool>,
    rows: &mpsc::Sender<StreamItem>,
) -> Attempt {
    let backend_start = lock(shared).backend_start.clone();
    let generation = lock(shared).generation;
    let pid = lock(shared).backend_pid;
    let mut hot = HotWait {
        shutdown,
        destroy,
        cancel: BoundCancel {
            shared,
            token,
            tls,
            generation,
            pid,
            backend_start: &backend_start,
        },
        started,
        wall: limits.wall,
        cleanup_deadline: None,
        cancel_attempted: false,
        deadline_cleanup: false,
    };
    let tx = match open_read_only(client, &mut hot).await {
        Ok(tx) => tx,
        Err(kind) => {
            if kind.desync && !hot.cancel_attempted {
                let _ = arm_and_send(&mut hot).await;
            }
            return publish_stop(rows, kind, false).await;
        }
    };
    if shutdown_requested(hot.shutdown) {
        return cleanup(tx, rows, consumer_stop(0, 0), &mut hot).await;
    }
    if let Err(kind) = apply_session(&tx, role, &mut hot).await {
        return cleanup(tx, rows, kind, &mut hot).await;
    }
    let statement = match prepare_user(&tx, executed_sql, &mut hot).await {
        Ok(statement) => statement,
        Err(kind) => return cleanup(tx, rows, kind, &mut hot).await,
    };
    if let Err(err) = allowed_result_types(&statement) {
        let kind = StopKind::synced(err);
        return cleanup(tx, rows, kind, &mut hot).await;
    }
    lock(shared).result_columns = statement
        .columns()
        .iter()
        .map(|column| ResultColumn {
            name: column.name().to_string(),
            oid: column.type_().oid(),
            typmod: column.type_modifier(),
        })
        .collect();
    if !statement.params().is_empty() {
        let kind = StopKind::synced(ExecError::Protocol { step: "parameters" });
        return cleanup(tx, rows, kind, &mut hot).await;
    }
    let portal = match bind_portal(&tx, &statement, &mut hot).await {
        Ok(portal) => portal,
        Err(kind) => return cleanup(tx, rows, kind, &mut hot).await,
    };
    let read = read_pages(&tx, &portal, rows, &mut hot, limits).await;
    match read {
        Read::Finished {
            truncated,
            returned_rows,
            result_bytes,
        } => {
            if limits.read_quality
                && let Err(kind) = capture_quality(&tx, &mut hot, &limits.tables, shared).await
            {
                return cleanup(tx, rows, kind, &mut hot).await;
            }
            let confirmed = rollback_confirmed(tx, started, limits.wall).await;
            if !confirmed {
                let kind = StopKind {
                    error: ExecError::Protocol { step: "rollback" },
                    desync: true,
                    partial: false,
                    truncated,
                    returned_rows,
                    result_bytes,
                };
                return publish_stop(rows, kind, false).await;
            }
            let (pid, backend_start) = {
                let guard = lock(shared);
                (guard.backend_pid, guard.backend_start.clone())
            };
            if !same_backend(client, pid, &backend_start, started, limits.wall).await {
                let err = ExecError::Protocol { step: "backend" };
                let _ = rows.send(StreamItem::Failed(err)).await;
                return Attempt {
                    status: CompletionStatus::Failed,
                    truncated,
                    returned_rows,
                    result_bytes,
                    rollback_confirmed: true,
                    socket_dropped: true,
                    stop: Some(StopClass::Protocol),
                    sqlstate: None,
                    reuse: false,
                };
            }
            Attempt {
                status: CompletionStatus::Succeeded,
                truncated,
                returned_rows,
                result_bytes,
                rollback_confirmed: true,
                socket_dropped: false,
                stop: None,
                sqlstate: None,
                reuse: true,
            }
        }
        Read::Stop(kind) => cleanup(tx, rows, kind, &mut hot).await,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RoleBind {
    Invalid,
    Reuse,
    Proceed,
}

fn role_bind_decision(already: Option<&str>, role: &str, identity_role: &str) -> RoleBind {
    if !role_shape_ok(role) || role != identity_role {
        return RoleBind::Invalid;
    }
    match already {
        Some(bound) if bound != role => RoleBind::Reuse,
        _ => RoleBind::Proceed,
    }
}

struct BoundCancel<'a> {
    shared: &'a Mutex<Shared>,
    token: &'a tokio_postgres::CancelToken,
    tls: &'a tokio_postgres_rustls::MakeRustlsConnect,
    generation: u64,
    pid: i32,
    backend_start: &'a str,
}

struct HotWait<'a> {
    shutdown: &'a mut watch::Receiver<bool>,
    destroy: &'a mut watch::Receiver<bool>,
    cancel: BoundCancel<'a>,
    started: Instant,
    wall: Duration,
    cleanup_deadline: Option<Instant>,
    cancel_attempted: bool,
    deadline_cleanup: bool,
}

enum Hot<T> {
    Ready(T),
    Consumer,
    NotStartedDeadline,
    Deadline,
    Abandoned,
}

fn consumer_stop(returned_rows: u32, result_bytes: u64) -> StopKind {
    StopKind {
        error: ExecError::Protocol { step: "consumer" },
        desync: false,
        partial: true,
        truncated: false,
        returned_rows,
        result_bytes,
    }
}

fn deadline_stop(returned_rows: u32, result_bytes: u64, desync: bool) -> StopKind {
    StopKind {
        error: ExecError::Timeout { sqlstate: None },
        desync,
        partial: false,
        truncated: false,
        returned_rows,
        result_bytes,
    }
}

fn abandoned_stop(returned_rows: u32, result_bytes: u64) -> StopKind {
    StopKind {
        error: ExecError::Protocol { step: "consumer" },
        desync: true,
        partial: true,
        truncated: false,
        returned_rows,
        result_bytes,
    }
}

/// Poll `future` until it completes.
///
/// Shutdown and the client deadline share this future. Dropping it unread
/// would leave the portal response on the socket. Cancel transport and the
/// pinned response share the one cleanup deadline. A client deadline still
/// returns [`Hot::Deadline`] when that transport expires.
async fn await_hot<T>(future: impl Future<Output = T> + Send, hot: &mut HotWait<'_>) -> Hot<T> {
    if destroy_requested(hot.destroy) {
        hot.cancel_attempted = true;
        return Hot::Abandoned;
    }
    if shutdown_requested(hot.shutdown) {
        return Hot::Consumer;
    }
    let Some(remain) = remaining(hot.started, hot.wall) else {
        return Hot::NotStartedDeadline;
    };
    let mut future = std::pin::pin!(future);
    let mut deadline = std::pin::pin!(tokio::time::sleep(remain));
    loop {
        if hot.cancel_attempted && !hot.deadline_cleanup {
            let budget = cleanup_remaining(hot.cleanup_deadline);
            if budget.is_zero() || destroy_requested(hot.destroy) {
                return Hot::Abandoned;
            }
            let destroy = &mut *hot.destroy;
            return tokio::select! {
                biased;
                result = future.as_mut() => Hot::Ready(result),
                changed = destroy.changed() => {
                    let _ = changed;
                    Hot::Abandoned
                }
                () = tokio::time::sleep(budget) => Hot::Abandoned,
            };
        }
        let decision = {
            let shutdown = &mut *hot.shutdown;
            let destroy = &mut *hot.destroy;
            tokio::select! {
                biased;
                changed = destroy.changed() => {
                    if changed.is_err() || destroy_requested(destroy) {
                        CleanupDecision::Destroy
                    } else {
                        CleanupDecision::Ignore
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || shutdown_requested(shutdown) {
                        CleanupDecision::Shutdown
                    } else {
                        CleanupDecision::Ignore
                    }
                }
                result = future.as_mut() => return Hot::Ready(result),
                () = deadline.as_mut() => CleanupDecision::ClientDeadline,
            }
        };
        match decision {
            CleanupDecision::Ignore => {}
            CleanupDecision::Destroy => {
                hot.cancel_attempted = true;
                return Hot::Abandoned;
            }
            CleanupDecision::Shutdown => {
                if arm_and_send(hot).await == Arm::Expired {
                    return Hot::Abandoned;
                }
            }
            CleanupDecision::ClientDeadline => {
                hot.deadline_cleanup = true;
                let _ = arm_and_send(hot).await;
                return Hot::Deadline;
            }
        }
    }
}

enum CleanupDecision {
    Ignore,
    Destroy,
    Shutdown,
    ClientDeadline,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arm {
    Ready,
    Expired,
}

async fn arm_and_send(hot: &mut HotWait<'_>) -> Arm {
    let deadline = begin_cleanup_deadline(hot.cancel.shared, &mut hot.cleanup_deadline);
    hot.cancel_attempted = true;
    let budget = deadline.saturating_duration_since(Instant::now());
    if budget.is_zero() {
        return Arm::Expired;
    }
    match send_bound_cancel(&hot.cancel, budget).await {
        CancelSend::Expired => Arm::Expired,
        CancelSend::Sent | CancelSend::Skipped => Arm::Ready,
    }
}

async fn hot_result<T>(
    future: impl Future<Output = Result<T, PgError>> + Send,
    hot: &mut HotWait<'_>,
    step: &'static str,
) -> Result<T, StopKind> {
    match await_hot(future, hot).await {
        Hot::Consumer => Err(consumer_stop(0, 0)),
        Hot::NotStartedDeadline => Err(deadline_stop(0, 0, false)),
        Hot::Deadline => Err(deadline_stop(0, 0, true)),
        Hot::Abandoned => Err(abandoned_stop(0, 0)),
        Hot::Ready(Ok(_)) if shutdown_requested(hot.shutdown) => Err(consumer_stop(0, 0)),
        Hot::Ready(Ok(value)) => Ok(value),
        Hot::Ready(Err(_)) if shutdown_requested(hot.shutdown) => Err(consumer_stop(0, 0)),
        Hot::Ready(Err(err)) => Err(StopKind::synced(classify_pg(err, step, false))),
    }
}

/// Cancel only while this generation still names the bound backend.
///
/// The stored identity check and `cancel_query` are not separated by another await.
/// No second session is opened. `budget` is the time left on the one cleanup deadline.
async fn send_bound_cancel(cancel: &BoundCancel<'_>, budget: Duration) -> CancelSend {
    if budget.is_zero() {
        return CancelSend::Expired;
    }
    let send = {
        let guard = lock(cancel.shared);
        guard.accepting_cancel
            && guard.generation == cancel.generation
            && guard.backend_pid > 0
            && guard.backend_pid == cancel.pid
            && !cancel.backend_start.is_empty()
            && guard.backend_start == cancel.backend_start
    };
    if !send {
        return CancelSend::Skipped;
    }
    let tls = cancel.tls.clone();
    match tokio::time::timeout(budget, cancel.token.cancel_query(tls)).await {
        Ok(Ok(())) => CancelSend::Sent,
        Ok(Err(_)) | Err(_) => CancelSend::Expired,
    }
}

enum CancelSend {
    Skipped,
    Sent,
    Expired,
}

async fn cleanup(
    tx: Transaction<'_>,
    rows: &mpsc::Sender<StreamItem>,
    kind: StopKind,
    hot: &mut HotWait<'_>,
) -> Attempt {
    if kind.desync {
        if !hot.cancel_attempted {
            let _ = arm_and_send(hot).await;
        }
        drop(tx);
        return publish_stop(rows, kind, false).await;
    }
    let confirmed = if uses_cleanup_clock(&kind.error, kind.partial) {
        let deadline = begin_cleanup_deadline(hot.cancel.shared, &mut hot.cleanup_deadline);
        let budget = deadline.saturating_duration_since(Instant::now());
        if budget.is_zero() {
            drop(tx);
            false
        } else {
            rollback_bounded(tx, budget).await
        }
    } else {
        rollback_confirmed(tx, hot.started, hot.wall).await
    };
    publish_stop(rows, kind, confirmed).await
}

fn uses_cleanup_clock(error: &ExecError, partial: bool) -> bool {
    partial || matches!(error, ExecError::Canceled { .. })
}

async fn publish_stop(
    rows: &mpsc::Sender<StreamItem>,
    kind: StopKind,
    rollback_confirmed: bool,
) -> Attempt {
    if !kind.partial {
        let _ = rows.send(StreamItem::Failed(kind.error.clone())).await;
    }
    attempt_from_kind(&kind, rollback_confirmed, true)
}

fn attempt_from_kind(kind: &StopKind, rollback_confirmed: bool, socket_dropped: bool) -> Attempt {
    let status = if kind.partial {
        CompletionStatus::Partial
    } else {
        CompletionStatus::Failed
    };
    Attempt {
        status,
        truncated: kind.truncated,
        returned_rows: kind.returned_rows,
        result_bytes: kind.result_bytes,
        rollback_confirmed,
        socket_dropped,
        stop: Some(stop_class(&kind.error)),
        sqlstate: kind.error.sqlstate().map(str::to_string),
        reuse: false,
    }
}

struct StopKind {
    error: ExecError,
    desync: bool,
    partial: bool,
    truncated: bool,
    returned_rows: u32,
    result_bytes: u64,
}

impl StopKind {
    fn synced(error: ExecError) -> Self {
        Self {
            error,
            desync: false,
            partial: false,
            truncated: false,
            returned_rows: 0,
            result_bytes: 0,
        }
    }
}

enum Read {
    Finished {
        truncated: bool,
        returned_rows: u32,
        result_bytes: u64,
    },
    Stop(StopKind),
}

async fn read_pages(
    tx: &Transaction<'_>,
    portal: &tokio_postgres::Portal,
    rows: &mpsc::Sender<StreamItem>,
    hot: &mut HotWait<'_>,
    limits: &Limits,
) -> Read {
    let mut returned_rows = 0_u32;
    let mut result_bytes = 0_u64;
    loop {
        if shutdown_requested(hot.shutdown) {
            return Read::Stop(consumer_stop(returned_rows, result_bytes));
        }
        if remaining(hot.started, hot.wall).is_none() {
            return Read::Stop(deadline_stop(returned_rows, result_bytes, false));
        }
        let page = await_hot(tx.query_portal(portal, PORTAL_PAGE_ROWS), hot).await;
        let fetched = match page {
            Hot::Consumer => return Read::Stop(consumer_stop(returned_rows, result_bytes)),
            Hot::NotStartedDeadline => {
                return Read::Stop(deadline_stop(returned_rows, result_bytes, false));
            }
            Hot::Deadline => return Read::Stop(deadline_stop(returned_rows, result_bytes, true)),
            Hot::Abandoned => return Read::Stop(abandoned_stop(returned_rows, result_bytes)),
            Hot::Ready(Err(err)) if shutdown_requested(hot.shutdown) => {
                let _ = err;
                return Read::Stop(consumer_stop(returned_rows, result_bytes));
            }
            Hot::Ready(Err(err)) => {
                return Read::Stop(StopKind {
                    error: classify_pg(err, "portal", false),
                    desync: false,
                    partial: false,
                    truncated: false,
                    returned_rows,
                    result_bytes,
                });
            }
            Hot::Ready(Ok(_)) if shutdown_requested(hot.shutdown) => {
                return Read::Stop(consumer_stop(returned_rows, result_bytes));
            }
            Hot::Ready(Ok(fetched)) => fetched,
        };
        if fetched.is_empty() {
            return Read::Finished {
                truncated: false,
                returned_rows,
                result_bytes,
            };
        }
        if fetched.len() != 1 {
            return Read::Stop(StopKind {
                error: ExecError::Protocol { step: "page" },
                desync: false,
                partial: false,
                truncated: false,
                returned_rows,
                result_bytes,
            });
        }
        let cells = match decode_row(&fetched[0]) {
            Ok(cells) => cells,
            Err(error) => {
                return Read::Stop(StopKind {
                    error,
                    desync: false,
                    partial: false,
                    truncated: false,
                    returned_rows,
                    result_bytes,
                });
            }
        };
        let row_bytes = cells
            .iter()
            .map(cell_bytes)
            .fold(0_u64, u64::saturating_add);
        if stop_before_append(
            returned_rows,
            limits.max_rows,
            result_bytes,
            row_bytes,
            limits.max_result_bytes,
        ) {
            return Read::Finished {
                truncated: true,
                returned_rows,
                result_bytes,
            };
        }
        match push_row(rows, hot.shutdown, cells).await {
            Push::Sent => {
                returned_rows = returned_rows.saturating_add(1);
                result_bytes = result_bytes.saturating_add(row_bytes);
            }
            Push::Gone => {
                return Read::Stop(StopKind {
                    error: ExecError::Protocol { step: "consumer" },
                    desync: false,
                    partial: true,
                    truncated: false,
                    returned_rows,
                    result_bytes,
                });
            }
        }
    }
}

enum Push {
    Sent,
    Gone,
}

async fn push_row(
    rows: &mpsc::Sender<StreamItem>,
    shutdown: &mut watch::Receiver<bool>,
    cells: Vec<Cell>,
) -> Push {
    tokio::select! {
        biased;
        changed = shutdown.changed() => {
            match changed {
                Ok(()) | Err(_) => Push::Gone,
            }
        }
        result = rows.send(StreamItem::Row(cells)) => {
            if result.is_ok() { Push::Sent } else { Push::Gone }
        }
    }
}

async fn open_read_only<'c>(
    client: &'c mut Client,
    hot: &mut HotWait<'_>,
) -> Result<Transaction<'c>, StopKind> {
    let started = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start();
    match await_hot(started, hot).await {
        Hot::Consumer => Err(consumer_stop(0, 0)),
        Hot::NotStartedDeadline => Err(deadline_stop(0, 0, false)),
        Hot::Deadline => Err(deadline_stop(0, 0, true)),
        Hot::Abandoned => Err(abandoned_stop(0, 0)),
        Hot::Ready(Err(err)) if shutdown_requested(hot.shutdown) => {
            let _ = err;
            Err(consumer_stop(0, 0))
        }
        Hot::Ready(Err(err)) => Err(StopKind::synced(classify_pg(err, "begin", false))),
        Hot::Ready(Ok(tx)) => Ok(tx),
    }
}

async fn apply_session(
    tx: &Transaction<'_>,
    role: &str,
    hot: &mut HotWait<'_>,
) -> Result<(), StopKind> {
    platform_batch(tx, "SET LOCAL search_path TO ''", "search_path", hot).await?;
    match platform_query(
        tx,
        "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolbypassrls, rolreplication \
         FROM pg_catalog.pg_roles WHERE rolname = $1",
        role,
        hot,
    )
    .await?
    {
        Some(row) => check_role_row(&row)?,
        None => return Err(StopKind::synced(ExecError::RoleParameter)),
    }
    let quoted = quote_role(role).map_err(StopKind::synced)?;
    let set_role = format!("SET LOCAL ROLE {quoted}");
    platform_batch(tx, &set_role, "set_role", hot).await?;
    let Some(ms) = timeout_parameter_ms(hot.started, hot.wall) else {
        return Err(StopKind::synced(ExecError::Timeout { sqlstate: None }));
    };
    let set_timeout = format!("SET LOCAL statement_timeout = {ms}");
    platform_batch(tx, &set_timeout, "statement_timeout", hot).await?;
    let settings = platform_query_one(
        tx,
        "SELECT current_setting('transaction_isolation'), current_setting('transaction_read_only'), \
         current_user::text, current_setting('search_path')",
        hot,
    )
    .await?;
    let timeout_setting = platform_query_one(
        tx,
        "SELECT setting FROM pg_catalog.pg_settings WHERE name = 'statement_timeout'",
        hot,
    )
    .await?;
    check_settings(&settings, &timeout_setting, role, ms)?;
    Ok(())
}

async fn prepare_user(
    tx: &Transaction<'_>,
    executed_sql: &str,
    hot: &mut HotWait<'_>,
) -> Result<tokio_postgres::Statement, StopKind> {
    hot_result(tx.prepare(executed_sql), hot, "prepare").await
}

async fn bind_portal(
    tx: &Transaction<'_>,
    statement: &tokio_postgres::Statement,
    hot: &mut HotWait<'_>,
) -> Result<tokio_postgres::Portal, StopKind> {
    hot_result(tx.bind(statement, &[]), hot, "bind").await
}

async fn platform_batch(
    tx: &Transaction<'_>,
    sql: &str,
    step: &'static str,
    hot: &mut HotWait<'_>,
) -> Result<(), StopKind> {
    hot_result(tx.batch_execute(sql), hot, step).await
}

async fn platform_query(
    tx: &Transaction<'_>,
    sql: &str,
    role: &str,
    hot: &mut HotWait<'_>,
) -> Result<Option<Row>, StopKind> {
    let rows = hot_result(tx.query(sql, &[&role]), hot, "role").await?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(rows.into_iter().next()),
        _ => Err(StopKind::synced(ExecError::RoleParameter)),
    }
}

async fn platform_query_one(
    tx: &Transaction<'_>,
    sql: &str,
    hot: &mut HotWait<'_>,
) -> Result<Row, StopKind> {
    let mut rows = hot_result(tx.query(sql, &[]), hot, "settings").await?;
    if rows.len() != 1 {
        return Err(StopKind::synced(ExecError::Protocol { step: "settings" }));
    }
    rows.pop()
        .ok_or_else(|| StopKind::synced(ExecError::Protocol { step: "settings" }))
}

fn check_role_row(row: &Row) -> Result<(), StopKind> {
    let mut flags = [false; 7];
    for (index, flag) in flags.iter_mut().enumerate() {
        *flag = row
            .try_get(index)
            .map_err(|_| ExecError::Protocol { step: "role" })
            .map_err(StopKind::synced)?;
    }
    if flags.iter().any(|flag| *flag) {
        Err(StopKind::synced(ExecError::RoleParameter))
    } else {
        Ok(())
    }
}

fn check_settings(row: &Row, timeout_row: &Row, role: &str, ms: u64) -> Result<(), StopKind> {
    let isolation: String = row
        .try_get(0)
        .map_err(|_| ExecError::Protocol { step: "settings" })
        .map_err(StopKind::synced)?;
    let read_only: String = row
        .try_get(1)
        .map_err(|_| ExecError::Protocol { step: "settings" })
        .map_err(StopKind::synced)?;
    let current_user: String = row
        .try_get(2)
        .map_err(|_| ExecError::Protocol { step: "settings" })
        .map_err(StopKind::synced)?;
    let search_path: String = row
        .try_get(3)
        .map_err(|_| ExecError::Protocol { step: "settings" })
        .map_err(StopKind::synced)?;
    let timeout_setting: String = timeout_row
        .try_get(0)
        .map_err(|_| ExecError::Protocol { step: "settings" })
        .map_err(StopKind::synced)?;
    if isolation != "repeatable read" {
        return Err(StopKind::synced(ExecError::Protocol { step: "isolation" }));
    }
    if read_only != "on" {
        return Err(StopKind::synced(ExecError::Protocol { step: "read_only" }));
    }
    if current_user != role {
        return Err(StopKind::synced(ExecError::Protocol {
            step: "current_user",
        }));
    }
    if !search_path_empty(&search_path) {
        return Err(StopKind::synced(ExecError::Protocol {
            step: "search_path",
        }));
    }
    if timeout_setting != ms.to_string() {
        return Err(StopKind::synced(ExecError::Protocol {
            step: "statement_timeout",
        }));
    }
    Ok(())
}

async fn capture_quality(
    tx: &Transaction<'_>,
    hot: &mut HotWait<'_>,
    tables: &[QualityTable],
    shared: &Mutex<Shared>,
) -> Result<(), StopKind> {
    platform_batch(tx, "RESET ROLE", "reset_role", hot).await?;
    platform_batch(tx, "SAVEPOINT oo_gw_quality", "savepoint", hot).await?;
    let snapshot = match read_quality_rows(tx, hot, tables).await {
        Ok(snapshot) => snapshot,
        Err(kind) => {
            platform_batch(
                tx,
                "ROLLBACK TO SAVEPOINT oo_gw_quality",
                "savepoint_rollback",
                hot,
            )
            .await?;
            if matches!(
                kind.error,
                ExecError::Canceled { .. } | ExecError::Timeout { .. }
            ) {
                return Err(kind);
            }
            QualitySnapshot::blank("unreadable")
        }
    };
    lock(shared).quality = snapshot;
    Ok(())
}

const PROBE_NAMES: &[&str] = &[
    "physical_mappings",
    "table_freshness",
    "source_watermark",
    "table_quality",
    "apply_receipts",
    "full_import_run",
    "full_import_table",
    "full_import_quality",
];

struct HeldMap {
    source: String,
    schema: String,
    table: String,
    run_generation: i64,
    physical: String,
    bound: bool,
}

enum ProbeEnd {
    State(&'static str),
    Fatal(StopKind),
}

fn probe_label(sqlstate: Option<&str>) -> &'static str {
    match sqlstate {
        Some("42P01") => "absent",
        Some("42501") => "not_readable",
        Some("57014") => "fatal",
        _ => "unread",
    }
}

async fn probe_hot(
    tx: &Transaction<'_>,
    hot: &mut HotWait<'_>,
    sql: &str,
) -> Result<Vec<Row>, ProbeEnd> {
    if let Err(kind) = platform_batch(tx, "SAVEPOINT oo_gw_probe", "savepoint", hot).await {
        return Err(ProbeEnd::Fatal(kind));
    }
    match hot_result(tx.query(sql, &[]), hot, "quality_probe").await {
        Ok(rows) => {
            if let Err(kind) =
                platform_batch(tx, "RELEASE SAVEPOINT oo_gw_probe", "savepoint", hot).await
            {
                return Err(ProbeEnd::Fatal(kind));
            }
            Ok(rows)
        }
        Err(kind) => {
            if let Err(rollback) = platform_batch(
                tx,
                "ROLLBACK TO SAVEPOINT oo_gw_probe",
                "savepoint_rollback",
                hot,
            )
            .await
            {
                return Err(ProbeEnd::Fatal(rollback));
            }
            let _ = platform_batch(tx, "RELEASE SAVEPOINT oo_gw_probe", "savepoint", hot).await;
            let label = probe_label(kind.error.sqlstate());
            if label == "fatal" {
                Err(ProbeEnd::Fatal(kind))
            } else {
                Err(ProbeEnd::State(label))
            }
        }
    }
}

async fn read_quality_rows(
    tx: &Transaction<'_>,
    hot: &mut HotWait<'_>,
    tables: &[QualityTable],
) -> Result<QualitySnapshot, StopKind> {
    let mut snapshot = QualitySnapshot::blank("read");
    let mut held: Vec<HeldMap> = Vec::new();
    for name in PROBE_NAMES {
        let sql = probe_sql(name);
        let rows = match probe_hot(tx, hot, sql).await {
            Ok(rows) => rows,
            Err(ProbeEnd::State(state)) => {
                snapshot.probes.push(RelationProbe { name, state });
                continue;
            }
            Err(ProbeEnd::Fatal(kind)) => return Err(kind),
        };
        snapshot.probes.push(RelationProbe {
            name,
            state: "read",
        });
        absorb_rows(name, &rows, tables, &mut held, &mut snapshot)?;
    }
    project_held(&held, &mut snapshot);
    bind_maria_epoch(&mut snapshot);
    Ok(snapshot)
}

fn probe_sql(name: &str) -> &'static str {
    match name {
        "physical_mappings" => {
            "SELECT source::text, logical_namespace::text, logical_table::text, \
             run_generation::int8, physical_name::text \
             FROM _oo.physical_mappings WHERE level = 't'"
        }
        "table_freshness" => {
            "SELECT schema_name::text, table_name::text, COALESCE(state, ''), \
             COALESCE(to_char(source_committed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), ''), \
             COALESCE(to_char(applied_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), '') \
             FROM _oo.table_freshness"
        }
        "source_watermark" => {
            "SELECT source::text, consistency::text, cross_table_consistent, \
             applied_pos::text, COALESCE(state, ''), position_codec::text, \
             position_lineage::text FROM _oo.source_watermark"
        }
        "table_quality" => {
            "SELECT schema_name::text, table_name::text, \
             COALESCE(reconcile_status, ''), COALESCE(entity_check, ''), \
             COALESCE(partial_txn_in_progress, false), COALESCE(position_state, ''), \
             COALESCE(budget_state, ''), \
             CASE \
               WHEN unexplained_codes IS NULL THEN false \
               WHEN unexplained_codes = 'null'::jsonb THEN false \
               WHEN unexplained_codes = '{}'::jsonb THEN false \
               WHEN unexplained_codes = '[]'::jsonb THEN false \
               ELSE true \
             END, \
             COALESCE(capture_method, ''), COALESCE(delete_detection, '') \
             FROM _oo.table_quality"
        }
        "apply_receipts" => {
            "SELECT source::text, table_generation::text, applied_pos::text, \
             position_codec::text, cross_table_consistent, partial_txn, \
             COALESCE(to_char(target_commit_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), ''), \
             position_lineage::text, source_epoch::int4 \
             FROM _oo.apply_receipts"
        }
        "full_import_run" => {
            "SELECT source_id::text, source_generation::int8, target_generation::int8, \
             state::text, sealed_at IS NOT NULL FROM _oo.full_import_run"
        }
        "full_import_table" => {
            "SELECT schema_ident::text, table_ident::text, physical_name::text, \
             applied_rows::int8, COALESCE(applied_pos::text, ''), exhausted \
             FROM _oo.full_import_table"
        }
        "full_import_quality" => {
            "SELECT physical_name::text, kind::text, \
             COALESCE(rejected_rows, 0)::int8, COALESCE(accepted_rows, 0)::int8 \
             FROM _oo.full_import_quality"
        }
        _ => "SELECT 1 WHERE false",
    }
}

fn absorb_rows(
    name: &str,
    rows: &[Row],
    tables: &[QualityTable],
    held: &mut Vec<HeldMap>,
    snapshot: &mut QualitySnapshot,
) -> Result<(), StopKind> {
    let decode = |err: tokio_postgres::Error| {
        let _ = err;
        StopKind::synced(ExecError::Protocol { step: "quality" })
    };
    match name {
        "physical_mappings" => {
            for row in rows {
                if held.len() >= 64 {
                    break;
                }
                let source: String = row.try_get(0).map_err(decode)?;
                let schema: String = row.try_get(1).map_err(decode)?;
                let table: String = row.try_get(2).map_err(decode)?;
                let generation: i64 = row.try_get(3).map_err(decode)?;
                let physical: String = row.try_get(4).map_err(decode)?;
                if let Some(item) =
                    hold_mapping(tables, source, schema, table, generation, physical)
                {
                    held.push(item);
                }
            }
        }
        "table_freshness" => {
            for row in rows {
                let schema: String = row.try_get(0).map_err(decode)?;
                let table: String = row.try_get(1).map_err(decode)?;
                let Some(logical) = match_quality_table(tables, &schema, &table) else {
                    continue;
                };
                snapshot.freshness.push(FreshnessObserved {
                    schema: logical.schema.clone(),
                    table: logical.name.clone(),
                    state: row.try_get(2).map_err(decode)?,
                    source_committed_at: row.try_get(3).map_err(decode)?,
                    applied_at: row.try_get(4).map_err(decode)?,
                });
            }
        }
        "source_watermark" => {
            for row in rows {
                if snapshot.watermarks.len() >= 32 {
                    break;
                }
                let source: String = row.try_get(0).map_err(decode)?;
                if !held.iter().any(|item| item.source == source) {
                    continue;
                }
                let consistency: String = row.try_get(1).map_err(decode)?;
                if consistency != "transactional" && consistency != "per_table" {
                    continue;
                }
                let applied_pos: Option<String> = row.try_get(3).map_err(decode)?;
                let Some(applied_pos) = watermark_position(applied_pos) else {
                    continue;
                };
                let cross: bool = row.try_get(2).map_err(decode)?;
                let state: String = row.try_get(4).map_err(decode)?;
                let codec: String = row.try_get(5).map_err(decode)?;
                let lineage: String = row.try_get(6).map_err(decode)?;
                if !watermark_identity(&codec, &lineage, &applied_pos) {
                    continue;
                }
                snapshot.watermarks.push(WatermarkObserved {
                    source,
                    consistency,
                    cross_table_consistent: cross,
                    applied_pos,
                    state,
                    codec,
                    lineage,
                });
            }
        }
        "table_quality" => {
            for row in rows {
                let schema: String = row.try_get(0).map_err(decode)?;
                let table: String = row.try_get(1).map_err(decode)?;
                let Some(logical) = match_quality_table(tables, &schema, &table) else {
                    continue;
                };
                let reconcile: String = row.try_get(2).map_err(decode)?;
                let entity: String = row.try_get(3).map_err(decode)?;
                let partial: bool = row.try_get(4).map_err(decode)?;
                let position: String = row.try_get(5).map_err(decode)?;
                let budget: String = row.try_get(6).map_err(decode)?;
                let unexplained: bool = row.try_get(7).map_err(decode)?;
                push_quality_warning(
                    &mut snapshot.warnings,
                    logical,
                    QualityFacts {
                        reconcile: &reconcile,
                        entity: &entity,
                        partial,
                        position: &position,
                        budget: &budget,
                        unexplained,
                    },
                );
                let capture: String = row.try_get(8).map_err(decode)?;
                let deletion: String = row.try_get(9).map_err(decode)?;
                note_method(&mut snapshot.capture_method, &capture, CAPTURE_METHODS);
                note_method(&mut snapshot.delete_detection, &deletion, DELETE_METHODS);
            }
        }
        "apply_receipts" => {
            for row in rows {
                if snapshot.receipts.len() >= 32 {
                    break;
                }
                let source: String = row.try_get(0).map_err(decode)?;
                let physical: String = row.try_get(1).map_err(decode)?;
                let Some(item) = held
                    .iter()
                    .find(|item| item.source == source && item.physical == physical)
                else {
                    continue;
                };
                let applied_pos: String = row.try_get(2).map_err(decode)?;
                let codec: String = row.try_get(3).map_err(decode)?;
                let lineage: String = row.try_get(7).map_err(decode)?;
                let source_epoch: i32 = row.try_get(8).map_err(decode)?;
                if !receipt_identity(&codec, &lineage, &applied_pos, source_epoch) {
                    continue;
                }
                if codec == "mariadb-gtid/v1"
                    && !snapshot.watermarks.iter().any(|mark| {
                        mark.source == source
                            && mark.codec == "mariadb-gtid/v1"
                            && mark.lineage == lineage
                    })
                {
                    continue;
                }
                snapshot.receipts.push(ReceiptObserved {
                    source,
                    schema: item.schema.clone(),
                    table: item.table.clone(),
                    run_generation: item.run_generation,
                    applied_pos,
                    codec,
                    lineage,
                    source_epoch,
                    cross_table_consistent: row.try_get(4).map_err(decode)?,
                    partial_txn: row.try_get(5).map_err(decode)?,
                    target_commit_at: row.try_get(6).map_err(decode)?,
                });
            }
        }
        "full_import_run" => {
            for row in rows {
                if snapshot.runs.len() >= 32 {
                    break;
                }
                let source: String = row.try_get(0).map_err(decode)?;
                if !held.iter().any(|item| item.source == source) {
                    continue;
                }
                let state: String = row.try_get(3).map_err(decode)?;
                if !matches!(state.as_str(), "open" | "awaiting_publish" | "retired") {
                    continue;
                }
                snapshot.runs.push(ImportRunObserved {
                    source,
                    source_generation: row.try_get(1).map_err(decode)?,
                    target_generation: row.try_get(2).map_err(decode)?,
                    state,
                    sealed: row.try_get(4).map_err(decode)?,
                });
            }
        }
        "full_import_table" => {
            for row in rows {
                if snapshot.imports.len() >= 32 {
                    break;
                }
                let schema: String = row.try_get(0).map_err(decode)?;
                let table: String = row.try_get(1).map_err(decode)?;
                let physical: String = row.try_get(2).map_err(decode)?;
                let Some(item) = held.iter().find(|item| {
                    item.schema == schema && item.table == table && item.physical == physical
                }) else {
                    continue;
                };
                let applied_pos: String = row.try_get(4).map_err(decode)?;
                if !applied_pos.is_empty() && !position_text(&applied_pos) {
                    continue;
                }
                snapshot.imports.push(ImportTableObserved {
                    schema,
                    table,
                    run_generation: item.run_generation,
                    applied_rows: row.try_get(3).map_err(decode)?,
                    applied_pos,
                    exhausted: row.try_get(5).map_err(decode)?,
                });
            }
        }
        "full_import_quality" => {
            let mut sums: Vec<QualityCount> = Vec::new();
            for row in rows {
                let physical: String = row.try_get(0).map_err(decode)?;
                if !held.iter().any(|item| item.physical == physical) {
                    continue;
                }
                let kind: String = row.try_get(1).map_err(decode)?;
                if kind != "batch" && kind != "reject" {
                    continue;
                }
                let rejected: i64 = row.try_get(2).map_err(decode)?;
                let accepted: i64 = row.try_get(3).map_err(decode)?;
                if let Some(found) = sums.iter_mut().find(|item| item.kind == kind) {
                    found.rejected_rows = found.rejected_rows.saturating_add(rejected);
                    found.accepted_rows = found.accepted_rows.saturating_add(accepted);
                    found.batches = found.batches.saturating_add(1);
                } else if sums.len() < 8 {
                    sums.push(QualityCount {
                        kind,
                        rejected_rows: rejected,
                        accepted_rows: accepted,
                        batches: 1,
                    });
                }
            }
            snapshot.quality_counts = sums;
        }
        _ => {}
    }
    Ok(())
}

fn hold_mapping(
    tables: &[QualityTable],
    source: String,
    schema: String,
    table: String,
    generation: i64,
    physical: String,
) -> Option<HeldMap> {
    if !source_text(&source) || generation < 1 || !physical_text(&physical) {
        return None;
    }
    let logical = tables
        .iter()
        .find(|item| item.schema == schema && item.name == table)?;
    if logical.source.as_deref().is_some_and(|expected| expected != source) {
        return None;
    }
    let bound = physical == logical.physical_name;
    Some(HeldMap {
        source,
        schema,
        table,
        run_generation: generation,
        physical,
        bound,
    })
}

fn project_held(held: &[HeldMap], snapshot: &mut QualitySnapshot) {
    for item in held {
        snapshot.mappings.push(MappingObserved {
            source: item.source.clone(),
            schema: item.schema.clone(),
            table: item.table.clone(),
            run_generation: item.run_generation,
            bound: item.bound,
        });
    }
}

fn source_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn physical_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 63
        && bytes.starts_with(b"pt_")
        && bytes[3..].iter().all(|byte| byte.is_ascii_hexdigit())
}

fn position_text(value: &str) -> bool {
    let digits = value.strip_prefix('+').unwrap_or(value);
    (1..=38).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn watermark_position(value: Option<String>) -> Option<String> {
    value.filter(|position| position_text(position))
}

fn codec_text(value: &str) -> bool {
    matches!(
        value,
        "pg-commit-lsn/v1"
            | "mysql-binlog-pos/v1"
            | "batch-cursor-us/v1"
            | "snapshot-zero/v1"
            | "mariadb-gtid/v1"
    )
}

fn canonical_u32(text: &str) -> Option<u32> {
    let bytes = text.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 10
        || !bytes.iter().all(|byte| byte.is_ascii_digit())
        || (bytes.len() > 1 && bytes[0] == b'0')
    {
        return None;
    }
    text.parse().ok()
}

fn maria_lineage(lineage: &str) -> bool {
    let Some(rest) = lineage.strip_prefix('d') else {
        return false;
    };
    let Some((domain, server)) = rest.split_once(".s") else {
        return false;
    };
    let Some(server) = canonical_u32(server) else {
        return false;
    };
    canonical_u32(domain).is_some() && server >= 1
}

fn maria_seq(pos: &str) -> bool {
    let bytes = pos.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 20
        && bytes.iter().all(|byte| byte.is_ascii_digit())
        && (bytes.len() == 1 || bytes[0] != b'0')
        && pos.parse::<u64>().is_ok()
}

fn watermark_identity(codec: &str, lineage: &str, applied_pos: &str) -> bool {
    if codec == "mariadb-gtid/v1" {
        return maria_lineage(lineage) && maria_seq(applied_pos);
    }
    position_text(applied_pos)
}

fn receipt_identity(codec: &str, lineage: &str, applied_pos: &str, epoch: i32) -> bool {
    if !position_text(applied_pos) || !codec_text(codec) {
        return false;
    }
    if codec == "mariadb-gtid/v1" {
        return maria_lineage(lineage) && maria_seq(applied_pos) && epoch >= 0;
    }
    true
}

fn bind_maria_epoch(snapshot: &mut QualitySnapshot) {
    let mut sources = std::collections::BTreeSet::new();
    for mark in &snapshot.watermarks {
        if mark.codec == "mariadb-gtid/v1" {
            sources.insert(mark.source.clone());
        }
    }
    for item in &snapshot.receipts {
        if item.codec == "mariadb-gtid/v1" {
            sources.insert(item.source.clone());
        }
    }
    for source in sources {
        let lineages: Vec<String> = snapshot
            .watermarks
            .iter()
            .filter(|mark| mark.source == source && mark.codec == "mariadb-gtid/v1")
            .map(|mark| mark.lineage.clone())
            .collect();
        let lineage = if lineages.len() == 1 && lineages.iter().all(|item| item == &lineages[0]) {
            Some(lineages[0].clone())
        } else {
            None
        };
        let Some(lineage) = lineage else {
            snapshot.watermarks.retain(|mark| mark.source != source || mark.codec != "mariadb-gtid/v1");
            snapshot.receipts.retain(|item| item.source != source || item.codec != "mariadb-gtid/v1");
            continue;
        };
        snapshot.receipts.retain(|item| {
            item.source != source || item.codec != "mariadb-gtid/v1" || item.lineage == lineage
        });
        let epochs: std::collections::BTreeSet<i32> = snapshot
            .receipts
            .iter()
            .filter(|item| item.source == source && item.codec == "mariadb-gtid/v1")
            .map(|item| item.source_epoch)
            .collect();
        if epochs.len() != 1 {
            snapshot.watermarks.retain(|mark| mark.source != source || mark.codec != "mariadb-gtid/v1");
            snapshot.receipts.retain(|item| item.source != source || item.codec != "mariadb-gtid/v1");
        }
    }
}

const CAPTURE_METHODS: &[&str] = &[
    "c1_cdc",
    "c2_change_feed",
    "c3_cursor",
    "c4_partition",
    "c5_diff",
];
const DELETE_METHODS: &[&str] = &["native", "partition_replace", "reconcile", "none"];

fn note_method(slot: &mut String, value: &str, allowed: &[&str]) {
    if !allowed.contains(&value) {
        return;
    }
    if slot.is_empty() {
        *slot = value.to_owned();
    } else if slot != value && slot != "mixed" {
        *slot = "mixed".to_owned();
    }
}

fn match_quality_table<'a>(
    tables: &'a [QualityTable],
    schema: &str,
    table: &str,
) -> Option<&'a QualityTable> {
    tables.iter().find(|item| {
        (item.schema == schema && item.name == table)
            || (item.physical_schema == schema && item.physical_name == table)
    })
}

struct QualityFacts<'a> {
    reconcile: &'a str,
    entity: &'a str,
    partial: bool,
    position: &'a str,
    budget: &'a str,
    unexplained: bool,
}

fn push_quality_warning(
    warnings: &mut Vec<QualityWarning>,
    table: &QualityTable,
    facts: QualityFacts<'_>,
) {
    let QualityFacts {
        reconcile,
        entity,
        partial,
        position,
        budget,
        unexplained,
    } = facts;
    let push = |warnings: &mut Vec<QualityWarning>, code, message: &str| {
        warnings.push(QualityWarning {
            code,
            schema: table.schema.clone(),
            table: table.name.clone(),
            message: message.to_owned(),
        });
    };
    if reconcile == "mismatch" {
        push(
            warnings,
            "RECONCILE_MISMATCH",
            "reconcile status is mismatch",
        );
    } else if reconcile == "stale" {
        push(warnings, "RECONCILE_STALE", "reconcile status is stale");
    }
    if unexplained {
        push(
            warnings,
            "UNEXPLAINED_CODES",
            "unexplained codes are present",
        );
    }
    if partial {
        push(
            warnings,
            "PARTIAL_TRANSACTION",
            "a partial transaction is in progress",
        );
    }
    if entity == "failed" {
        push(warnings, "ENTITY_CHECK_FAILED", "entity check failed");
    }
    if position == "stale_risk" || position == "expired" {
        push(warnings, "STALE_DATA", "position state is not current");
    }
    if budget == "exhausted" {
        push(warnings, "STALE_DATA", "budget state is exhausted");
    }
}

/// Read `_oo` quality for a login that is not inside a user `SET ROLE`.
///
/// Each relation is probed on its own savepoint. `42P01` is `absent`.
/// `42501` is `not_readable`. Rows that do not match `tables` are omitted.
pub(crate) async fn observe_quality(
    client: &mut Client,
    tables: &[QualityTable],
) -> QualitySnapshot {
    let Ok(transaction) = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
    else {
        return QualitySnapshot::blank("unreadable");
    };
    let snapshot = match observe_quality_rows(&transaction, tables).await {
        Ok(snapshot) => snapshot,
        Err(()) => QualitySnapshot::blank("unreadable"),
    };
    let _ = transaction.rollback().await;
    snapshot
}

async fn observe_quality_rows(
    tx: &Transaction<'_>,
    tables: &[QualityTable],
) -> Result<QualitySnapshot, ()> {
    let mut snapshot = QualitySnapshot::blank("read");
    let mut held: Vec<HeldMap> = Vec::new();
    for name in PROBE_NAMES {
        let sql = probe_sql(name);
        let rows = match probe_plain(tx, sql).await {
            Ok(rows) => rows,
            Err(Some(state)) => {
                snapshot.probes.push(RelationProbe { name, state });
                continue;
            }
            Err(None) => return Err(()),
        };
        snapshot.probes.push(RelationProbe {
            name,
            state: "read",
        });
        absorb_rows(name, &rows, tables, &mut held, &mut snapshot).map_err(|_| ())?;
    }
    project_held(&held, &mut snapshot);
    bind_maria_epoch(&mut snapshot);
    Ok(snapshot)
}

async fn probe_plain(tx: &Transaction<'_>, sql: &str) -> Result<Vec<Row>, Option<&'static str>> {
    tx.batch_execute("SAVEPOINT oo_gw_probe")
        .await
        .map_err(|_| None)?;
    match tx.query(sql, &[]).await {
        Ok(rows) => {
            tx.batch_execute("RELEASE SAVEPOINT oo_gw_probe")
                .await
                .map_err(|_| None)?;
            Ok(rows)
        }
        Err(err) => {
            tx.batch_execute("ROLLBACK TO SAVEPOINT oo_gw_probe")
                .await
                .map_err(|_| None)?;
            let _ = tx.batch_execute("RELEASE SAVEPOINT oo_gw_probe").await;
            let code = err.code().map(|code| code.code().to_owned());
            let label = probe_label(code.as_deref());
            if label == "fatal" {
                Err(None)
            } else {
                Err(Some(label))
            }
        }
    }
}

async fn rollback_bounded(tx: Transaction<'_>, bound: Duration) -> bool {
    match tokio::time::timeout(bound, tx.rollback()).await {
        Ok(Ok(())) => true,
        Ok(Err(_)) | Err(_) => false,
    }
}

async fn rollback_confirmed(tx: Transaction<'_>, started: Instant, wall: Duration) -> bool {
    let Some(remain) = remaining(started, wall) else {
        drop(tx);
        return false;
    };
    match tokio::time::timeout(remain, tx.rollback()).await {
        Ok(Ok(())) => true,
        Ok(Err(_)) | Err(_) => false,
    }
}

async fn same_backend(
    client: &Client,
    pid: i32,
    backend_start: &str,
    started: Instant,
    wall: Duration,
) -> bool {
    let Some(remain) = remaining(started, wall) else {
        return false;
    };
    let query = tokio::time::timeout(
        remain,
        client.query(
            "SELECT pg_catalog.pg_backend_pid(), \
             COALESCE(to_char(backend_start AT TIME ZONE 'UTC', 'YYYYMMDDHH24MISSUS'), '') \
             FROM pg_catalog.pg_stat_activity WHERE pid = pg_catalog.pg_backend_pid()",
            &[],
        ),
    )
    .await;
    let Ok(Ok(rows)) = query else {
        return false;
    };
    if rows.len() != 1 {
        return false;
    }
    let seen_pid: Result<i32, _> = rows[0].try_get(0);
    let seen_start: Result<String, _> = rows[0].try_get(1);
    matches!((seen_pid, seen_start), (Ok(seen_pid), Ok(seen_start)) if seen_pid == pid && seen_start == backend_start)
}

async fn read_backend_binding(client: &Client) -> Result<(i32, String), ExecError> {
    let rows = client
        .query(
            "SELECT pg_catalog.pg_backend_pid(), \
             COALESCE(to_char(a.backend_start AT TIME ZONE 'UTC', 'YYYYMMDDHH24MISSUS'), ''), \
             s.ssl \
             FROM pg_catalog.pg_stat_activity AS a \
             JOIN pg_catalog.pg_stat_ssl AS s ON s.pid = a.pid \
             WHERE a.pid = pg_catalog.pg_backend_pid()",
            &[],
        )
        .await
        .map_err(|_| ExecError::Protocol { step: "bind" })?;
    if rows.len() != 1 {
        return Err(ExecError::Protocol { step: "bind" });
    }
    let pid: i32 = rows[0]
        .try_get(0)
        .map_err(|_| ExecError::Protocol { step: "bind" })?;
    let backend_start: String = rows[0]
        .try_get(1)
        .map_err(|_| ExecError::Protocol { step: "bind" })?;
    let ssl: bool = rows[0]
        .try_get(2)
        .map_err(|_| ExecError::Protocol { step: "bind" })?;
    if pid <= 0 || backend_start.is_empty() {
        return Err(ExecError::Protocol { step: "backend" });
    }
    if !ssl {
        return Err(ExecError::Protocol { step: "tls" });
    }
    Ok((pid, backend_start))
}

fn allowed_result_types(statement: &tokio_postgres::Statement) -> Result<(), ExecError> {
    for column in statement.columns() {
        let oid = column.type_().oid();
        if !oid_allowed(oid) {
            return Err(ExecError::TypeRefused { oid });
        }
    }
    Ok(())
}

fn decode_row(row: &Row) -> Result<Vec<Cell>, ExecError> {
    let mut cells = Vec::with_capacity(row.len());
    for (index, column) in row.columns().iter().enumerate() {
        cells.push(decode_cell(
            row,
            index,
            column.type_().oid(),
            column.type_modifier(),
        )?);
    }
    Ok(cells)
}

fn decode_cell(row: &Row, index: usize, oid: u32, typmod: i32) -> Result<Cell, ExecError> {
    match oid {
        OID_BOOL => match row
            .try_get::<_, Option<bool>>(index)
            .map_err(|_| ExecError::Protocol { step: "decode" })?
        {
            None => Ok(Cell::Null),
            Some(value) => Ok(Cell::Bool(value)),
        },
        OID_INT2 => int_cell(
            row.try_get::<_, Option<i16>>(index)
                .map_err(|_| ExecError::Protocol { step: "decode" })?,
        ),
        OID_INT4 => int_cell(
            row.try_get::<_, Option<i32>>(index)
                .map_err(|_| ExecError::Protocol { step: "decode" })?,
        ),
        OID_INT8 => int_cell(
            row.try_get::<_, Option<i64>>(index)
                .map_err(|_| ExecError::Protocol { step: "decode" })?,
        ),
        OID_TEXT | OID_VARCHAR | OID_BPCHAR => text_cell(row, index),
        OID_NUMERIC => decode_numeric_cell(row, index, typmod),
        OID_BYTEA | OID_JSON | OID_FLOAT8 | OID_DATE | OID_TIME | OID_TIMESTAMP
        | OID_TIMESTAMPTZ | OID_INTERVAL | OID_UUID | OID_JSONB => match row
            .try_get::<_, Option<ExtendedPayload>>(index)
            .map_err(|_| ExecError::Protocol { step: "decode" })?
        {
            None => Ok(Cell::Null),
            Some(payload) => format_payload(oid, &payload.0),
        },
        other => Err(ExecError::TypeRefused { oid: other }),
    }
}

fn text_cell(row: &Row, index: usize) -> Result<Cell, ExecError> {
    match row
        .try_get::<_, Option<String>>(index)
        .map_err(|_| ExecError::Protocol { step: "decode" })?
    {
        None => Ok(Cell::Null),
        Some(value) if value.contains('\0') => Err(ExecError::Protocol { step: "decode" }),
        Some(value) => Ok(Cell::Text(value)),
    }
}

fn decode_numeric_cell(row: &Row, index: usize, typmod: i32) -> Result<Cell, ExecError> {
    match row.try_get::<_, Option<PgNumericCell>>(index) {
        Ok(None) => Ok(Cell::Null),
        Ok(Some(cell)) => {
            let exact = apply_numeric_typmod(cell.0, typmod)?;
            Ok(Cell::Decimal(exact))
        }
        Err(err) => Err(ExecError::Protocol {
            step: numeric_error_step(&err),
        }),
    }
}

fn numeric_error_step(err: &PgError) -> &'static str {
    err.source()
        .and_then(numeric_reject_step)
        .unwrap_or("decode")
}

fn numeric_reject_step(err: &(dyn StdError + 'static)) -> Option<&'static str> {
    err.downcast_ref::<NumericReject>()
        .map(|reject| reject.step)
}

fn int_cell<T>(value: Option<T>) -> Result<Cell, ExecError>
where
    T: fmt::Display,
{
    let Some(value) = value else {
        return Ok(Cell::Null);
    };
    let text = value.to_string();
    if !canonical_int_text(&text) {
        return Err(ExecError::Protocol { step: "decode" });
    }
    Ok(Cell::Int(text))
}

fn classify_pg(err: PgError, step: &'static str, deadline_owned: bool) -> ExecError {
    let mapped = if let Some(db) = err.as_db_error() {
        let sqlstate = db.code().code().to_string();
        let message = db.message();
        if sqlstate == "57014" && message.starts_with("canceling statement due to user request") {
            ExecError::Canceled { sqlstate }
        } else if sqlstate == "57014" {
            ExecError::Timeout {
                sqlstate: Some(sqlstate),
            }
        } else {
            ExecError::Database { sqlstate }
        }
    } else if err.is_closed() {
        ExecError::Protocol { step: "connection" }
    } else {
        ExecError::Protocol { step }
    };
    if deadline_owned && let ExecError::Canceled { sqlstate } = mapped {
        return ExecError::Timeout {
            sqlstate: Some(sqlstate),
        };
    }
    mapped
}

fn stop_class(err: &ExecError) -> StopClass {
    match err {
        ExecError::Timeout { .. } => StopClass::Timeout,
        ExecError::Canceled { .. } => StopClass::Canceled,
        ExecError::TypeRefused { .. } => StopClass::TypeRefused,
        ExecError::Database { .. } => StopClass::Database,
        _ => StopClass::Protocol,
    }
}

fn completion_from(attempt: &Attempt, ids: &AuditIds) -> CompletionRecord {
    CompletionRecord {
        status: attempt.status,
        truncated: attempt.truncated,
        returned_rows: attempt.returned_rows,
        result_bytes: attempt.result_bytes,
        rollback_confirmed: attempt.rollback_confirmed,
        socket_dropped: attempt.socket_dropped,
        stop: attempt.stop,
        sqlstate: attempt.sqlstate.clone(),
        role: ids.role.clone(),
        original_sha256: ids.original_sha256.clone(),
        executed_sha256: ids.executed_sha256.clone(),
        fingerprint_hex: ids.fingerprint_hex.clone(),
        project: ids.project.clone(),
        policy_id: ids.policy_id.clone(),
        backend_pid: ids.backend_pid,
        backend_start: ids.backend_start.clone(),
    }
}

fn store_summary(shared: &Mutex<Shared>, attempt: &Attempt, end_audit_ok: bool) {
    lock(shared).summary = Some(QuerySummary {
        status: attempt.status,
        truncated: attempt.truncated,
        returned_rows: attempt.returned_rows,
        result_bytes: attempt.result_bytes,
        rollback_confirmed: attempt.rollback_confirmed,
        socket_dropped: attempt.socket_dropped,
        stop: attempt.stop,
        sqlstate: attempt.sqlstate.clone(),
        end_audit_ok,
    });
}

fn patch_audit(shared: &Mutex<Shared>, end_audit_ok: bool) {
    if let Some(summary) = lock(shared).summary.as_mut() {
        summary.end_audit_ok = end_audit_ok;
    }
}

fn begin_generation(shared: &Mutex<Shared>) -> u64 {
    let mut guard = lock(shared);
    guard.generation = guard.generation.wrapping_add(1);
    guard.accepting_cancel = true;
    guard.summary = None;
    guard.result_columns.clear();
    guard.quality = QualitySnapshot::skipped();
    guard.cleanup_deadline = None;
    guard.cancel_packet_sent = false;
    guard.generation
}

fn end_generation(shared: &Mutex<Shared>) {
    let mut guard = lock(shared);
    guard.generation = guard.generation.wrapping_add(1);
    guard.accepting_cancel = false;
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|err| err.into_inner())
}

fn remaining(started: Instant, wall: Duration) -> Option<Duration> {
    wall.checked_sub(started.elapsed())
        .filter(|remain| !remain.is_zero())
}

fn budget_left(deadline: Instant, now: Instant) -> Duration {
    deadline.saturating_duration_since(now)
}

fn cleanup_remaining(deadline: Option<Instant>) -> Duration {
    deadline
        .map(|deadline| budget_left(deadline, Instant::now()))
        .unwrap_or(Duration::ZERO)
}

fn may_lend(open: bool, reserved: bool, packet_sent: bool) -> bool {
    open && !reserved && !packet_sent
}

fn begin_cleanup_deadline(shared: &Mutex<Shared>, local: &mut Option<Instant>) -> Instant {
    if let Some(deadline) = *local {
        return deadline;
    }
    let deadline = {
        let mut guard = lock(shared);
        match guard.cleanup_deadline {
            Some(deadline) => deadline,
            None => {
                let deadline = Instant::now() + CLEANUP_BOUND;
                guard.cleanup_deadline = Some(deadline);
                deadline
            }
        }
    };
    *local = Some(deadline);
    deadline
}

fn shared_cleanup_remaining(shared: &Mutex<Shared>) -> Duration {
    cleanup_remaining(lock(shared).cleanup_deadline)
}

fn try_reserve_cancel(shared: &Mutex<Shared>, generation: u64) -> bool {
    let mut guard = lock(shared);
    if !guard.open
        || !guard.accepting_cancel
        || guard.generation != generation
        || guard.cancel_reserved.is_some()
    {
        return false;
    }
    guard.cancel_reserved = Some(generation);
    if guard.cleanup_deadline.is_none() {
        guard.cleanup_deadline = Some(Instant::now() + CLEANUP_BOUND);
    }
    true
}

/// Future-local owner of the generation reserved by `try_reserve_cancel`.
///
/// `CancelHandle` is cloned, including by the session, so its destructor must
/// not touch the reservation. Drop runs on the future that reserved. It does
/// not await.
struct ReservationGuard {
    shared: Arc<Mutex<Shared>>,
    generation: u64,
    fate: ReservationFate,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReservationFate {
    /// Dropped before `cancel_query` was entered.
    Release,
    /// Transport was entered, or this future already chose fail-close.
    FailClose,
    /// The future settled the reservation on its own path.
    Disarmed,
}

impl ReservationGuard {
    fn arm(shared: Arc<Mutex<Shared>>, generation: u64) -> Self {
        Self {
            shared,
            generation,
            fate: ReservationFate::Release,
        }
    }

    fn enter_transport(&mut self) {
        self.fate = ReservationFate::FailClose;
    }

    fn fail_close_on_drop(&mut self) {
        self.fate = ReservationFate::FailClose;
    }

    fn disarm(&mut self) {
        self.fate = ReservationFate::Disarmed;
    }
}

impl Drop for ReservationGuard {
    fn drop(&mut self) {
        match self.fate {
            ReservationFate::Disarmed => {}
            ReservationFate::Release => release_own_reservation(&self.shared, self.generation),
            ReservationFate::FailClose => fail_close_owned(&self.shared, self.generation),
        }
    }
}

fn release_own_reservation(shared: &Mutex<Shared>, generation: u64) {
    let notify = {
        let mut guard = lock(shared);
        if guard.cancel_reserved != Some(generation) {
            return;
        }
        guard.cancel_reserved = None;
        Arc::clone(&guard.cancel_notify)
    };
    notify.notify_waiters();
}

fn fail_close_owned(shared: &Mutex<Shared>, generation: u64) {
    let notify = {
        let mut guard = lock(shared);
        if guard.cancel_reserved != Some(generation) {
            return;
        }
        guard.open = false;
        guard.accepting_cancel = false;
        guard.cancel_reserved = None;
        let _ = guard.destroy.send(true);
        Arc::clone(&guard.cancel_notify)
    };
    notify.notify_waiters();
}

/// Producer exit with a reservation still set. The check and the close share
/// one lock, so a generation that already released is left open.
fn close_leftover_reservation(shared: &Mutex<Shared>) {
    let notify = {
        let mut guard = lock(shared);
        if guard.cancel_reserved.is_none() {
            return;
        }
        guard.open = false;
        guard.accepting_cancel = false;
        guard.cancel_reserved = None;
        let _ = guard.destroy.send(true);
        Arc::clone(&guard.cancel_notify)
    };
    notify.notify_waiters();
}

fn mark_cancel_packet(shared: &Mutex<Shared>) {
    let notify = {
        let mut guard = lock(shared);
        guard.cancel_packet_sent = true;
        guard.cancel_reserved = None;
        Arc::clone(&guard.cancel_notify)
    };
    notify.notify_waiters();
}

async fn observe_within(
    observer: &Client,
    pid: i32,
    budget: Duration,
) -> Result<Option<BackendView>, ExecError> {
    if budget.is_zero() {
        return Err(ExecError::Timeout { sqlstate: None });
    }
    match tokio::time::timeout(budget, observe_backend(observer, pid)).await {
        Ok(result) => result,
        Err(_) => Err(ExecError::Timeout { sqlstate: None }),
    }
}

async fn wait_cancel_settled(shared: &Mutex<Shared>) {
    loop {
        let notify = Arc::clone(&lock(shared).cancel_notify);
        let notified = notify.notified();
        let remain = {
            let guard = lock(shared);
            if guard.cancel_reserved.is_none() {
                return;
            }
            cleanup_remaining(guard.cleanup_deadline)
        };
        if remain.is_zero() {
            return;
        }
        let _ = tokio::time::timeout(remain, notified).await;
    }
}

async fn settle_lend(mut attempt: Attempt, shared: &Mutex<Shared>) -> Attempt {
    if !attempt.reuse {
        close_leftover_reservation(shared);
        return attempt;
    }
    wait_cancel_settled(shared).await;
    close_leftover_reservation(shared);
    let lend = {
        let mut guard = lock(shared);
        let reserved = guard.cancel_reserved.is_some();
        let allowed = may_lend(guard.open, reserved, guard.cancel_packet_sent);
        if allowed {
            guard.accepting_cancel = false;
        }
        allowed
    };
    if !lend {
        attempt.reuse = false;
        attempt.socket_dropped = true;
    }
    attempt
}

fn destroy_requested(destroy: &watch::Receiver<bool>) -> bool {
    *destroy.borrow()
}

fn timeout_parameter_ms(started: Instant, wall: Duration) -> Option<u64> {
    let remain = remaining(started, wall)?;
    let ms = u64::try_from(remain.as_millis()).ok()?;
    if ms == 0 {
        return None;
    }
    Some(ms.saturating_sub(timeout_margin_ms()).max(1))
}

fn timeout_margin_ms() -> u64 {
    u64::try_from(TIMEOUT_MARGIN.as_millis()).unwrap_or(250)
}

fn checked_wall(wall: Duration) -> Result<Duration, ExecError> {
    if !wall.subsec_nanos().is_multiple_of(1_000_000) {
        return Err(ExecError::Limit);
    }
    let ms = wall.as_millis();
    if (1..=u128::from(timeout_budget_ms())).contains(&ms) {
        Ok(wall)
    } else {
        Err(ExecError::Limit)
    }
}

fn timeout_budget_ms() -> u64 {
    u64::try_from(MAX_QUERY_WALL.as_millis()).unwrap_or(30_000)
}

fn search_path_empty(value: &str) -> bool {
    matches!(value, "" | "\"\"")
}

fn shutdown_requested(shutdown: &watch::Receiver<bool>) -> bool {
    *shutdown.borrow()
}

fn role_shape_ok(role: &str) -> bool {
    let Some(slug) = role.strip_prefix("oo_r_") else {
        return false;
    };
    let bytes = slug.as_bytes();
    if bytes.is_empty() || bytes.len() > 58 || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    if !bytes[1..]
        .iter()
        .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return false;
    }
    !reserved_role(role)
}

fn reserved_role(value: &str) -> bool {
    matches!(
        value,
        "public"
            | "postgres"
            | "oo_target_owner"
            | "oo_pipeline_runtime"
            | "oo_fixture_admin"
            | "oo_gateway"
            | "pg_read_all_stats"
    ) || value.starts_with("pg_")
}

fn quote_role(role: &str) -> Result<String, ExecError> {
    if !role_shape_ok(role) || role.contains('"') {
        return Err(ExecError::RoleParameter);
    }
    Ok(format!("\"{role}\""))
}

fn oid_allowed(oid: u32) -> bool {
    matches!(
        oid,
        OID_BOOL
            | OID_BYTEA
            | OID_INT8
            | OID_INT2
            | OID_INT4
            | OID_TEXT
            | OID_JSON
            | OID_FLOAT8
            | OID_BPCHAR
            | OID_VARCHAR
            | OID_DATE
            | OID_TIME
            | OID_TIMESTAMP
            | OID_TIMESTAMPTZ
            | OID_INTERVAL
            | OID_NUMERIC
            | OID_UUID
            | OID_JSONB
    )
}

struct ExtendedPayload(Vec<u8>);

impl<'a> FromSql<'a> for ExtendedPayload {
    fn from_sql(_ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn StdError + Sync + Send>> {
        Ok(Self(raw.to_vec()))
    }

    fn accepts(ty: &Type) -> bool {
        oid_allowed(ty.oid())
            && !matches!(
                ty.oid(),
                OID_BOOL
                    | OID_INT8
                    | OID_INT2
                    | OID_INT4
                    | OID_TEXT
                    | OID_VARCHAR
                    | OID_BPCHAR
                    | OID_NUMERIC
            )
    }
}

/// Render one binary payload. Dates are ISO-8601. `bytea` is lowercase hex.
/// `jsonb` is the version byte plus the server's JSON text. Floats that are
/// not finite are refused. This is not the numeric exact path.
pub(crate) fn format_payload(oid: u32, raw: &[u8]) -> Result<Cell, ExecError> {
    let text = match oid {
        OID_BYTEA => hex_lower(raw),
        OID_JSON => json_text(raw)?,
        OID_JSONB => jsonb_text(raw)?,
        OID_FLOAT8 => float8_text(raw)?,
        OID_DATE => date_text(raw)?,
        OID_TIME => time_text(raw)?,
        OID_TIMESTAMP => timestamp_text(raw, false)?,
        OID_TIMESTAMPTZ => timestamp_text(raw, true)?,
        OID_INTERVAL => interval_text(raw)?,
        OID_UUID => uuid_text(raw)?,
        _ => return Err(ExecError::TypeRefused { oid }),
    };
    if text.contains('\0') {
        return Err(ExecError::Protocol { step: "decode" });
    }
    Ok(Cell::Text(text))
}

fn hex_lower(raw: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(raw.len() * 2);
    for byte in raw {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn json_text(raw: &[u8]) -> Result<String, ExecError> {
    let text = std::str::from_utf8(raw).map_err(|_| ExecError::Protocol { step: "json" })?;
    serde_json::from_str::<serde_json::Value>(text)
        .map_err(|_| ExecError::Protocol { step: "json" })?;
    Ok(text.to_owned())
}

fn jsonb_text(raw: &[u8]) -> Result<String, ExecError> {
    let Some((1, rest)) = raw.split_first() else {
        return Err(ExecError::Protocol { step: "jsonb" });
    };
    json_text(rest).map_err(|_| ExecError::Protocol { step: "jsonb" })
}

fn float8_text(raw: &[u8]) -> Result<String, ExecError> {
    if raw.len() != 8 {
        return Err(ExecError::Protocol { step: "float8" });
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(raw);
    let value = f64::from_be_bytes(bytes);
    if !value.is_finite() {
        return Err(ExecError::Protocol { step: "float8" });
    }
    Ok(value.to_string())
}

fn date_text(raw: &[u8]) -> Result<String, ExecError> {
    if raw.len() != 4 {
        return Err(ExecError::Protocol { step: "date" });
    }
    let days = i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
    let unix = i64::from(days) + PG_EPOCH_UNIX_DAYS;
    let (year, month, day) =
        civil_from_days(unix).map_err(|_| ExecError::Protocol { step: "date" })?;
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

fn time_text(raw: &[u8]) -> Result<String, ExecError> {
    let bytes: [u8; 8] = raw
        .try_into()
        .map_err(|_| ExecError::Protocol { step: "time" })?;
    let micros = i64::from_be_bytes(bytes);
    if !(0..=86_400_000_000).contains(&micros) {
        return Err(ExecError::Protocol { step: "time" });
    }
    let seconds = micros / 1_000_000;
    Ok(format!(
        "{:02}:{:02}:{:02}.{:06}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60,
        micros % 1_000_000
    ))
}

/// Native MySQL TIME is sealed as a signed microsecond duration. Other
/// PostgreSQL interval shapes have no MySQL TIME equivalent and stay refused.
fn interval_text(raw: &[u8]) -> Result<String, ExecError> {
    let bytes: [u8; 16] = raw.try_into().map_err(|_| ExecError::Protocol { step: "interval" })?;
    if bytes[8..].iter().any(|byte| *byte != 0) {
        return Err(ExecError::Protocol { step: "interval" });
    }
    let micros = i64::from_be_bytes(bytes[..8].try_into().map_err(|_| ExecError::Protocol { step: "interval" })?);
    let negative = micros < 0;
    let magnitude = micros.unsigned_abs();
    let seconds = magnitude / 1_000_000;
    Ok(format!("{}{:02}:{:02}:{:02}.{:06}", if negative { "-" } else { "" },
        seconds / 3600, (seconds % 3600) / 60, seconds % 60, magnitude % 1_000_000))
}

fn timestamp_text(raw: &[u8], zone: bool) -> Result<String, ExecError> {
    if raw.len() != 8 {
        return Err(ExecError::Protocol { step: "timestamp" });
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(raw);
    let micros = i64::from_be_bytes(bytes);
    if micros == i64::MAX || micros == i64::MIN {
        return Err(ExecError::Protocol { step: "timestamp" });
    }
    let unix_us = micros
        .checked_add(PG_EPOCH_UNIX_US)
        .ok_or(ExecError::Protocol { step: "timestamp" })?;
    let secs = unix_us.div_euclid(1_000_000);
    let frac = unix_us.rem_euclid(1_000_000) as u32;
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400) as u32;
    let (year, month, day) =
        civil_from_days(days).map_err(|_| ExecError::Protocol { step: "timestamp" })?;
    let hour = sod / 3600;
    let minute = (sod % 3600) / 60;
    let second = sod % 60;
    let suffix = if zone { "Z" } else { "" };
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{frac:06}{suffix}"
    ))
}

fn uuid_text(raw: &[u8]) -> Result<String, ExecError> {
    if raw.len() != 16 {
        return Err(ExecError::Protocol { step: "uuid" });
    }
    let hex = hex_lower(raw);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

fn civil_from_days(unix_days: i64) -> Result<(i32, u32, u32), ()> {
    let z = unix_days.checked_add(719_468).ok_or(())?;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(());
    }
    Ok((year as i32, month as u32, day as u32))
}

#[derive(Debug)]
struct NumericReject {
    step: &'static str,
}

impl fmt::Display for NumericReject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.step)
    }
}

impl StdError for NumericReject {}

#[derive(Debug)]
struct PgNumericCell(ExactDecimal);

impl<'a> FromSql<'a> for PgNumericCell {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn StdError + Sync + Send>> {
        if ty.oid() != OID_NUMERIC {
            return Err(Box::new(NumericReject {
                step: "numeric_payload",
            }));
        }
        decode_numeric_payload(raw)
            .map(PgNumericCell)
            .map_err(|step| Box::new(NumericReject { step }) as _)
    }

    fn accepts(ty: &Type) -> bool {
        ty.oid() == OID_NUMERIC
    }
}

fn apply_numeric_typmod(exact: ExactDecimal, typmod: i32) -> Result<ExactDecimal, ExecError> {
    let Some((precision, scale)) = numeric_typmod_parts(typmod) else {
        return Ok(exact);
    };
    if exact.scale() != scale {
        return Err(ExecError::Protocol {
            step: "numeric_scale",
        });
    }
    let width = usize::try_from(precision).map_err(|_| ExecError::Protocol {
        step: "numeric_precision",
    })?;
    if exact.coefficient().len() > width {
        return Err(ExecError::Protocol {
            step: "numeric_precision",
        });
    }
    Ok(exact)
}

/// PostgreSQL's 11-bit signed numeric typmod. `None` means unconstrained.
fn numeric_typmod_parts(typmod: i32) -> Option<(i32, i32)> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod.wrapping_sub(4);
    let precision = (adjusted >> 16) & 0xFFFF;
    let raw = adjusted & 0x7FF;
    let scale = (raw ^ 0x400) - 0x400;
    Some((precision, scale))
}

fn decode_numeric_payload(raw: &[u8]) -> Result<ExactDecimal, &'static str> {
    if raw.len() < 8 {
        return Err("numeric_payload");
    }
    let ndigits = i16::from_be_bytes([raw[0], raw[1]]);
    let weight = i16::from_be_bytes([raw[2], raw[3]]);
    let sign = u16::from_be_bytes([raw[4], raw[5]]);
    let dscale = i16::from_be_bytes([raw[6], raw[7]]);
    if ndigits < 0 || dscale < 0 {
        return Err("numeric_payload");
    }
    let count = usize::try_from(ndigits).map_err(|_| "numeric_payload")?;
    let body = count.checked_mul(2).ok_or("numeric_payload")?;
    let expected = body.checked_add(8).ok_or("numeric_payload")?;
    if raw.len() != expected {
        return Err("numeric_payload");
    }
    match sign {
        NUMERIC_SIGN_NAN => return Err("numeric_nan"),
        NUMERIC_SIGN_PINF => return Err("numeric_infinity"),
        NUMERIC_SIGN_NINF => return Err("numeric_neg_infinity"),
        NUMERIC_SIGN_POS | NUMERIC_SIGN_NEG => {}
        _ => return Err("numeric_payload"),
    }
    let scale = usize::try_from(dscale).map_err(|_| "numeric_payload")?;
    if scale > CANONICAL_MAX_COEFFICIENT_DIGITS {
        return Err("numeric_precision");
    }
    let point = i32::from(weight)
        .checked_add(1)
        .and_then(|value| value.checked_mul(4))
        .ok_or("numeric_precision")?;
    if point > i32::try_from(CANONICAL_MAX_COEFFICIENT_DIGITS).unwrap_or(i32::MAX) {
        return Err("numeric_precision");
    }
    let mut groups = Vec::with_capacity(count);
    for index in 0..count {
        let off = 8 + index * 2;
        let digit = i16::from_be_bytes([raw[off], raw[off + 1]]);
        if !(0..=9_999).contains(&digit) {
            return Err("numeric_payload");
        }
        groups.push(digit as u16);
    }
    if groups.is_empty() || groups.iter().all(|digit| *digit == 0) {
        return ExactDecimal::zero(i32::from(dscale)).map_err(decimal_step);
    }
    let (int_part, frac_part) = split_numeric(weight, scale, &groups)?;
    let coefficient = numeric_coefficient(&int_part, &frac_part)?;
    ExactDecimal::new(sign == NUMERIC_SIGN_NEG, coefficient, i32::from(dscale))
        .map_err(decimal_step)
}

fn decimal_step(err: DecimalError) -> &'static str {
    match err {
        DecimalError::Budget => "numeric_precision",
        _ => "numeric_payload",
    }
}

fn numeric_coefficient(int_part: &str, frac_part: &str) -> Result<String, &'static str> {
    let trimmed_int = int_part.trim_start_matches('0');
    let coefficient = if trimmed_int.is_empty() {
        let trimmed_frac = frac_part.trim_start_matches('0');
        if trimmed_frac.is_empty() {
            "0".to_string()
        } else {
            trimmed_frac.to_string()
        }
    } else {
        let mut combined = String::with_capacity(trimmed_int.len() + frac_part.len());
        combined.push_str(trimmed_int);
        combined.push_str(frac_part);
        combined
    };
    if coefficient.len() > CANONICAL_MAX_COEFFICIENT_DIGITS {
        return Err("numeric_precision");
    }
    Ok(coefficient)
}

fn split_numeric(
    weight: i16,
    dscale: usize,
    groups: &[u16],
) -> Result<(String, String), &'static str> {
    let mut int_part = String::new();
    let mut frac_part = String::new();
    if weight < 0 {
        let lead_groups = i32::from(weight)
            .checked_neg()
            .and_then(|value| value.checked_sub(1))
            .ok_or("numeric_precision")?;
        if lead_groups < 0 {
            return Err("numeric_payload");
        }
        let lead_digits = usize::try_from(lead_groups)
            .ok()
            .and_then(|value| value.checked_mul(4))
            .ok_or("numeric_precision")?;
        if lead_digits >= dscale {
            if groups.iter().any(|digit| *digit != 0) {
                return Err("numeric_scale");
            }
            int_part.push('0');
            frac_part.push_str(&"0".repeat(dscale));
            return Ok((int_part, frac_part));
        }
        frac_part.push_str(&"0".repeat(lead_digits));
        for digit in groups {
            push_numeric_group(&mut frac_part, *digit, dscale, "numeric_scale")?;
        }
        int_part.push('0');
    } else {
        for (index, digit) in groups.iter().enumerate() {
            let group_weight = i32::from(weight) - i32::try_from(index).unwrap_or(i32::MAX);
            if group_weight >= 0 {
                push_numeric_group(
                    &mut int_part,
                    *digit,
                    CANONICAL_MAX_COEFFICIENT_DIGITS,
                    "numeric_precision",
                )?;
            } else {
                push_numeric_group(&mut frac_part, *digit, dscale, "numeric_scale")?;
            }
        }
        let count = i32::try_from(groups.len()).map_err(|_| "numeric_precision")?;
        let last = i32::from(weight) - count + 1;
        if last > 0 {
            let zeros = usize::try_from(last)
                .ok()
                .and_then(|value| value.checked_mul(4))
                .ok_or("numeric_precision")?;
            if int_part.len().saturating_add(zeros) > CANONICAL_MAX_COEFFICIENT_DIGITS {
                return Err("numeric_precision");
            }
            int_part.push_str(&"0".repeat(zeros));
        }
        if int_part.is_empty() {
            int_part.push('0');
        }
    }
    if frac_part.len() < dscale {
        let pad = dscale - frac_part.len();
        frac_part.push_str(&"0".repeat(pad));
    }
    if int_part.len() > CANONICAL_MAX_COEFFICIENT_DIGITS
        || frac_part.len() > CANONICAL_MAX_COEFFICIENT_DIGITS
    {
        return Err("numeric_precision");
    }
    Ok((int_part, frac_part))
}

fn push_numeric_group(
    out: &mut String,
    digit: u16,
    limit: usize,
    overflow: &'static str,
) -> Result<(), &'static str> {
    let text = group_digits(digit);
    if out.len() >= limit {
        if text.iter().any(|byte| *byte != b'0') {
            return Err(overflow);
        }
        return Ok(());
    }
    let room = limit - out.len();
    if room >= text.len() {
        push_ascii(out, &text);
        return Ok(());
    }
    if text[room..].iter().any(|byte| *byte != b'0') {
        return Err(overflow);
    }
    push_ascii(out, &text[..room]);
    Ok(())
}

fn push_ascii(out: &mut String, bytes: &[u8]) {
    for byte in bytes {
        out.push(char::from(*byte));
    }
}

fn group_digits(digit: u16) -> [u8; 4] {
    let mut text = [b'0'; 4];
    let mut value = u32::from(digit);
    for slot in text.iter_mut().rev() {
        *slot = b'0' + (value % 10) as u8;
        value /= 10;
    }
    text
}

fn canonical_int_text(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return false;
    }
    if text.starts_with('-') && digits == "0" {
        return false;
    }
    true
}

fn cell_bytes(cell: &Cell) -> u64 {
    match cell {
        Cell::Null => 0,
        Cell::Bool(true) => 4,
        Cell::Bool(false) => 5,
        Cell::Int(text) | Cell::Text(text) => u64::try_from(text.len()).unwrap_or(u64::MAX),
        Cell::Decimal(value) => u64::try_from(value.to_string().len()).unwrap_or(u64::MAX),
    }
}

fn stop_before_append(
    returned: u32,
    max_rows: u32,
    used: u64,
    row_bytes: u64,
    max_bytes: u64,
) -> bool {
    returned >= max_rows || used.saturating_add(row_bytes) > max_bytes
}

#[cfg(test)]
mod tests {
    #[test]
    fn nullable_source_watermark_does_not_invent_a_position() {
        assert_eq!(super::watermark_position(None), None);
        assert_eq!(super::watermark_position(Some("+12345678901234567890".into())), Some("+12345678901234567890".into()));
        assert_eq!(super::watermark_position(Some("-1".into())), None);
        assert_eq!(super::watermark_position(Some(String::new())), None);
    }

    fn maria_mark(lineage: &str, pos: &str) -> super::WatermarkObserved {
        super::WatermarkObserved {
            source: "logistics".into(),
            consistency: "transactional".into(),
            cross_table_consistent: true,
            applied_pos: pos.into(),
            state: "lagging".into(),
            codec: "mariadb-gtid/v1".into(),
            lineage: lineage.into(),
        }
    }

    fn maria_receipt(lineage: &str, pos: &str, epoch: i32) -> super::ReceiptObserved {
        super::ReceiptObserved {
            source: "logistics".into(),
            schema: "logistics".into(),
            table: "hubs".into(),
            run_generation: 1,
            applied_pos: pos.into(),
            codec: "mariadb-gtid/v1".into(),
            lineage: lineage.into(),
            source_epoch: epoch,
            cross_table_consistent: true,
            partial_txn: false,
            target_commit_at: String::new(),
        }
    }

    #[test]
    fn mariadb_watermark_requires_lineage_and_one_epoch() {
        assert!(!super::watermark_identity("mariadb-gtid/v1", "0-1-7", "7"));
        assert!(!super::watermark_identity("mariadb-gtid/v1", "d0.s1", "0-1-7"));
        assert!(super::watermark_identity("mariadb-gtid/v1", "d1.s1", "7"));
        assert!(!super::watermark_identity("mariadb-gtid/v1", "d0.s0", "7"));
        assert!(!super::watermark_identity("mariadb-gtid/v1", "d0.s2", "07"));
        assert!(super::watermark_identity("mariadb-gtid/v1", "d0.s1", "7"));
        assert!(super::watermark_identity("pg-commit-lsn/v1", "src_main", "10"));
        let mut same = super::QualitySnapshot::blank("read");
        same.watermarks.push(maria_mark("d0.s1", "7"));
        same.receipts.push(maria_receipt("d0.s1", "7", 1));
        same.receipts.push(maria_receipt("d1.s1", "7", 1));
        super::bind_maria_epoch(&mut same);
        assert_eq!(same.watermarks.len(), 1);
        assert_eq!(same.receipts.len(), 1);
        assert_eq!(same.receipts[0].lineage, "d0.s1");
        let mut other_server = super::QualitySnapshot::blank("read");
        other_server.watermarks.push(maria_mark("d0.s1", "7"));
        other_server.receipts.push(maria_receipt("d0.s2", "7", 1));
        super::bind_maria_epoch(&mut other_server);
        assert!(other_server.watermarks.is_empty());
        assert!(other_server.receipts.is_empty());
        let mut other_epoch = super::QualitySnapshot::blank("read");
        other_epoch.watermarks.push(maria_mark("d0.s1", "7"));
        other_epoch.receipts.push(maria_receipt("d0.s1", "7", 1));
        other_epoch.receipts.push(maria_receipt("d0.s1", "6", 2));
        super::bind_maria_epoch(&mut other_epoch);
        assert!(other_epoch.watermarks.is_empty());
        assert!(other_epoch.receipts.is_empty());
        let mut bare = super::QualitySnapshot::blank("read");
        bare.watermarks.push(maria_mark("d0.s1", "7"));
        super::bind_maria_epoch(&mut bare);
        assert!(bare.watermarks.is_empty());
    }
    #[test]
    fn quality_mapping_requires_the_vector_source() {
        let physical = format!("pt_{}", "a".repeat(60));
        let tables = vec![super::QualityTable {
            source: Some("source_east".into()), schema: "sales".into(), name: "orders".into(),
            physical_schema: "pn_test".into(), physical_name: physical.clone(),
        }];
        assert!(super::hold_mapping(&tables, "source_west".into(), "sales".into(), "orders".into(), 1, physical.clone()).is_none());
        assert!(super::hold_mapping(&tables, "source_east".into(), "sales".into(), "orders".into(), 1, physical).is_some());
    }
    use postgres_types::FromSql;
    use tokio_postgres::types::Type;

    use super::*;

    fn view(pid: i32, start: &str) -> BackendView {
        BackendView {
            pid,
            backend_start: start.to_string(),
            state: "active".to_string(),
        }
    }

    #[test]
    fn cancel_gate_matches_only_the_accepted_generation() {
        let seen = view(42, "20261005120000123456");
        assert_eq!(
            cancel_gate(3, 3, true, 42, "20261005120000123456", Some(&seen)),
            CancelGate::Send
        );
        assert_eq!(
            cancel_gate(3, 4, true, 42, "20261005120000123456", Some(&seen)),
            CancelGate::Refuse
        );
        assert_eq!(
            cancel_gate(3, 3, false, 42, "20261005120000123456", Some(&seen)),
            CancelGate::Refuse
        );
        assert_eq!(
            cancel_gate(3, 3, true, 0, "20261005120000123456", Some(&seen)),
            CancelGate::Refuse
        );
        assert_eq!(
            cancel_gate(3, 3, true, 42, "", Some(&seen)),
            CancelGate::Refuse
        );
        assert_eq!(
            cancel_gate(3, 3, true, 42, "20261005120000123456", None),
            CancelGate::Refuse
        );
        let other = view(42, "20261005120000999999");
        assert_eq!(
            cancel_gate(3, 3, true, 42, "20261005120000123456", Some(&other)),
            CancelGate::Refuse
        );
    }

    #[test]
    fn role_bind_is_permanent_for_a_valid_role_only() {
        assert_eq!(
            role_bind_decision(None, "oo_r_reader_a", "oo_r_reader_a"),
            RoleBind::Proceed
        );
        assert_eq!(role_bind_decision(None, "nope", "nope"), RoleBind::Invalid);
        assert_eq!(
            role_bind_decision(None, "oo_r_reader_a", "oo_r_reader_b"),
            RoleBind::Invalid
        );
        assert_eq!(
            role_bind_decision(None, "oo_r_", "oo_r_"),
            RoleBind::Invalid
        );
        assert_eq!(
            role_bind_decision(Some("oo_r_reader_a"), "oo_r_reader_a", "oo_r_reader_a"),
            RoleBind::Proceed
        );
        assert_eq!(
            role_bind_decision(Some("oo_r_reader_a"), "oo_r_reader_b", "oo_r_reader_b"),
            RoleBind::Reuse
        );
        assert_eq!(
            role_bind_decision(Some("oo_r_reader_a"), "nope", "nope"),
            RoleBind::Invalid
        );
        assert_eq!(CLEANUP_BOUND, Duration::from_millis(750));
        assert!(CLEANUP_BOUND < Duration::from_secs(1));
        assert!(CLEANUP_BOUND < MAX_QUERY_WALL);
    }

    #[test]
    fn cleanup_budget_does_not_grow_after_it_starts() {
        let start = Instant::now();
        let deadline = start + CLEANUP_BOUND;
        assert_eq!(budget_left(deadline, start), CLEANUP_BOUND);
        assert_eq!(
            budget_left(deadline, start + Duration::from_millis(200)),
            Duration::from_millis(550)
        );
        assert_eq!(
            budget_left(deadline, start + Duration::from_millis(500)),
            Duration::from_millis(250)
        );
        assert_eq!(budget_left(deadline, start + CLEANUP_BOUND), Duration::ZERO);
        assert_eq!(
            budget_left(deadline, start + Duration::from_millis(900)),
            Duration::ZERO
        );
    }

    #[test]
    fn may_lend_requires_an_open_unfenced_session() {
        assert!(!may_lend(true, true, false));
        assert!(!may_lend(true, false, true));
        assert!(!may_lend(false, false, false));
        assert!(may_lend(true, false, false));
    }

    #[test]
    fn role_shape_accepts_only_the_nologin_prefix() {
        assert!(role_shape_ok("oo_r_reader_a"));
        assert_eq!(
            quote_role("oo_r_reader_a").as_deref(),
            Ok("\"oo_r_reader_a\"")
        );
        assert!(!role_shape_ok("reader_a"));
        assert!(!role_shape_ok("oo_r_"));
        assert!(!role_shape_ok("oo_r_Reader"));
        assert!(!role_shape_ok("public"));
        assert!(!role_shape_ok("pg_read_all_stats"));
        let long = format!("oo_r_{}", "a".repeat(59));
        assert!(!role_shape_ok(&long));
        assert!(quote_role("nope").is_err());
    }

    #[test]
    fn byte_cap_is_applied_before_append_and_integers_are_canonical() {
        assert!(!stop_before_append(0, 2, 0, 4, 4));
        assert!(stop_before_append(1, 2, 4, 1, 4));
        assert!(stop_before_append(2, 2, 0, 1, 100));
        assert!(stop_before_append(0, 2, 0, 5, 4));
        assert_eq!(cell_bytes(&Cell::Null), 0);
        assert_eq!(cell_bytes(&Cell::Bool(true)), 4);
        assert_eq!(cell_bytes(&Cell::Bool(false)), 5);
        assert_eq!(i16::MIN.to_string(), "-32768");
        assert_eq!(i64::MIN.to_string(), "-9223372036854775808");
        assert!(canonical_int_text(&i64::MIN.to_string()));
        assert!(canonical_int_text(&i16::MIN.to_string()));
        assert!(!canonical_int_text("01"));
        assert!(!canonical_int_text("+1"));
        assert!(!canonical_int_text("-"));
        assert!(
            matches!(int_cell(Some(i64::MIN)), Ok(Cell::Int(text)) if text == "-9223372036854775808")
        );
    }

    #[test]
    fn bootstrap_oids_match_the_driver_and_other_types_are_refused() {
        assert_eq!(Type::BOOL.oid(), OID_BOOL);
        assert_eq!(Type::INT8.oid(), OID_INT8);
        assert_eq!(Type::INT2.oid(), OID_INT2);
        assert_eq!(Type::INT4.oid(), OID_INT4);
        assert_eq!(Type::TEXT.oid(), OID_TEXT);
        assert_eq!(Type::NUMERIC.oid(), OID_NUMERIC);
        assert!(oid_allowed(Type::BOOL.oid()));
        assert!(oid_allowed(Type::INT8.oid()));
        assert!(oid_allowed(Type::NUMERIC.oid()));
        assert!(oid_allowed(Type::VARCHAR.oid()));
        assert!(oid_allowed(Type::FLOAT8.oid()));
        assert!(oid_allowed(Type::DATE.oid()));
        assert!(oid_allowed(Type::TIME.oid()));
        assert!(oid_allowed(Type::INTERVAL.oid()));
        let mut negative_interval = [0u8; 16];
        negative_interval[..8].copy_from_slice(&(-3_600_000_000_i64).to_be_bytes());
        assert_eq!(format_payload(OID_INTERVAL, &negative_interval).unwrap(), Cell::Text("-01:00:00.000000".into()));
        negative_interval[15] = 1;
        assert!(format_payload(OID_INTERVAL, &negative_interval).is_err());
        assert_eq!(
            format_payload(OID_TIME, &45_296_123_456_i64.to_be_bytes()).unwrap(),
            Cell::Text("12:34:56.123456".to_owned())
        );
        assert_eq!(
            format_payload(OID_TIME, &86_400_000_000_i64.to_be_bytes()).unwrap(),
            Cell::Text("24:00:00.000000".to_owned())
        );
        assert!(format_payload(OID_TIME, &86_400_000_001_i64.to_be_bytes()).is_err());
        assert!(oid_allowed(Type::JSON.oid()));
        assert!(oid_allowed(Type::JSONB.oid()));
        assert!(oid_allowed(Type::BYTEA.oid()));
        assert!(!oid_allowed(Type::FLOAT4.oid()));
        assert!(!oid_allowed(705));
        assert_eq!(
            format_payload(OID_DATE, &0_i32.to_be_bytes()),
            Ok(Cell::Text("2000-01-01".to_owned()))
        );
        assert_eq!(
            format_payload(OID_BYTEA, &[0x00, 0xff]),
            Ok(Cell::Text("00ff".to_owned()))
        );
        assert_eq!(
            format_payload(OID_JSON, br#"{"a":1}"#),
            Ok(Cell::Text("{\"a\":1}".to_owned()))
        );
        assert_eq!(
            format_payload(OID_JSONB, b"\x01{\"b\":true}"),
            Ok(Cell::Text("{\"b\":true}".to_owned()))
        );
        assert!(format_payload(OID_FLOAT8, &f64::NAN.to_be_bytes()).is_err());
        assert_eq!(
            format_payload(OID_FLOAT8, &1.5_f64.to_be_bytes()),
            Ok(Cell::Text("1.5".to_owned()))
        );
        assert_eq!(
            stop_class(&ExecError::TypeRefused {
                oid: Type::FLOAT4.oid()
            }),
            StopClass::TypeRefused
        );
        assert_eq!(
            stop_class(&ExecError::Protocol {
                step: "numeric_nan"
            }),
            StopClass::Protocol
        );
        assert_eq!(PORTAL_PAGE_ROWS, 1);
        assert_eq!(UNTRUSTED_ADMISSION, UntrustedAdmission::Blocked);
    }

    #[test]
    fn error_codes_use_the_existing_enum_and_hide_server_text() {
        assert_eq!(
            ExecError::AuditUnavailable.code(),
            ErrorCode::AuditUnavailable
        );
        assert_eq!(
            ExecError::Timeout { sqlstate: None }.code(),
            ErrorCode::Timeout
        );
        assert_eq!(
            ExecError::Canceled {
                sqlstate: "57014".to_string()
            }
            .code(),
            ErrorCode::Timeout
        );
        assert_eq!(
            ExecError::TypeRefused { oid: 1700 }.code(),
            ErrorCode::StatementNotAllowed
        );
        assert_eq!(ExecError::RoleParameter.code(), ErrorCode::SchemaInvalid);
        assert_eq!(ExecError::Limit.code(), ErrorCode::SchemaInvalid);
        assert_eq!(ExecError::RoleReuse.code(), ErrorCode::SchemaInvalid);
        assert_eq!(ExecError::SessionClosed.code(), ErrorCode::SchemaInvalid);
        assert_eq!(ExecError::SessionBusy.code(), ErrorCode::SchemaInvalid);
        assert_eq!(
            ExecError::Protocol { step: "bind" }.code(),
            ErrorCode::Internal
        );
        let database = ExecError::Database {
            sqlstate: "42501".to_string(),
        };
        assert_eq!(database.code(), ErrorCode::Internal);
        let display = database.to_string();
        assert!(!display.contains("secret"));
        assert!(!display.contains("password"));
        assert_eq!(database.sqlstate(), Some("42501"));
        assert!(checked_wall(Duration::from_millis(0)).is_err());
        assert!(checked_wall(Duration::from_millis(30_001)).is_err());
        assert!(checked_wall(Duration::from_millis(30_000)).is_ok());
        assert!(checked_wall(Duration::from_nanos(1_500_000)).is_err());
    }

    fn reservation_shared(
        generation: u64,
        reserved: Option<u64>,
    ) -> (Arc<Mutex<Shared>>, watch::Receiver<bool>) {
        let (destroy, rx) = watch::channel(false);
        let shared = Arc::new(Mutex::new(Shared {
            generation,
            accepting_cancel: true,
            backend_pid: 11,
            backend_start: "20261006120000111111".to_string(),
            bound_role: None,
            open: true,
            summary: None,
            cancel_reserved: reserved,
            cancel_packet_sent: false,
            cleanup_deadline: None,
            cancel_notify: Arc::new(Notify::new()),
            destroy,
            result_columns: Vec::new(),
            quality: QualitySnapshot::skipped(),
        }));
        (shared, rx)
    }

    #[test]
    fn reservation_guard_drop_before_transport_releases_only_its_generation() {
        let (shared, _rx) = reservation_shared(4, None);
        assert!(try_reserve_cancel(&shared, 4));
        drop(ReservationGuard::arm(Arc::clone(&shared), 9));
        assert_eq!(lock(&shared).cancel_reserved, Some(4));
        assert!(lock(&shared).open);
        drop(ReservationGuard::arm(Arc::clone(&shared), 4));
        assert!(lock(&shared).cancel_reserved.is_none());
        assert!(lock(&shared).open);
        assert!(lock(&shared).accepting_cancel);
        assert!(try_reserve_cancel(&shared, 4));
    }

    #[test]
    fn reservation_guard_drop_after_transport_fail_closes_only_its_generation() {
        let (shared, _rx) = reservation_shared(4, None);
        assert!(try_reserve_cancel(&shared, 4));
        {
            let mut foreign = ReservationGuard::arm(Arc::clone(&shared), 9);
            foreign.enter_transport();
        }
        assert_eq!(lock(&shared).cancel_reserved, Some(4));
        assert!(lock(&shared).open);
        {
            let mut owner = ReservationGuard::arm(Arc::clone(&shared), 4);
            owner.enter_transport();
            owner.disarm();
        }
        assert_eq!(lock(&shared).cancel_reserved, Some(4));
        assert!(lock(&shared).open);
        {
            let mut owner = ReservationGuard::arm(Arc::clone(&shared), 4);
            owner.enter_transport();
        }
        let guard = lock(&shared);
        assert!(guard.cancel_reserved.is_none());
        assert!(!guard.open);
        assert!(!guard.accepting_cancel);
        assert!(*guard.destroy.borrow());
    }

    #[test]
    fn producer_leftover_reservation_fail_closes_and_an_empty_one_does_not() {
        let (clear, _clear_rx) = reservation_shared(2, None);
        close_leftover_reservation(&clear);
        assert!(lock(&clear).open);
        assert!(lock(&clear).cancel_reserved.is_none());
        let (held, _held_rx) = reservation_shared(2, Some(2));
        close_leftover_reservation(&held);
        let guard = lock(&held);
        assert!(!guard.open);
        assert!(!guard.accepting_cancel);
        assert!(guard.cancel_reserved.is_none());
        assert!(*guard.destroy.borrow());
    }

    fn numeric_wire(weight: i16, sign: u16, dscale: i16, digits: &[u16]) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&i16::try_from(digits.len()).unwrap().to_be_bytes());
        raw.extend_from_slice(&weight.to_be_bytes());
        raw.extend_from_slice(&sign.to_be_bytes());
        raw.extend_from_slice(&dscale.to_be_bytes());
        for digit in digits {
            raw.extend_from_slice(&i16::try_from(*digit).unwrap().to_be_bytes());
        }
        raw
    }

    fn numeric_typmod(precision: i32, scale: i32) -> i32 {
        ((precision << 16) | (scale & 0x7FF)) + 4
    }

    #[test]
    fn numeric_wire_matches_the_reviewed_vectors() {
        let wide = decode_numeric_payload(&numeric_wire(
            4,
            NUMERIC_SIGN_POS,
            2,
            &[100, 0, 0, 0, 0, 100],
        ))
        .expect("wide");
        assert_eq!(wide.to_string(), "1000000000000000000.01");
        assert_eq!(wide.scale(), 2);
        assert!(!wide.is_negative());
        let one =
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_POS, 2, &[1, 100])).expect("1.01");
        assert_eq!(one.to_string(), "1.01");
        let cent =
            decode_numeric_payload(&numeric_wire(-1, NUMERIC_SIGN_POS, 2, &[100])).expect("0.01");
        assert_eq!(cent.to_string(), "0.01");
        let zero =
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_POS, 2, &[])).expect("zero");
        assert_eq!(zero.to_string(), "0.00");
        assert!(!zero.is_negative());
        let neg = decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_NEG, 2, &[1, 100]))
            .expect("-1.01");
        assert_eq!(neg.to_string(), "-1.01");
        let tiny =
            decode_numeric_payload(&numeric_wire(-2, NUMERIC_SIGN_POS, 6, &[100])).expect("tiny");
        assert_eq!(tiny.to_string(), "0.000001");
        let padded =
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_POS, 4, &[1])).expect("pad");
        assert_eq!(padded.to_string(), "1.0000");
        assert_eq!(padded.coefficient(), "10000");
        assert_eq!(padded.scale(), 4);
    }

    #[test]
    fn numeric_hand_literals_keep_scale_and_reject_specials() {
        let positive = decode_numeric_payload(&numeric_wire(
            3,
            NUMERIC_SIGN_POS,
            4,
            &[12, 3456, 7890, 1234, 5678],
        ))
        .expect("positive");
        assert!(!positive.is_negative());
        assert_eq!(positive.coefficient(), "123456789012345678");
        assert_eq!(positive.scale(), 4);
        assert_eq!(positive.to_string(), "12345678901234.5678");
        let negative = decode_numeric_payload(&numeric_wire(
            3,
            NUMERIC_SIGN_NEG,
            4,
            &[12, 3456, 7890, 1234, 5678],
        ))
        .expect("negative");
        assert!(negative.is_negative());
        assert_eq!(negative.coefficient(), "123456789012345678");
        assert_eq!(negative.to_string(), "-12345678901234.5678");
        let small =
            decode_numeric_payload(&numeric_wire(-1, NUMERIC_SIGN_POS, 4, &[1])).expect("small");
        assert_eq!(small.coefficient(), "1");
        assert_eq!(small.scale(), 4);
        assert_eq!(small.to_string(), "0.0001");
        let small_neg = decode_numeric_payload(&numeric_wire(-1, NUMERIC_SIGN_NEG, 4, &[1]))
            .expect("small neg");
        assert!(small_neg.is_negative());
        assert_eq!(small_neg.coefficient(), "1");
        assert_eq!(small_neg.to_string(), "-0.0001");
        let zeros =
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_POS, 4, &[])).expect("zeros");
        assert!(!zeros.is_negative());
        assert_eq!(zeros.coefficient(), "0");
        assert_eq!(zeros.scale(), 4);
        assert_eq!(zeros.to_string(), "0.0000");
        let neg_zero =
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_NEG, 4, &[])).expect("neg zero");
        assert!(!neg_zero.is_negative());
        assert_eq!(neg_zero.coefficient(), "0");
        assert_eq!(
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_POS, 1, &[1, 100])),
            Err("numeric_scale")
        );
        assert_eq!(
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_NAN, 0, &[])),
            Err("numeric_nan")
        );
        assert_eq!(
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_PINF, 0, &[])),
            Err("numeric_infinity")
        );
        assert_eq!(
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_NINF, 0, &[])),
            Err("numeric_neg_infinity")
        );
        assert_eq!(
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_POS, 0, &[10_000])),
            Err("numeric_payload")
        );
        assert_eq!(decode_numeric_payload(&[0, 1]), Err("numeric_payload"));
        let mut negative_count = numeric_wire(0, NUMERIC_SIGN_POS, 0, &[]);
        negative_count[0] = 0xFF;
        negative_count[1] = 0xFF;
        assert_eq!(
            decode_numeric_payload(&negative_count),
            Err("numeric_payload")
        );
        assert_eq!(
            decode_numeric_payload(&numeric_wire(0, 0x0001, 0, &[])),
            Err("numeric_payload")
        );
    }

    #[test]
    fn numeric_typmod_checks_scale_and_coefficient_length() {
        assert_eq!(numeric_typmod(18, 4), 1_179_656);
        let amount = decode_numeric_payload(&numeric_wire(
            3,
            NUMERIC_SIGN_POS,
            4,
            &[12, 3456, 7890, 1234, 5678],
        ))
        .expect("amount");
        let admitted = apply_numeric_typmod(amount, 1_179_656).expect("18,4");
        assert_eq!(admitted.coefficient(), "123456789012345678");
        let narrow =
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_POS, 4, &[1])).expect("pad");
        assert!(apply_numeric_typmod(narrow, numeric_typmod(4, 4)).is_err());
        let fraction = decode_numeric_payload(&numeric_wire(-1, NUMERIC_SIGN_POS, 6, &[12, 3400]))
            .expect("fraction");
        assert_eq!(fraction.coefficient(), "1234");
        assert_eq!(fraction.scale(), 6);
        assert_eq!(fraction.to_string(), "0.001234");
        assert!(fraction.storage_precision() > 4);
        let kept =
            apply_numeric_typmod(fraction.clone(), numeric_typmod(4, 6)).expect("coeff width");
        assert_eq!(kept.coefficient(), "1234");
        assert!(apply_numeric_typmod(fraction, numeric_typmod(4, 4)).is_err());
        let unconstrained =
            decode_numeric_payload(&numeric_wire(0, NUMERIC_SIGN_POS, 2, &[1, 100])).expect("free");
        assert!(apply_numeric_typmod(unconstrained, -1).is_ok());
        assert_eq!(
            decode_numeric_payload(&numeric_wire(2_048, NUMERIC_SIGN_POS, 0, &[1])),
            Err("numeric_precision")
        );
        let budget = decode_numeric_payload(&numeric_wire(2_047, NUMERIC_SIGN_POS, 4, &[1]));
        assert_eq!(budget, Err("numeric_precision"));
        let nan =
            PgNumericCell::from_sql(&Type::NUMERIC, &numeric_wire(0, NUMERIC_SIGN_NAN, 0, &[]));
        let err = nan.expect_err("nan");
        assert_eq!(
            err.downcast_ref::<NumericReject>().map(|item| item.step),
            Some("numeric_nan")
        );
        assert!(!PgNumericCell::accepts(&Type::FLOAT8));
        assert!(PgNumericCell::accepts(&Type::NUMERIC));
        let cell = Cell::Decimal(ExactDecimal::new(false, "10000", 4).expect("cell"));
        assert_eq!(cell_bytes(&cell), 6);
        let negative_cell = Cell::Decimal(ExactDecimal::new(true, "101", 2).expect("neg"));
        assert_eq!(cell_bytes(&negative_cell), 5);
    }
}
