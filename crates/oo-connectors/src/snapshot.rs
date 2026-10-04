//! Non-reusable PostgreSQL snapshot owner. Protocol `oo.source.pgsql.snapshot/v1`.
//!
//! One owner holds one data connection and one current-catalog connection.
//! The data connection has one `REPEATABLE READ READ ONLY` transaction.
//! The current-catalog connection has one `READ COMMITTED READ ONLY` transaction.
//! [`PgSession::catalog`] is not this snapshot: that method begins and rolls back
//! on its own. This owner calls `collect_catalog` inside the data transaction.
//!
//! `read_probe` is one primary-key lookup with `LIMIT 2`. It is not a table scan.
//! `open_stream` / `next_batch` pull a full-snapshot Arrow stream on this same
//! owner and transaction (`oo.source.pgsql.stream/v1`). `plan_keys`, `open_chunk`,
//! and `read_keys` plan and read typed half-open ranges on that same transaction.
//! That pull is not a second snapshot and not the whole reader. Resume cursors
//! stay unimplemented. `cancel` asks the guard session to cancel the data backend
//! and then rolls back both sessions. Dropping `next_batch` restores the drive
//! and does not by itself end the backend. `finish` is lifecycle release.
//! `SnapshotFinished` is not admission. `finish_complete` returns `RunComplete`
//! only after this owner delivered `StreamItem::Exhausted` for every selected table.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sha2::{Digest, Sha256};
use sqlx::postgres::{PgArguments, PgValueFormat};
use sqlx::{Arguments, Row, ValueRef};

use crate::config::{PgSourceConfig, SelectedTable};
use crate::error::SourceError;
use crate::model::{SourceCatalog, SourceColumn, SourceTable};
use crate::session::{self, PgConnector, PgSession};
use crate::sql;
use crate::types::{
    assemble_batch, decode_payload, guard_plans, map_column, valid_quoted_ident, ColumnPlan,
    DecodedCell, MapDecision, PgTypePolicy, RejectCellPolicy, RunBinding, TypeFacts, TypeGraph,
    TypedBatch, UnsupportedColumnPolicy,
};

/// Snapshot owner protocol. This is not `oo.source.pgsql/v1` and not the whole reader.
pub const SNAPSHOT_PROTOCOL: &str = "oo.source.pgsql.snapshot/v1";

/// How long a caller may poll `pg_stat_activity` after finish, abort, cancel, drop, or failure.
/// Absence of the `(pid, backend_start)` row is the release observation.
/// The same bound is how long an in-flight Execute may take to leave the backend
/// after the guard runs `pg_cancel_backend`. Past that bound the pull is
/// `cleanup_unconfirmed` and the socket is closed without a confirmed `ROLLBACK`.
pub const CLEANUP_OBSERVATION_BOUND_MS: u64 = 5_000;

pub(crate) const OID_BOOL: i64 = 16;
pub(crate) const OID_INT8: i64 = 20;
pub(crate) const OID_INT2: i64 = 21;
pub(crate) const OID_INT4: i64 = 23;
pub(crate) const OID_TEXT: i64 = 25;
pub(crate) const OID_BPCHAR: i64 = 1042;
pub(crate) const OID_VARCHAR: i64 = 1043;
const MAX_KEY_COLUMNS: usize = 8;
const INDEX_MAX_KEYS: usize = 32;

static GENERATIONS: Mutex<BTreeSet<(String, u64)>> = Mutex::new(BTreeSet::new());

/// Column shape the caller says it selected. The owner re-reads the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittedColumn {
    /// `pg_attribute.attnum`.
    pub attnum: i32,
    /// Original column name.
    pub name: String,
    /// `pg_attribute.atttypid`.
    pub type_oid: i64,
    /// `pg_attribute.atttypmod`.
    pub typmod: i32,
    /// `pg_attribute.attnotnull`.
    pub not_null: bool,
}

/// One selected table the caller says it bound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittedTable {
    /// Schema name.
    pub schema: String,
    /// Table name.
    pub name: String,
    /// Columns compared by attnum, name, type oid, typmod, and nullability.
    pub columns: Vec<AdmittedColumn>,
    /// Primary-key column names in key order.
    pub primary_key: Vec<String>,
}

/// Admitted catalog identity for this run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittedCatalog {
    /// Selected tables. The set must match the source configuration.
    pub tables: Vec<AdmittedTable>,
}

/// One bound primary-key value. R1 accepts int2, int4, int8, bool, and text types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeValue {
    /// int2, int4, or int8. The owner narrows to the catalog oid.
    Int8(i64),
    /// bool.
    Bool(bool),
    /// text, varchar, or bpchar.
    Text(String),
}

/// One primary-key lookup inside the open snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutProbe {
    /// Schema name from the bound catalog.
    pub schema: String,
    /// Table name from the bound catalog.
    pub table: String,
    /// Key values in primary-key order.
    pub key: Vec<ProbeValue>,
}

/// Input to [`SnapshotOwner::open`]. The password stays on `config` and is not stored.
#[derive(Clone, Debug)]
pub struct SnapshotRequest {
    /// Admitted source configuration. `connect` applies the existing TLS and version boundary.
    pub config: PgSourceConfig,
    /// Caller catalog shape. Compared with the catalog read inside this transaction.
    pub admitted: AdmittedCatalog,
    /// Output of [`config_identity`] for `config`.
    pub config_identity: [u8; 32],
    /// Output of [`type_policy_identity`] for `policy`.
    pub type_policy_identity: [u8; 32],
    /// Type policy passed to the existing plan guard.
    pub policy: PgTypePolicy,
    /// Run generation. Zero is rejected. A consumed generation cannot be opened again.
    pub generation: u64,
    /// Deadline checked before each owner step. It does not cancel a statement already sent.
    pub deadline: Instant,
    /// Expected `session_user` and `current_user`.
    pub expected_principal: String,
    /// Expected `current_database`.
    pub expected_database: String,
    /// Expected `pg_is_in_recovery()`.
    pub expected_in_recovery: bool,
    /// Expected `pg_postmaster_start_time()::text` from a session with the same `TimeZone`.
    pub expected_postmaster_start: String,
}

/// `pg_stat_activity` identity of the owner's backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendKey {
    /// `pg_backend_pid()`.
    pub pid: i64,
    /// `backend_start::text`.
    pub backend_start: String,
}

/// Privilege answers captured inside the snapshot. A later statement can differ.
/// PostgreSQL does not freeze ACL checks in the MVCC snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableAccess {
    /// Schema name.
    pub schema: String,
    /// Table name.
    pub name: String,
    /// `pg_class.oid` of the bound relation.
    pub relation_oid: i64,
    /// `has_table_privilege(..., 'SELECT')` for this statement.
    pub select: Option<bool>,
    /// `INSERT` for this statement.
    pub insert: Option<bool>,
    /// `UPDATE` for this statement.
    pub update: Option<bool>,
    /// `DELETE` for this statement.
    pub delete: Option<bool>,
    /// `TRUNCATE` for this statement.
    pub truncate: Option<bool>,
}

/// Public cut identity. It does not include the connection or a password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotBinding {
    /// [`SNAPSHOT_PROTOCOL`].
    pub protocol: &'static str,
    /// Generation reserved for this open.
    pub generation: u64,
    /// Source id.
    pub source_id: String,
    /// Database name read inside the transaction.
    pub database: String,
    /// Principal read inside the transaction.
    pub principal: String,
    /// `pg_is_in_recovery()` inside the transaction.
    pub in_recovery: bool,
    /// `pg_postmaster_start_time()::text` inside the transaction.
    pub postmaster_start: String,
    /// `pg-catalog-hash/v1` of the catalog captured in this transaction.
    pub schema_hash: String,
    /// Config identity checked before connect.
    pub config_identity: [u8; 32],
    /// Type-policy identity checked before connect.
    pub type_policy_identity: [u8; 32],
    /// Digest of the config identity, policy identity, schema hash, and generation.
    pub input_digest: [u8; 32],
    /// Backend key.
    pub backend: BackendKey,
    /// Catalog captured inside this transaction.
    pub catalog: SourceCatalog,
    /// Table privilege answers captured inside this transaction.
    pub access: Vec<TableAccess>,
}

/// Lifecycle release after `ROLLBACK` is accepted and both connections close.
/// This is not admission. It does not mean every selected table reached
/// [`crate::StreamItem::Exhausted`]. No production target admits a load from
/// this value. Callers outside this crate cannot construct it.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotFinished {
    /// Generation that was consumed.
    pub generation: u64,
    /// Source id.
    pub source_id: String,
    /// Backend that was closed.
    pub backend: BackendKey,
}

/// Admission that this owner delivered [`crate::StreamItem::Exhausted`] for
/// every selected table and then passed the release checks.
///
/// `SnapshotFinished` is not this value. A successful rollback is not this
/// value. Callers cannot construct it: the seal is private to the snapshot
/// owner, and there is no public constructor. No production target admits a
/// load yet. Only this value may admit an eventual loader.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunComplete {
    /// Source id captured for this owner.
    pub source_id: String,
    /// Generation that was consumed.
    pub generation: u64,
    /// [`config_identity`] of the configuration bound at open.
    pub config_identity: [u8; 32],
    /// [`type_policy_identity`] of the policy bound at open.
    pub type_policy_identity: [u8; 32],
    /// Type policy bound at open.
    pub policy: PgTypePolicy,
    /// Digest of the config identity, policy identity, schema hash, and generation.
    pub input_digest: [u8; 32],
    /// `pg-catalog-hash/v1` of the catalog captured in this transaction.
    pub schema_hash: String,
    /// Catalog captured inside the snapshot transaction. This is not a second read.
    pub catalog: SourceCatalog,
    /// Selected tables in owner order.
    pub selected: Vec<SelectedTable>,
    seal: CompleteSeal,
}

/// Private proof that [`RunComplete`] was minted by [`SnapshotOwner::finish_complete`].
#[derive(Clone, Debug, PartialEq, Eq)]
struct CompleteSeal;

/// Returned only after an explicit abort confirms `ROLLBACK` and close.
/// This is not [`SnapshotFinished`]. Callers outside this crate cannot construct it.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotAborted {
    /// Generation that was consumed.
    pub generation: u64,
    /// Source id.
    pub source_id: String,
    /// Backend that was closed.
    pub backend: BackendKey,
}

/// Caller handle for the active pull. [`RunCancel::request`] only sets the flag.
/// It does not roll back and it does not prove the backend has exited.
#[derive(Clone, Debug)]
pub struct RunCancel {
    flag: Arc<AtomicBool>,
}

impl RunCancel {
    /// Ask the in-flight Execute to stop. The owner still has to be cancelled,
    /// finished, aborted, or dropped before the backends are released.
    pub fn request(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }
}

/// Returned after `cancel` when both sessions rolled back or closed and the
/// pull did not end `cleanup_unconfirmed`. This is not [`SnapshotFinished`].
/// The generation's output stays incomplete. Callers outside this crate cannot construct it.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunCancelled {
    /// Generation that was consumed.
    pub generation: u64,
    /// Source id.
    pub source_id: String,
    /// Data backend that was closed.
    pub backend: BackendKey,
}

struct OpenPlan {
    admitted: AdmittedCatalog,
    config_identity: [u8; 32],
    type_policy_identity: [u8; 32],
    policy: PgTypePolicy,
    generation: u64,
    deadline: Instant,
    expected_principal: String,
    expected_database: String,
    expected_in_recovery: bool,
    expected_postmaster_start: String,
    username: String,
    database: String,
    source_id: String,
}

struct NodeFact {
    in_recovery: bool,
    postmaster_start: String,
    database: String,
    session_user: String,
    current_user: String,
}

struct RelationFact {
    oid: i64,
    relkind: String,
    rls: bool,
    forced: bool,
    select: Option<bool>,
    insert: Option<bool>,
    update: Option<bool>,
    delete: Option<bool>,
    truncate: Option<bool>,
}

struct AttrFact {
    attnum: i32,
    name: String,
    type_oid: i64,
    typmod: i32,
    not_null: bool,
    select: Option<bool>,
}

struct BoundTable {
    schema: String,
    name: String,
    schema_sql: String,
    table_sql: String,
    select_sql: Vec<String>,
    key_sql: Vec<String>,
    key_oids: Vec<i64>,
    key_attnums: Vec<i32>,
    plans: Vec<ColumnPlan>,
}

struct Captured {
    node: NodeFact,
    catalog: SourceCatalog,
    access: Vec<TableAccess>,
    tables: Vec<BoundTable>,
    pid: i64,
    backend_start: String,
}

/// SHA-256 of the non-secret configuration fields.
/// The password and the CA PEM are not in the preimage.
pub fn config_identity(config: &PgSourceConfig) -> [u8; 32] {
    let mut body = String::from("oo-pg-config-identity/v1\n");
    push_line(&mut body, "source_id", config.source_id().as_str());
    push_line(&mut body, "host", config.endpoint().host());
    push_line(&mut body, "port", &config.endpoint().port().to_string());
    push_line(&mut body, "database", config.endpoint().database());
    push_line(&mut body, "username", config.credentials().username());
    push_line(&mut body, "secret_ref", config.secret_ref().as_str());
    let budget = config.budget();
    push_line(
        &mut body,
        "connect_ms",
        &budget.connect_timeout().as_millis().to_string(),
    );
    push_line(
        &mut body,
        "statement_ms",
        &budget.statement_timeout().as_millis().to_string(),
    );
    push_line(
        &mut body,
        "lock_ms",
        &budget.lock_timeout().as_millis().to_string(),
    );
    push_line(&mut body, "max_rows", &budget.max_rows().to_string());
    push_line(&mut body, "max_bytes", &budget.max_bytes().to_string());
    for table in config.selection().tables() {
        body.push_str("table\t");
        body.push_str(table.schema());
        body.push('\t');
        body.push_str(table.table());
        body.push('\n');
    }
    digest(body.as_bytes())
}

/// SHA-256 of the type-policy enum names.
pub fn type_policy_identity(policy: PgTypePolicy) -> [u8; 32] {
    let unsupported = match policy.unsupported {
        UnsupportedColumnPolicy::PreserveRow => "preserve_row",
        UnsupportedColumnPolicy::FailRead => "fail_read",
    };
    let reject = match policy.reject {
        RejectCellPolicy::PreserveRow => "preserve_row",
    };
    let body = format!("oo-pg-type-policy-identity/v1\nunsupported={unsupported}\nreject={reject}\n");
    digest(body.as_bytes())
}

/// Digest bound into [`RunBinding`] for rows read from this cut.
pub fn snapshot_input_digest(
    config_identity: &[u8; 32],
    type_policy_identity: &[u8; 32],
    schema_hash_hex: &str,
    generation: u64,
) -> Result<[u8; 32], SourceError> {
    let hash = decode_hex(schema_hash_hex).ok_or_else(|| SourceError::catalog("catalog_identity"))?;
    let mut hasher = Sha256::new();
    hasher.update(config_identity);
    hasher.update(type_policy_identity);
    hasher.update(hash);
    hasher.update(generation.to_le_bytes());
    Ok(hasher.finalize().into())
}

/// Lifetime-bound snapshot. Drop rolls back and closes when a Tokio runtime is
/// current, and otherwise drops both sockets. Drop does not return [`SnapshotFinished`].
pub struct SnapshotOwner {
    session: Option<PgSession>,
    guard: Option<PgSession>,
    binding: SnapshotBinding,
    tables: Vec<BoundTable>,
    policy: PgTypePolicy,
    deadline: Instant,
    poison: Option<SourceError>,
    released: bool,
    stream: Option<crate::stream::StreamSlot>,
    cancel: Arc<AtomicBool>,
    /// Set only by the pull when it delivers [`crate::StreamItem::Exhausted`]
    /// for every selected table. Nothing in the test crate can store this.
    exhausted_delivered: Arc<AtomicBool>,
}

impl SnapshotOwner {
    /// Reserve `generation`, open the data session and the current-catalog session,
    /// and begin one read-only repeatable-read transaction on the data session.
    ///
    /// A generation of zero, a stale deadline, or an identity mismatch is refused
    /// before a connection slot is taken and before the generation is reserved.
    /// Cap refusal does not consume the generation. A failure after reservation consumes
    /// the generation. The owner is returned only while both transactions are open.
    pub async fn open(request: SnapshotRequest) -> Result<Self, SourceError> {
        let plan = open_plan(&request)?;
        let mut permits = session::try_reserve(&plan.source_id, 2)?;
        if let Err(err) = reserve(&plan.source_id, plan.generation) {
            drop(permits);
            return Err(err);
        }
        let data_permit = match permits.pop() {
            Some(permit) => permit,
            None => {
                drop(permits);
                return Err(SourceError::budget("max_connections"));
            }
        };
        let guard_permit = match permits.pop() {
            Some(permit) => permit,
            None => {
                drop(data_permit);
                drop(permits);
                return Err(SourceError::budget("max_connections"));
            }
        };
        drop(permits);
        let connector = PgConnector::new(request.config);
        let mut data = match connector.connect_reserved(data_permit).await {
            Ok(session) => session,
            Err(err) => {
                drop(guard_permit);
                return Err(err);
            }
        };
        if let Err(err) = admit_connected(&data, &plan) {
            drop(guard_permit);
            return Err(fail_open(data, err).await);
        }
        let mut guard = match connector.connect_reserved(guard_permit).await {
            Ok(session) => session,
            Err(err) => return Err(fail_open(data, err).await),
        };
        if let Err(err) = admit_connected(&guard, &plan) {
            return Err(fail_pair(data, guard, err).await);
        }
        if data.version_num != guard.version_num {
            return Err(fail_pair(data, guard, SourceError::protocol("read_node")).await);
        }
        if let Err(err) = ensure_deadline(plan.deadline) {
            return Err(fail_pair(data, guard, err).await);
        }
        if let Err(err) = data.begin().await {
            return Err(fail_pair(data, guard, err).await);
        }
        if let Err(err) = guard.begin_read_committed().await {
            return Err(fail_pair(data, guard, err).await);
        }
        let captured = match capture(&mut data, &plan).await {
            Ok(captured) => captured,
            Err(err) => return Err(fail_pair(data, guard, err).await),
        };
        if let Err(err) = ensure_deadline(plan.deadline) {
            return Err(fail_pair(data, guard, err).await);
        }
        if let Err(err) = observe_current(&mut guard, &captured.node, &captured.catalog, &captured.access).await
        {
            return Err(fail_pair(data, guard, err).await);
        }
        let input_digest = match snapshot_input_digest(
            &plan.config_identity,
            &plan.type_policy_identity,
            &captured.catalog.schema_hash,
            plan.generation,
        ) {
            Ok(digest) => digest,
            Err(err) => return Err(fail_pair(data, guard, err).await),
        };
        Ok(Self {
            session: Some(data),
            guard: Some(guard),
            binding: SnapshotBinding {
                protocol: SNAPSHOT_PROTOCOL,
                generation: plan.generation,
                source_id: plan.source_id,
                database: captured.node.database,
                principal: captured.node.current_user,
                in_recovery: captured.node.in_recovery,
                postmaster_start: captured.node.postmaster_start,
                schema_hash: captured.catalog.schema_hash.clone(),
                config_identity: plan.config_identity,
                type_policy_identity: plan.type_policy_identity,
                input_digest,
                backend: BackendKey {
                    pid: captured.pid,
                    backend_start: captured.backend_start,
                },
                catalog: captured.catalog,
                access: captured.access,
            },
            tables: captured.tables,
            policy: plan.policy,
            deadline: plan.deadline,
            poison: None,
            released: false,
            stream: None,
            cancel: Arc::new(AtomicBool::new(false)),
            exhausted_delivered: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Flag shared with the active pull. Requesting cancel does not release the owner.
    pub fn run_cancel(&self) -> RunCancel {
        RunCancel {
            flag: Arc::clone(&self.cancel),
        }
    }

    /// Stop the pull, roll back, and close both sessions.
    /// [`RunCancelled`] is returned only when neither session skipped `ROLLBACK`
    /// and the pull did not end `cleanup_unconfirmed`. A dropped `next_batch`
    /// future is not this method and does not end the backend.
    pub async fn cancel(mut self) -> Result<RunCancelled, SourceError> {
        self.cancel.store(true, Ordering::Relaxed);
        let stream_err = self.take_stream().await.err();
        let cancelled = RunCancelled {
            generation: self.binding.generation,
            source_id: self.binding.source_id.clone(),
            backend: self.binding.backend.clone(),
        };
        self.finish_cancel(stream_err, cancelled).await
    }

    /// Catalog, privileges, and backend identity captured in this transaction.
    pub fn binding(&self) -> &SnapshotBinding {
        &self.binding
    }

    /// Read at most one row by primary key. Zero rows is an empty batch.
    /// Two rows is `probe_key`. A failure poisons the owner.
    pub async fn read_probe(&mut self, probe: CutProbe) -> Result<TypedBatch, SourceError> {
        if self.stream.is_some() {
            return Err(SourceError::protocol("stream_active"));
        }
        let result = self.probe_inner(probe).await;
        match result {
            Ok(batch) => Ok(batch),
            Err(err) => Err(self.poison(err)),
        }
    }

    /// Read the current catalog, then roll back and close both sessions.
    /// [`SnapshotFinished`] is lifecycle release for compatibility. It is not
    /// admission and does not require [`crate::StreamItem::Exhausted`].
    /// It is returned only when the catalog still matches, both rollbacks and
    /// closes succeed, the owner is not poisoned, and the deadline has not passed.
    pub async fn finish(mut self) -> Result<SnapshotFinished, SourceError> {
        let requested = self.cancel.load(Ordering::Relaxed);
        let stream_err = self.take_stream().await.err();
        if let Some(err) = stream_err.clone() {
            let _ = self.poison(err);
        }
        if requested {
            let cancelled = RunCancelled {
                generation: self.binding.generation,
                source_id: self.binding.source_id.clone(),
                backend: self.binding.backend.clone(),
            };
            self.finish_cancel(stream_err, cancelled).await?;
            return Err(SourceError::protocol("cancelled"));
        }
        let finished = SnapshotFinished {
            generation: self.binding.generation,
            source_id: self.binding.source_id.clone(),
            backend: self.binding.backend.clone(),
        };
        let mut early = self.poison.clone();
        if early.is_none() {
            if Instant::now() >= self.deadline {
                early = Some(SourceError::budget("deadline"));
            } else if let Err(err) = self.observe_guard().await {
                early = Some(err);
            }
        }
        let released = self.release_both().await;
        if let Some(err) = stream_err.filter(|err| err.kind() == crate::error::SourceErrorKind::Budget)
        {
            return Err(err);
        }
        released?;
        if let Some(err) = early {
            return Err(err);
        }
        ensure_deadline(self.deadline)?;
        Ok(finished)
    }

    /// Release both sessions and return [`RunComplete`] only when this owner
    /// delivered [`crate::StreamItem::Exhausted`] for every selected table.
    ///
    /// Cancellation, the deadline, and the current catalog and ACL are checked
    /// before release. A budget, portal, or source failure, a deadline, a
    /// current catalog or ACL failure, and a failed cleanup return that failure
    /// and do not mint [`RunComplete`]. No stream, a stream that was not pulled,
    /// one batch while rows remain, `TableEof` without `Exhausted`, and a later
    /// selected table that was never scanned return `protocol` fact
    /// `not_exhausted` after both connections close. A successful rollback is
    /// not admission. [`SnapshotFinished`] cannot admit a loader.
    pub async fn finish_complete(mut self) -> Result<RunComplete, SourceError> {
        let requested = self.cancel.load(Ordering::Relaxed);
        let stream_err = self.take_stream().await.err();
        if let Some(err) = stream_err.clone() {
            let _ = self.poison(err);
        }
        if requested || self.cancel.load(Ordering::Relaxed) {
            let cancelled = RunCancelled {
                generation: self.binding.generation,
                source_id: self.binding.source_id.clone(),
                backend: self.binding.backend.clone(),
            };
            self.finish_cancel(stream_err, cancelled).await?;
            return Err(SourceError::protocol("cancelled"));
        }
        let mut early = self.poison.clone();
        if early.is_none() {
            if Instant::now() >= self.deadline {
                early = Some(SourceError::budget("deadline"));
            } else if let Err(err) = self.observe_guard().await {
                early = Some(err);
            } else if !self.exhausted_delivered.load(Ordering::Relaxed) {
                early = Some(SourceError::protocol("not_exhausted"));
            }
        }
        if self.cancel.load(Ordering::Relaxed) {
            let cancelled = RunCancelled {
                generation: self.binding.generation,
                source_id: self.binding.source_id.clone(),
                backend: self.binding.backend.clone(),
            };
            self.finish_cancel(stream_err, cancelled).await?;
            return Err(SourceError::protocol("cancelled"));
        }
        let complete = if early.is_none() {
            match self.mint_complete() {
                Ok(token) => Some(token),
                Err(err) => {
                    early = Some(err);
                    None
                }
            }
        } else {
            None
        };
        let released = self.release_both().await;
        if let Some(err) = stream_err.filter(|err| err.kind() == crate::error::SourceErrorKind::Budget)
        {
            return Err(err);
        }
        released?;
        if let Some(err) = early {
            return Err(err);
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(SourceError::protocol("cancelled"));
        }
        ensure_deadline(self.deadline)?;
        if !self.exhausted_delivered.load(Ordering::Relaxed) {
            return Err(SourceError::protocol("not_exhausted"));
        }
        complete.ok_or_else(|| SourceError::protocol("not_exhausted"))
    }

    fn mint_complete(&self) -> Result<RunComplete, SourceError> {
        if !self.exhausted_delivered.load(Ordering::Relaxed) {
            return Err(SourceError::protocol("not_exhausted"));
        }
        let mut selected = Vec::with_capacity(self.tables.len());
        for table in &self.tables {
            selected.push(
                SelectedTable::new(table.schema.clone(), table.name.clone())
                    .map_err(|_| SourceError::catalog("catalog_identity"))?,
            );
        }
        Ok(RunComplete {
            source_id: self.binding.source_id.clone(),
            generation: self.binding.generation,
            config_identity: self.binding.config_identity,
            type_policy_identity: self.binding.type_policy_identity,
            policy: self.policy,
            input_digest: self.binding.input_digest,
            schema_hash: self.binding.schema_hash.clone(),
            catalog: self.binding.catalog.clone(),
            selected,
            seal: CompleteSeal,
        })
    }

    /// Roll back and close both sessions. Success is [`SnapshotAborted`], including
    /// when the owner was already poisoned. Cleanup failure is an error.
    pub async fn abort(mut self) -> Result<SnapshotAborted, SourceError> {
        let _ = self.take_stream().await;
        let aborted = SnapshotAborted {
            generation: self.binding.generation,
            source_id: self.binding.source_id.clone(),
            backend: self.binding.backend.clone(),
        };
        self.release_both().await?;
        Ok(aborted)
    }

    async fn probe_inner(&mut self, probe: CutProbe) -> Result<TypedBatch, SourceError> {
        self.ensure_usable()?;
        let index = self
            .tables
            .iter()
            .position(|table| table.schema == probe.schema && table.name == probe.table)
            .ok_or_else(|| SourceError::catalog("selected_table_missing"))?;
        let prepared = prepare_probe(&self.tables[index], &probe)?;
        guard_plans(&prepared.plans, self.policy)?;
        let cut = self.recheck_cut(index)?;
        {
            let session = self.live_session()?;
            recheck(session, &cut).await?;
        }
        self.ensure_usable()?;
        self.observe_guard().await?;
        self.ensure_usable()?;
        {
            let session = self.live_session()?;
            session.execute_control(sql::SAVEPOINT_PROBE).await?;
        }
        if let Err(err) = self.ensure_usable() {
            let err = match self.drop_probe_lock().await {
                Ok(()) => err,
                Err(cleanup) => cleanup,
            };
            return Err(self.drift_or(err).await);
        }
        let max_bytes = self.live_session()?.budget.max_bytes();
        let query = bind_probe(prepared.sql, &probe.key, &prepared.key_oids)?;
        let fetched = {
            let session = self.live_session()?;
            session.arm_rows(2);
            session::fetch_all(session, query).await
        };
        let rows = match fetched {
            Ok(rows) => rows,
            Err(err) => {
                let _ = self.drop_probe_lock().await;
                return Err(self.drift_or(err).await);
            }
        };
        if let Err(err) = self.drop_probe_lock().await {
            return Err(self.drift_or(err).await);
        }
        let decoded = match decode_rows(max_bytes, &prepared.plans, &rows) {
            Ok(decoded) => decoded,
            Err(err) => return Err(self.drift_or(err).await),
        };
        let run = RunBinding {
            source_id: self.binding.source_id.clone(),
            schema: prepared.schema,
            table: prepared.name,
            run_generation: self.binding.generation,
            input_digest: self.binding.input_digest,
        };
        let batch = match assemble_batch(&run, prepared.plans, &prepared.key_attnums, decoded) {
            Ok(batch) => batch,
            Err(err) => return Err(self.drift_or(err).await),
        };
        self.ensure_usable()?;
        self.observe_guard().await?;
        Ok(batch)
    }

    async fn drift_or(&mut self, err: SourceError) -> SourceError {
        if Instant::now() < self.deadline
            && let Err(observed) = self.observe_guard().await
            && observed.fact() == Some("schema_drift")
        {
            return observed;
        }
        err
    }

    async fn drop_probe_lock(&mut self) -> Result<(), SourceError> {
        let session = self.live_session()?;
        let rolled = session.execute_control(sql::ROLLBACK_PROBE).await;
        let released = session.execute_control(sql::RELEASE_PROBE).await;
        match (rolled, released) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(err), _) => Err(err),
            (Ok(()), Err(err)) => Err(err),
        }
    }

    async fn observe_guard(&mut self) -> Result<(), SourceError> {
        let node = NodeFact {
            in_recovery: self.binding.in_recovery,
            postmaster_start: self.binding.postmaster_start.clone(),
            database: self.binding.database.clone(),
            session_user: self.binding.principal.clone(),
            current_user: self.binding.principal.clone(),
        };
        let catalog = self.binding.catalog.clone();
        let access = self.binding.access.clone();
        let guard = self
            .guard
            .as_mut()
            .ok_or_else(|| SourceError::protocol("snapshot_incomplete"))?;
        observe_current(guard, &node, &catalog, &access).await
    }

    async fn finish_cancel(
        &mut self,
        stream_err: Option<SourceError>,
        cancelled: RunCancelled,
    ) -> Result<RunCancelled, SourceError> {
        let uncertain = self.rollback_uncertain()
            || stream_err
                .as_ref()
                .is_some_and(|err| err.fact() == Some("cleanup_unconfirmed"))
            || self
                .poison
                .as_ref()
                .is_some_and(|err| err.fact() == Some("cleanup_unconfirmed"));
        let released = self.release_both().await;
        if uncertain {
            return Err(SourceError::protocol("cleanup_unconfirmed"));
        }
        released?;
        Ok(cancelled)
    }

    fn rollback_uncertain(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(PgSession::rollback_skipped)
            || self
                .guard
                .as_ref()
                .is_some_and(PgSession::rollback_skipped)
    }

    async fn release_both(&mut self) -> Result<(), SourceError> {
        self.released = true;
        let data = self.session.take();
        let guard = self.guard.take();
        let data_res = match data {
            Some(session) => release_session(session).await,
            None => Ok(()),
        };
        let guard_res = match guard {
            Some(session) => release_session(session).await,
            None => Ok(()),
        };
        match (data_res, guard_res) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(err), _) => Err(err),
            (Ok(()), Err(err)) => Err(err),
        }
    }

    fn ensure_usable(&self) -> Result<(), SourceError> {
        if self.released || self.session.is_none() || self.guard.is_none() {
            return Err(SourceError::protocol("snapshot_incomplete"));
        }
        if let Some(err) = &self.poison {
            return Err(err.clone());
        }
        ensure_deadline(self.deadline)
    }

    fn poison(&mut self, err: SourceError) -> SourceError {
        if let Some(existing) = &self.poison {
            return existing.clone();
        }
        self.poison = Some(err.clone());
        err
    }

    fn live_session(&mut self) -> Result<&mut PgSession, SourceError> {
        self.session
            .as_mut()
            .ok_or_else(|| SourceError::protocol("snapshot_incomplete"))
    }

    /// Plan typed half-open chunks for every selected table on this cut.
    ///
    /// `target_rows` is `1..=50_000`. A table with no supported plain key gets one
    /// unkeyed chunk and no resume cursor. Planning uses this transaction.
    pub async fn plan_keys(&mut self, target_rows: u32) -> Result<crate::key_plan::KeyPlan, SourceError> {
        if target_rows == 0 || target_rows > crate::key_plan::DEFAULT_TARGET_ROWS {
            return Err(SourceError::config("target_rows"));
        }
        if self.stream.is_some() {
            return Err(SourceError::protocol("stream_active"));
        }
        match self.plan_keys_inner(target_rows).await {
            Ok(plan) => Ok(plan),
            Err(err) => Err(self.poison(err)),
        }
    }

    /// Pull one planned chunk. The chunk generation and input digest must be this cut.
    pub async fn open_chunk(
        &mut self,
        chunk: &crate::key_plan::PlannedChunk,
        budget: crate::stream::StreamBudget,
    ) -> Result<(), SourceError> {
        if self.stream.is_some() {
            return Err(SourceError::protocol("stream_active"));
        }
        match self.open_chunk_inner(chunk, budget).await {
            Ok(()) => Ok(()),
            Err(err) => Err(self.poison(err)),
        }
    }

    /// Pull one keyed range on this cut. No declared key is `key_required` and does not poison.
    /// Both bounds empty is a full ordered scan of the declared key.
    pub async fn read_keys(
        &mut self,
        schema: &str,
        table: &str,
        range: crate::key_plan::KeyRange,
        budget: crate::stream::StreamBudget,
    ) -> Result<(), SourceError> {
        if self.stream.is_some() {
            return Err(SourceError::protocol("stream_active"));
        }
        match self.read_keys_inner(schema, table, range, budget).await {
            Ok(()) => Ok(()),
            Err(err) if err.fact() == Some("key_required") => Err(err),
            Err(err) => Err(self.poison(err)),
        }
    }

    /// Move both sessions into one pull. `next_batch` is the only Execute driver.
    pub async fn open_stream(
        &mut self,
        budget: crate::stream::StreamBudget,
    ) -> Result<(), SourceError> {
        if self.stream.is_some() {
            return Err(SourceError::protocol("stream_active"));
        }
        self.ensure_usable()?;
        let plan = self.stream_plan(&budget)?;
        self.install_stream(plan)
    }

    /// One demanded batch, one table end, or run exhaustion. This polls the pull.
    /// It does not finish the owner.
    pub async fn next_batch(&mut self) -> Result<crate::stream::StreamItem, SourceError> {
        if self.released {
            return Err(SourceError::protocol("snapshot_incomplete"));
        }
        let Some(slot) = self.stream.as_mut() else {
            return Err(SourceError::protocol("stream_idle"));
        };
        match slot.next_item().await {
            crate::stream::Next::Item(item) => Ok(item),
            crate::stream::Next::Ended(end) => {
                self.stream = None;
                let err = match end {
                    Some(end) => {
                        let err = end
                            .error
                            .unwrap_or_else(|| SourceError::protocol("stream_ended"));
                        self.session = Some(end.data);
                        self.guard = Some(end.guard);
                        err
                    }
                    None => SourceError::protocol("stream_ended"),
                };
                Err(self.poison(err))
            }
        }
    }

    /// Stop the pull, restore both sessions, and re-read the current catalog.
    /// This does not roll back the snapshot transaction.
    pub async fn close_stream(&mut self) -> Result<(), SourceError> {
        if let Err(err) = self.take_stream().await {
            return Err(self.poison(err));
        }
        if let Some(err) = &self.poison {
            return Err(err.clone());
        }
        self.ensure_usable()?;
        self.observe_guard().await
    }

    async fn take_stream(&mut self) -> Result<(), SourceError> {
        let Some(slot) = self.stream.take() else {
            return Ok(());
        };
        let end = slot.stop().await;
        self.session = Some(end.data);
        self.guard = Some(end.guard);
        match end.error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    async fn plan_keys_inner(&mut self, target_rows: u32) -> Result<crate::key_plan::KeyPlan, SourceError> {
        self.ensure_usable()?;
        self.observe_guard().await?;
        let generation = self.binding.generation;
        let input_digest = self.binding.input_digest;
        let deadline = self.deadline;
        let mut planned = Vec::with_capacity(self.tables.len());
        for index in 0..self.tables.len() {
            ensure_deadline(deadline)?;
            let source = self.source_at(index)?;
            let schema_sql = self.tables[index].schema_sql.clone();
            let table_sql = self.tables[index].table_sql.clone();
            let plans = self.tables[index].plans.clone();
            let session = self.live_session()?;
            planned.push(
                crate::key_plan::plan_table(crate::key_plan::PlanIn {
                    session,
                    source: &source,
                    schema_sql: &schema_sql,
                    table_sql: &table_sql,
                    plans: &plans,
                    generation,
                    input_digest,
                    target_rows,
                    deadline,
                })
                .await?,
            );
        }
        Ok(crate::key_plan::KeyPlan {
            protocol: crate::key_plan::KEY_PLAN_PROTOCOL,
            generation,
            input_digest,
            target_rows,
            tables: planned,
        })
    }

    async fn open_chunk_inner(
        &mut self,
        chunk: &crate::key_plan::PlannedChunk,
        budget: crate::stream::StreamBudget,
    ) -> Result<(), SourceError> {
        self.ensure_usable()?;
        self.observe_guard().await?;
        if chunk.generation != self.binding.generation || chunk.input_digest != self.binding.input_digest
        {
            return Err(SourceError::protocol("key_plan"));
        }
        let index = self
            .table_index(&chunk.schema, &chunk.table)
            .ok_or_else(|| SourceError::catalog("selected_table_missing"))?;
        let spec = self.read_spec_chunk(index, chunk).await?;
        let plan = self.stream_plan_one(&budget, index, &spec)?;
        self.install_stream(plan)
    }

    async fn read_keys_inner(
        &mut self,
        schema: &str,
        table: &str,
        range: crate::key_plan::KeyRange,
        budget: crate::stream::StreamBudget,
    ) -> Result<(), SourceError> {
        self.ensure_usable()?;
        self.observe_guard().await?;
        let index = self
            .table_index(schema, table)
            .ok_or_else(|| SourceError::catalog("selected_table_missing"))?;
        let spec = self.read_spec_range(index, &range).await?;
        let plan = self.stream_plan_one(&budget, index, &spec)?;
        self.install_stream(plan)
    }

    async fn read_spec_chunk(
        &mut self,
        index: usize,
        chunk: &crate::key_plan::PlannedChunk,
    ) -> Result<crate::key_plan::ReadSpec, SourceError> {
        let source = self.source_at(index)?;
        let schema_sql = self.tables[index].schema_sql.clone();
        let table_sql = self.tables[index].table_sql.clone();
        let select_sql = self.tables[index].select_sql.clone();
        let plans = self.tables[index].plans.clone();
        let deadline = self.deadline;
        let session = self.live_session()?;
        crate::key_plan::chunk_read(
            crate::key_plan::ReadIn {
                session,
                source: &source,
                plans: &plans,
                schema_sql: &schema_sql,
                table_sql: &table_sql,
                select_sql: &select_sql,
                deadline,
            },
            chunk,
        )
        .await
    }

    async fn read_spec_range(
        &mut self,
        index: usize,
        range: &crate::key_plan::KeyRange,
    ) -> Result<crate::key_plan::ReadSpec, SourceError> {
        let source = self.source_at(index)?;
        let schema_sql = self.tables[index].schema_sql.clone();
        let table_sql = self.tables[index].table_sql.clone();
        let select_sql = self.tables[index].select_sql.clone();
        let plans = self.tables[index].plans.clone();
        let deadline = self.deadline;
        let session = self.live_session()?;
        crate::key_plan::range_read(
            crate::key_plan::ReadIn {
                session,
                source: &source,
                plans: &plans,
                schema_sql: &schema_sql,
                table_sql: &table_sql,
                select_sql: &select_sql,
                deadline,
            },
            range,
        )
        .await
    }

    fn table_index(&self, schema: &str, table: &str) -> Option<usize> {
        self.tables
            .iter()
            .position(|item| item.schema == schema && item.name == table)
    }

    fn source_at(&self, index: usize) -> Result<SourceTable, SourceError> {
        let table = self
            .tables
            .get(index)
            .ok_or_else(|| SourceError::catalog("selected_table_missing"))?;
        self.binding
            .catalog
            .tables
            .iter()
            .find(|source| source.schema == table.schema && source.name == table.name)
            .cloned()
            .ok_or_else(|| SourceError::catalog("catalog_identity"))
    }

    fn install_stream(&mut self, plan: crate::stream::StreamPlan) -> Result<(), SourceError> {
        let data = self
            .session
            .take()
            .ok_or_else(|| SourceError::protocol("snapshot_incomplete"))?;
        let guard = self
            .guard
            .take()
            .ok_or_else(|| SourceError::protocol("snapshot_incomplete"))?;
        self.stream = Some(crate::stream::StreamSlot::start(data, guard, plan));
        Ok(())
    }

    fn stream_plan(
        &self,
        budget: &crate::stream::StreamBudget,
    ) -> Result<crate::stream::StreamPlan, SourceError> {
        let mut tables = Vec::with_capacity(self.tables.len());
        for index in 0..self.tables.len() {
            tables.push(self.stream_table(index, None)?);
        }
        self.stream_envelope(budget, tables, true)
    }

    fn stream_plan_one(
        &self,
        budget: &crate::stream::StreamBudget,
        index: usize,
        spec: &crate::key_plan::ReadSpec,
    ) -> Result<crate::stream::StreamPlan, SourceError> {
        let tables = vec![self.stream_table(index, Some(spec))?];
        self.stream_envelope(budget, tables, false)
    }

    fn stream_table(
        &self,
        index: usize,
        spec: Option<&crate::key_plan::ReadSpec>,
    ) -> Result<crate::stream::StreamTablePlan, SourceError> {
        let table = &self.tables[index];
        let presence = table.select_sql.is_empty();
        let sql = if let Some(spec) = spec {
            spec.sql.clone()
        } else if presence {
            crate::types::compose_presence_scan(&table.schema_sql, &table.table_sql)?
        } else {
            crate::types::compose_scan(&table.schema_sql, &table.table_sql, &table.select_sql)?
        };
        let cut = self.recheck_cut(index)?;
        let zero_column = cut.source.columns.is_empty();
        let arrow_columns = u32::try_from(
            table
                .plans
                .iter()
                .filter(|plan| plan.mapping.decision == MapDecision::Lossless)
                .count(),
        )
        .unwrap_or(u32::MAX);
        let (lower, upper, key_oids, key_attnums) = if let Some(spec) = spec {
            (
                spec.lower.clone(),
                spec.upper.clone(),
                spec.key_oids.clone(),
                spec.key_attnums.clone(),
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new(), table.key_attnums.clone())
        };
        Ok(crate::stream::StreamTablePlan {
            schema: table.schema.clone(),
            name: table.name.clone(),
            sql,
            plans: table.plans.clone(),
            key_attnums,
            zero_column,
            presence,
            result_columns: if presence { 1 } else { table.select_sql.len() },
            arrow_columns,
            cut,
            lower,
            upper,
            key_oids,
        })
    }

    fn stream_envelope(
        &self,
        budget: &crate::stream::StreamBudget,
        tables: Vec<crate::stream::StreamTablePlan>,
        covers_selected: bool,
    ) -> Result<crate::stream::StreamPlan, SourceError> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| SourceError::protocol("snapshot_incomplete"))?;
        let full = covers_selected
            && tables.len() == self.tables.len()
            && tables.iter().enumerate().all(|(index, table)| {
                self.tables.get(index).is_some_and(|bound| {
                    table.schema == bound.schema
                        && table.name == bound.name
                        && table.lower.is_empty()
                        && table.upper.is_empty()
                })
            });
        Ok(crate::stream::StreamPlan {
            source_id: self.binding.source_id.clone(),
            generation: self.binding.generation,
            input_digest: self.binding.input_digest,
            deadline: self.deadline,
            statement_timeout: session.budget.statement_timeout(),
            max_bytes: session.budget.max_bytes(),
            batch_rows: budget.batch_rows(),
            batch_bytes: budget.batch_bytes(),
            max_value_bytes: budget.max_value_bytes(),
            max_row_bytes: budget.max_row_bytes(),
            guard: StreamGuard {
                in_recovery: self.binding.in_recovery,
                postmaster_start: self.binding.postmaster_start.clone(),
                database: self.binding.database.clone(),
                session_user: self.binding.principal.clone(),
                current_user: self.binding.principal.clone(),
                catalog: self.binding.catalog.clone(),
                access: self.binding.access.clone(),
            },
            tables,
            cancel: Arc::clone(&self.cancel),
            backend_pid: self.binding.backend.pid,
            backend_start: self.binding.backend.backend_start.clone(),
            covers_selected: full,
            exhausted_delivered: Arc::clone(&self.exhausted_delivered),
        })
    }

}

impl fmt::Debug for SnapshotOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotOwner")
            .field("generation", &self.binding.generation)
            .field("source_id", &self.binding.source_id)
            .field("backend_pid", &self.binding.backend.pid)
            .field("open", &self.session.is_some())
            .field("guard_open", &self.guard.is_some())
            .finish()
    }
}

impl Drop for SnapshotOwner {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        self.cancel.store(true, Ordering::Relaxed);
        // Stop plus rollback can sit in a portal drain or a TLS close.
        // Bound the spawned work so dropping it closes the sockets inside
        // the absence deadline. This spawn does not block `Drop`.
        let budget = crate::session::SOCKET_ABORT_AFTER;
        if let Some(slot) = self.stream.take() {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let cleanup = async move {
                        let end = slot.stop().await;
                        let _ = release_session(end.data).await;
                        let _ = release_session(end.guard).await;
                    };
                    let _ = tokio::time::timeout(budget, cleanup).await;
                });
            }
            return;
        }
        let data = self.session.take();
        let guard = self.guard.take();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let cleanup = async move {
                    if let Some(session) = data {
                        let _ = release_session(session).await;
                    }
                    if let Some(session) = guard {
                        let _ = release_session(session).await;
                    }
                };
                let _ = tokio::time::timeout(budget, cleanup).await;
            });
            return;
        }
        drop(data);
        drop(guard);
    }
}

struct PreparedProbe {
    sql: String,
    schema: String,
    name: String,
    key_oids: Vec<i64>,
    key_attnums: Vec<i32>,
    plans: Vec<ColumnPlan>,
}

fn prepare_probe(table: &BoundTable, probe: &CutProbe) -> Result<PreparedProbe, SourceError> {
    if probe.key.len() != table.key_oids.len() || probe.key.is_empty() {
        return Err(SourceError::protocol("probe_key"));
    }
    Ok(PreparedProbe {
        sql: compose_probe(
            &table.schema_sql,
            &table.table_sql,
            &table.select_sql,
            &table.key_sql,
        )?,
        schema: table.schema.clone(),
        name: table.name.clone(),
        key_oids: table.key_oids.clone(),
        key_attnums: table.key_attnums.clone(),
        plans: table.plans.clone(),
    })
}

fn open_plan(request: &SnapshotRequest) -> Result<OpenPlan, SourceError> {
    if request.generation == 0 {
        return Err(SourceError::config("generation_invalid"));
    }
    ensure_deadline(request.deadline)?;
    if config_identity(&request.config) != request.config_identity {
        return Err(SourceError::config("config_identity"));
    }
    if type_policy_identity(request.policy) != request.type_policy_identity {
        return Err(SourceError::config("type_policy_identity"));
    }
    selection_matches(&request.config, &request.admitted)?;
    Ok(OpenPlan {
        admitted: request.admitted.clone(),
        config_identity: request.config_identity,
        type_policy_identity: request.type_policy_identity,
        policy: request.policy,
        generation: request.generation,
        deadline: request.deadline,
        expected_principal: request.expected_principal.clone(),
        expected_database: request.expected_database.clone(),
        expected_in_recovery: request.expected_in_recovery,
        expected_postmaster_start: request.expected_postmaster_start.clone(),
        username: request.config.credentials().username().to_string(),
        database: request.config.endpoint().database().to_string(),
        source_id: request.config.source_id().as_str().to_string(),
    })
}

fn reserve(source_id: &str, generation: u64) -> Result<(), SourceError> {
    let mut guard = GENERATIONS
        .lock()
        .map_err(|_| SourceError::protocol("generation_reused"))?;
    if !guard.insert((source_id.to_string(), generation)) {
        return Err(SourceError::protocol("generation_reused"));
    }
    Ok(())
}

fn selection_matches(config: &PgSourceConfig, admitted: &AdmittedCatalog) -> Result<(), SourceError> {
    let selected = config.selection().tables();
    if selected.is_empty() || admitted.tables.is_empty() || selected.len() != admitted.tables.len() {
        return Err(SourceError::config("selection"));
    }
    let mut seen = BTreeSet::new();
    for table in &admitted.tables {
        if !seen.insert((table.schema.clone(), table.name.clone())) {
            return Err(SourceError::config("selection"));
        }
        if table.primary_key.len() > INDEX_MAX_KEYS {
            return Err(SourceError::config("selection"));
        }
        if table.columns.is_empty() && !table.primary_key.is_empty() {
            return Err(SourceError::protocol("probe_key"));
        }
        let mut attnums = BTreeSet::new();
        for column in &table.columns {
            if column.attnum <= 0 || column.name.is_empty() || !attnums.insert(column.attnum) {
                return Err(SourceError::config("selection"));
            }
        }
        for key in &table.primary_key {
            if !table.columns.iter().any(|column| column.name == *key) {
                return Err(SourceError::protocol("probe_key"));
            }
        }
    }
    for table in selected {
        if !seen.contains(&(table.schema().to_string(), table.table().to_string())) {
            return Err(SourceError::config("selection"));
        }
    }
    Ok(())
}

fn admit_connected(session: &PgSession, plan: &OpenPlan) -> Result<(), SourceError> {
    if !session.identity_aligned {
        return Err(SourceError::config("session_user_mismatch"));
    }
    if !session.startup_clean {
        return Err(SourceError::config("startup_options"));
    }
    if session.admission != "tested" {
        return Err(version_error(session.admission));
    }
    if session.current_user != plan.expected_principal
        || session.session_user != plan.expected_principal
        || session.current_user != plan.username
    {
        return Err(SourceError::config("principal"));
    }
    if session.database_name != plan.expected_database || session.database_name != plan.database {
        return Err(SourceError::config("database"));
    }
    Ok(())
}

fn version_error(admission: &str) -> SourceError {
    SourceError::version(match admission {
        "below_minimum" => "version_below_minimum",
        "untested" => "version_untested",
        _ => "version_unknown",
    })
}

async fn capture(session: &mut PgSession, plan: &OpenPlan) -> Result<Captured, SourceError> {
    ensure_deadline(plan.deadline)?;
    let node = read_node(session).await?;
    check_node_open(&node, plan)?;
    ensure_deadline(plan.deadline)?;
    let catalog = session.collect_catalog().await?;
    ensure_deadline(plan.deadline)?;
    match_admitted(&plan.admitted, &catalog)?;
    let mut access = Vec::new();
    let mut tables = Vec::new();
    for admitted in &plan.admitted.tables {
        let source = catalog
            .tables
            .iter()
            .find(|table| table.schema == admitted.schema && table.name == admitted.name)
            .ok_or_else(|| SourceError::catalog("catalog_identity"))?;
        let (table_access, bound) = bind_table(session, source, plan.policy).await?;
        access.push(table_access);
        tables.push(bound);
    }
    let (pid, backend_start, in_xact) = read_backend(session).await?;
    if !in_xact {
        return Err(SourceError::protocol("snapshot_transaction"));
    }
    Ok(Captured {
        node,
        catalog,
        access,
        tables,
        pid,
        backend_start,
    })
}

fn check_node_open(node: &NodeFact, plan: &OpenPlan) -> Result<(), SourceError> {
    if node.in_recovery != plan.expected_in_recovery
        || node.postmaster_start != plan.expected_postmaster_start
    {
        return Err(SourceError::protocol("read_node"));
    }
    if node.database != plan.expected_database || node.database != plan.database {
        return Err(SourceError::config("database"));
    }
    if node.current_user != plan.expected_principal
        || node.session_user != plan.expected_principal
        || node.current_user != plan.username
    {
        return Err(SourceError::config("principal"));
    }
    Ok(())
}

fn match_admitted(admitted: &AdmittedCatalog, catalog: &SourceCatalog) -> Result<(), SourceError> {
    if admitted.tables.len() != catalog.tables.len() {
        return Err(SourceError::catalog("catalog_identity"));
    }
    for admitted_table in &admitted.tables {
        let Some(table) = catalog
            .tables
            .iter()
            .find(|table| table.schema == admitted_table.schema && table.name == admitted_table.name)
        else {
            return Err(SourceError::catalog("catalog_identity"));
        };
        if !columns_match(&table.columns, &admitted_table.columns) {
            return Err(SourceError::catalog("catalog_identity"));
        }
        let got: Vec<&str> = table.primary_key.iter().map(|key| key.name.as_str()).collect();
        let want: Vec<&str> = admitted_table
            .primary_key
            .iter()
            .map(String::as_str)
            .collect();
        if got != want {
            return Err(SourceError::catalog("catalog_identity"));
        }
    }
    Ok(())
}

fn columns_match(got: &[SourceColumn], want: &[AdmittedColumn]) -> bool {
    if got.len() != want.len() {
        return false;
    }
    let mut got_cols: Vec<&SourceColumn> = got.iter().collect();
    got_cols.sort_by_key(|column| column.attnum);
    let mut want_cols = want.to_vec();
    want_cols.sort_by_key(|column| column.attnum);
    got_cols.iter().zip(want_cols.iter()).all(|(got, want)| {
        got.attnum == want.attnum
            && got.name == want.name
            && got.type_oid == want.type_oid
            && got.typmod == want.typmod
            && got.not_null == want.not_null
    })
}

async fn bind_table(
    session: &mut PgSession,
    source: &SourceTable,
    policy: PgTypePolicy,
) -> Result<(TableAccess, BoundTable), SourceError> {
    let relation = read_relation(session, &source.schema, &source.name).await?;
    check_relation(&relation)?;
    let attrs = read_attributes(session, &source.schema, &source.name).await?;
    if !attributes_match(source, &attrs) {
        return Err(SourceError::catalog("schema_drift"));
    }
    for attr in &attrs {
        if attr.select != Some(true) {
            return Err(SourceError::privilege("select_not_granted"));
        }
    }
    let plans = plan_columns(session, source, policy).await?;
    let probe = probe_keys(source);
    let key_attnums = if probe.supported {
        source.primary_key.iter().map(|key| key.attnum).collect()
    } else {
        Vec::new()
    };
    let key_oids = probe.oids;
    let quoted = quote_table(session, source, &plans).await?;
    Ok((
        TableAccess {
            schema: source.schema.clone(),
            name: source.name.clone(),
            relation_oid: relation.oid,
            select: relation.select,
            insert: relation.insert,
            update: relation.update,
            delete: relation.delete,
            truncate: relation.truncate,
        },
        BoundTable {
            schema: source.schema.clone(),
            name: source.name.clone(),
            schema_sql: quoted.schema_sql,
            table_sql: quoted.table_sql,
            select_sql: quoted.select_sql,
            key_sql: quoted.key_sql,
            key_oids,
            key_attnums,
            plans,
        },
    ))
}

fn check_relation(relation: &RelationFact) -> Result<(), SourceError> {
    if relation.relkind != "r" {
        return Err(SourceError::catalog("relkind"));
    }
    if relation.rls || relation.forced {
        return Err(SourceError::protocol("rls_unsupported"));
    }
    if relation.select != Some(true) {
        return Err(SourceError::privilege("select_not_granted"));
    }
    for grant in [
        relation.insert,
        relation.update,
        relation.delete,
        relation.truncate,
    ] {
        match grant {
            Some(true) => return Err(SourceError::privilege("write_capable")),
            None => return Err(SourceError::privilege("privilege_unknown")),
            Some(false) => {}
        }
    }
    Ok(())
}

fn attributes_match(source: &SourceTable, attrs: &[AttrFact]) -> bool {
    if source.columns.len() != attrs.len() {
        return false;
    }
    let mut columns: Vec<&SourceColumn> = source.columns.iter().collect();
    columns.sort_by_key(|column| column.attnum);
    let mut attrs: Vec<&AttrFact> = attrs.iter().collect();
    attrs.sort_by_key(|attr| attr.attnum);
    columns.iter().zip(attrs.iter()).all(|(column, attr)| {
        column.attnum == attr.attnum
            && column.name == attr.name
            && column.type_oid == attr.type_oid
            && column.typmod == attr.typmod
            && column.not_null == attr.not_null
    })
}

async fn plan_columns(
    session: &mut PgSession,
    source: &SourceTable,
    policy: PgTypePolicy,
) -> Result<Vec<ColumnPlan>, SourceError> {
    let graph = load_type_graph(session, &source.columns).await?;
    let probe = probe_keys(source);
    let key_attnums: Vec<i32> = if probe.supported {
        source.primary_key.iter().map(|key| key.attnum).collect()
    } else {
        Vec::new()
    };
    let mut plans = Vec::with_capacity(source.columns.len());
    for column in &source.columns {
        plans.push(map_column(
            column,
            &graph,
            key_attnums.contains(&column.attnum),
        ));
    }
    guard_plans(&plans, policy)?;
    Ok(plans)
}

struct ProbeKeys {
    supported: bool,
    oids: Vec<i64>,
}

/// Empty, wider than the probe, or a non-probe oid is not an open failure.
/// `read_probe` still returns `probe_key` for those tables.
fn probe_keys(source: &SourceTable) -> ProbeKeys {
    let attnums: Vec<i32> = source.primary_key.iter().map(|key| key.attnum).collect();
    if attnums.is_empty() || attnums.len() > MAX_KEY_COLUMNS {
        return ProbeKeys {
            supported: false,
            oids: Vec::new(),
        };
    }
    let mut oids = Vec::with_capacity(attnums.len());
    for attnum in attnums {
        let Some(column) = source.columns.iter().find(|column| column.attnum == attnum) else {
            return ProbeKeys {
                supported: false,
                oids: Vec::new(),
            };
        };
        if !probe_oid(column.type_oid) {
            return ProbeKeys {
                supported: false,
                oids: Vec::new(),
            };
        }
        oids.push(column.type_oid);
    }
    ProbeKeys {
        supported: true,
        oids,
    }
}

pub(crate) fn supported_key_oid(oid: i64) -> bool {
    matches!(
        oid,
        OID_BOOL | OID_INT8 | OID_INT2 | OID_INT4 | OID_TEXT | OID_BPCHAR | OID_VARCHAR
    )
}

fn probe_oid(oid: i64) -> bool {
    supported_key_oid(oid)
}

struct QuotedTable {
    schema_sql: String,
    table_sql: String,
    select_sql: Vec<String>,
    key_sql: Vec<String>,
}

async fn quote_table(
    session: &mut PgSession,
    source: &SourceTable,
    plans: &[ColumnPlan],
) -> Result<QuotedTable, SourceError> {
    let schema_sql = quote_one(session, &source.schema).await?;
    let table_sql = quote_one(session, &source.name).await?;
    let mut select_sql = Vec::new();
    for plan in plans {
        if plan.mapping.decision == MapDecision::Lossless {
            select_sql.push(quote_one(session, &plan.name).await?);
        }
    }
    let mut key_sql = Vec::new();
    if probe_keys(source).supported {
        for key in &source.primary_key {
            let Some(plan) = plans.iter().find(|plan| plan.attnum == key.attnum) else {
                return Err(SourceError::protocol("probe_key"));
            };
            if plan.mapping.decision != MapDecision::Lossless {
                return Err(SourceError::protocol("probe_key"));
            }
            key_sql.push(quote_one(session, &key.name).await?);
        }
    }
    Ok(QuotedTable {
        schema_sql,
        table_sql,
        select_sql,
        key_sql,
    })
}

pub(crate) struct RecheckCut {
    principal: String,
    database: String,
    in_recovery: bool,
    postmaster_start: String,
    relation_oid: i64,
    source: SourceTable,
    schema: String,
    name: String,
}

pub(crate) struct StreamGuard {
    in_recovery: bool,
    postmaster_start: String,
    database: String,
    session_user: String,
    current_user: String,
    catalog: SourceCatalog,
    access: Vec<TableAccess>,
}

impl SnapshotOwner {
    fn recheck_cut(&self, index: usize) -> Result<RecheckCut, SourceError> {
        let schema = self.tables[index].schema.clone();
        let name = self.tables[index].name.clone();
        let relation_oid = self
            .binding
            .access
            .iter()
            .find(|item| item.schema == schema && item.name == name)
            .map(|item| item.relation_oid)
            .ok_or_else(|| SourceError::catalog("schema_drift"))?;
        let source = self
            .binding
            .catalog
            .tables
            .iter()
            .find(|item| item.schema == schema && item.name == name)
            .cloned()
            .ok_or_else(|| SourceError::catalog("schema_drift"))?;
        Ok(RecheckCut {
            principal: self.binding.principal.clone(),
            database: self.binding.database.clone(),
            in_recovery: self.binding.in_recovery,
            postmaster_start: self.binding.postmaster_start.clone(),
            relation_oid,
            source,
            schema,
            name,
        })
    }
}

pub(crate) async fn recheck_stream(
    session: &mut PgSession,
    cut: &RecheckCut,
) -> Result<(), SourceError> {
    recheck(session, cut).await
}

pub(crate) async fn guard_still_current(
    guard: &mut PgSession,
    view: &StreamGuard,
) -> Result<(), SourceError> {
    let node = NodeFact {
        in_recovery: view.in_recovery,
        postmaster_start: view.postmaster_start.clone(),
        database: view.database.clone(),
        session_user: view.session_user.clone(),
        current_user: view.current_user.clone(),
    };
    observe_current(guard, &node, &view.catalog, &view.access).await
}

async fn recheck(session: &mut PgSession, cut: &RecheckCut) -> Result<(), SourceError> {
    let node = read_node(session).await?;
    if node.current_user != cut.principal
        || node.session_user != cut.principal
        || node.database != cut.database
    {
        return Err(SourceError::protocol("identity_drift"));
    }
    if node.in_recovery != cut.in_recovery || node.postmaster_start != cut.postmaster_start {
        return Err(SourceError::protocol("read_node"));
    }
    let relation = read_relation(session, &cut.schema, &cut.name).await?;
    if relation.oid != cut.relation_oid || relation.relkind != "r" {
        return Err(SourceError::catalog("schema_drift"));
    }
    check_relation(&relation)?;
    let attrs = read_attributes(session, &cut.schema, &cut.name).await?;
    if !attributes_match(&cut.source, &attrs) {
        return Err(SourceError::catalog("schema_drift"));
    }
    if attrs.iter().any(|attr| attr.select != Some(true)) {
        return Err(SourceError::privilege("select_not_granted"));
    }
    Ok(())
}

/// Primary-key equality with `LIMIT 2`. This is not a table scan and not
/// `fetch_all` of the relation. Values are bound. Identifiers are `quote_ident`
/// output that [`valid_quoted_ident`] already accepted.
fn compose_probe(
    schema: &str,
    table: &str,
    columns: &[String],
    keys: &[String],
) -> Result<String, SourceError> {
    if columns.is_empty() || keys.is_empty() || keys.len() > MAX_KEY_COLUMNS {
        return Err(SourceError::protocol("probe_key"));
    }
    if !valid_quoted_ident(schema) || !valid_quoted_ident(table) {
        return Err(SourceError::catalog("quote_ident"));
    }
    for ident in columns.iter().chain(keys.iter()) {
        if !valid_quoted_ident(ident) {
            return Err(SourceError::catalog("quote_ident"));
        }
    }
    let mut sql = String::from("SELECT ");
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            sql.push(',');
        }
        sql.push_str(column);
    }
    sql.push_str(" FROM ");
    sql.push_str(schema);
    sql.push('.');
    sql.push_str(table);
    sql.push_str(" WHERE ");
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            sql.push_str(" AND ");
        }
        sql.push_str(key);
        sql.push_str(" = $");
        sql.push_str(&(index + 1).to_string());
    }
    sql.push_str(" LIMIT $");
    sql.push_str(&(keys.len() + 1).to_string());
    Ok(sql)
}

fn bind_probe<'a>(
    sql: String,
    keys: &'a [ProbeValue],
    oids: &[i64],
) -> Result<sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>, SourceError> {
    if keys.len() != oids.len() {
        return Err(SourceError::protocol("probe_key"));
    }
    let mut query = sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql));
    for (value, oid) in keys.iter().zip(oids.iter().copied()) {
        query = bind_one(query, value, oid)?;
    }
    Ok(query.bind(2_i64))
}

pub(crate) fn bind_one<'a>(
    query: sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>,
    value: &'a ProbeValue,
    oid: i64,
) -> Result<sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>, SourceError> {
    match (value, oid) {
        (ProbeValue::Int8(value), OID_INT2) => {
            let narrowed = i16::try_from(*value).map_err(|_| SourceError::protocol("probe_key"))?;
            Ok(query.bind(narrowed))
        }
        (ProbeValue::Int8(value), OID_INT4) => {
            let narrowed = i32::try_from(*value).map_err(|_| SourceError::protocol("probe_key"))?;
            Ok(query.bind(narrowed))
        }
        (ProbeValue::Int8(value), OID_INT8) => Ok(query.bind(*value)),
        (ProbeValue::Bool(value), OID_BOOL) => Ok(query.bind(*value)),
        (ProbeValue::Text(value), OID_TEXT | OID_VARCHAR | OID_BPCHAR) => Ok(query.bind(value.as_str())),
        _ => Err(SourceError::protocol("probe_key")),
    }
}

pub(crate) fn push_probe(
    args: &mut PgArguments,
    value: &ProbeValue,
    oid: i64,
) -> Result<(), SourceError> {
    let bad = || SourceError::protocol("probe_key");
    match (value, oid) {
        (ProbeValue::Int8(value), OID_INT2) => {
            let narrowed = i16::try_from(*value).map_err(|_| bad())?;
            args.add(narrowed).map_err(|_| bad())
        }
        (ProbeValue::Int8(value), OID_INT4) => {
            let narrowed = i32::try_from(*value).map_err(|_| bad())?;
            args.add(narrowed).map_err(|_| bad())
        }
        (ProbeValue::Int8(value), OID_INT8) => args.add(*value).map_err(|_| bad()),
        (ProbeValue::Bool(value), OID_BOOL) => args.add(*value).map_err(|_| bad()),
        (ProbeValue::Text(value), OID_TEXT | OID_VARCHAR | OID_BPCHAR) => {
            args.add(value.clone()).map_err(|_| bad())
        }
        _ => Err(bad()),
    }
}

fn decode_rows(
    max_bytes: u64,
    plans: &[ColumnPlan],
    rows: &[sqlx::postgres::PgRow],
) -> Result<Vec<Vec<DecodedCell>>, SourceError> {
    if rows.len() > 1 {
        return Err(SourceError::protocol("probe_key"));
    }
    let readable: Vec<&ColumnPlan> = plans
        .iter()
        .filter(|plan| plan.mapping.decision == MapDecision::Lossless)
        .collect();
    let mut used = 0_u64;
    let mut decoded = Vec::with_capacity(rows.len());
    for row in rows {
        if row.columns().len() != readable.len() {
            return Err(SourceError::protocol("row_width"));
        }
        decoded.push(decode_row(max_bytes, &mut used, &readable, row)?);
    }
    Ok(decoded)
}

fn decode_row(
    max_bytes: u64,
    used: &mut u64,
    readable: &[&ColumnPlan],
    row: &sqlx::postgres::PgRow,
) -> Result<Vec<DecodedCell>, SourceError> {
    let mut cells = Vec::with_capacity(readable.len());
    for (index, plan) in readable.iter().enumerate() {
        let raw = row
            .try_get_raw(index)
            .map_err(|_| SourceError::protocol("binary_payload"))?;
        require_binary(raw.format())?;
        let actual = raw
            .type_info()
            .oid()
            .map(|oid| i64::from(oid.0))
            .ok_or_else(|| SourceError::protocol("type_oid_mismatch"))?;
        if !plan.accepts_result_oid(actual) {
            return Err(SourceError::protocol("type_oid_mismatch"));
        }
        if raw.is_null() {
            cells.push(DecodedCell::sql_null());
            continue;
        }
        let bytes = raw
            .as_bytes()
            .map_err(|_| SourceError::protocol("binary_payload"))?;
        charge(used, max_bytes, bytes.len() as u64)?;
        cells.push(decode_payload(plan, bytes));
    }
    Ok(cells)
}

fn require_binary(format: PgValueFormat) -> Result<(), SourceError> {
    if format == PgValueFormat::Binary {
        Ok(())
    } else {
        Err(SourceError::protocol("text_format_refused"))
    }
}

async fn read_node(session: &mut PgSession) -> Result<NodeFact, SourceError> {
    let row = session::fetch_one(session, sqlx::query(sql::SNAPSHOT_NODE)).await?;
    Ok(NodeFact {
        in_recovery: session::cell_bool(&row, 0)?,
        postmaster_start: session::cell_string(&row, 1)?,
        database: session::cell_string(&row, 2)?,
        session_user: session::cell_string(&row, 3)?,
        current_user: session::cell_string(&row, 4)?,
    })
}

async fn read_backend(session: &mut PgSession) -> Result<(i64, String, bool), SourceError> {
    let row = session::fetch_one(session, sqlx::query(sql::SNAPSHOT_BACKEND)).await?;
    Ok((
        session::cell_i64(&row, 0)?,
        session::cell_string(&row, 1)?,
        session::cell_bool(&row, 2)?,
    ))
}

async fn read_relation(
    session: &mut PgSession,
    schema: &str,
    name: &str,
) -> Result<RelationFact, SourceError> {
    let rows = session::fetch_all(
        session,
        sqlx::query(sql::SNAPSHOT_RELATION).bind(schema).bind(name),
    )
    .await?;
    if rows.len() != 1 {
        return Err(SourceError::catalog("selected_table_missing"));
    }
    let row = &rows[0];
    Ok(RelationFact {
        oid: session::cell_i64(row, 0)?,
        relkind: session::cell_string(row, 1)?,
        rls: session::cell_bool(row, 2)?,
        forced: session::cell_bool(row, 3)?,
        select: session::cell_opt_bool(row, 4)?,
        insert: session::cell_opt_bool(row, 5)?,
        update: session::cell_opt_bool(row, 6)?,
        delete: session::cell_opt_bool(row, 7)?,
        truncate: session::cell_opt_bool(row, 8)?,
    })
}

async fn read_attributes(
    session: &mut PgSession,
    schema: &str,
    name: &str,
) -> Result<Vec<AttrFact>, SourceError> {
    let rows = session::fetch_all(
        session,
        sqlx::query(sql::SNAPSHOT_ATTRIBUTES).bind(schema).bind(name),
    )
    .await?;
    let mut attrs = Vec::with_capacity(rows.len());
    for row in &rows {
        attrs.push(AttrFact {
            attnum: session::cell_i32(row, 0)?,
            name: session::cell_string(row, 1)?,
            type_oid: session::cell_i64(row, 2)?,
            typmod: session::cell_i32(row, 3)?,
            not_null: session::cell_bool(row, 4)?,
            select: session::cell_opt_bool(row, 5)?,
        });
    }
    Ok(attrs)
}

async fn quote_one(session: &mut PgSession, name: &str) -> Result<String, SourceError> {
    let row = session::fetch_one(session, sqlx::query(sql::QUOTE_IDENT).bind(name)).await?;
    let quoted = session::cell_string(&row, 0)?;
    if !valid_quoted_ident(&quoted) {
        return Err(SourceError::catalog("quote_ident"));
    }
    Ok(quoted)
}

async fn load_type_graph(
    session: &mut PgSession,
    columns: &[SourceColumn],
) -> Result<TypeGraph, SourceError> {
    let mut graph = TypeGraph::default();
    let mut pending: Vec<i64> = columns.iter().map(|column| column.type_oid).collect();
    for depth in 0..=8 {
        pending.sort_unstable();
        pending.dedup();
        pending.retain(|oid| *oid != 0 && graph.get(*oid).is_none());
        if pending.is_empty() {
            break;
        }
        let rows = session::fetch_all(session, sqlx::query(sql::TYPE_FACTS).bind(&pending)).await?;
        if rows.len() != pending.len() {
            return Err(SourceError::catalog("type_facts_missing"));
        }
        let mut next = Vec::new();
        for row in &rows {
            let facts = type_facts(row)?;
            if depth < 8 {
                if facts.typelem != 0 {
                    next.push(facts.typelem);
                }
                if facts.typbasetype != 0 {
                    next.push(facts.typbasetype);
                }
            }
            graph.insert(facts);
        }
        pending = next;
    }
    let enum_oids: Vec<i64> = graph
        .iter()
        .filter(|facts| facts.typtype == "e")
        .map(|facts| facts.oid)
        .collect();
    if !enum_oids.is_empty() {
        let rows = session::fetch_all(session, sqlx::query(sql::ENUM_LABELS).bind(&enum_oids)).await?;
        apply_enum_labels(&mut graph, &rows)?;
    }
    Ok(graph)
}

fn apply_enum_labels(graph: &mut TypeGraph, rows: &[sqlx::postgres::PgRow]) -> Result<(), SourceError> {
    let mut grouped: Vec<(i64, Vec<String>)> = Vec::new();
    for row in rows {
        let oid = session::cell_i64(row, 0)?;
        let label = session::cell_string(row, 1)?;
        if let Some((_, labels)) = grouped.iter_mut().find(|(item, _)| *item == oid) {
            labels.push(label);
        } else {
            grouped.push((oid, vec![label]));
        }
    }
    for (oid, labels) in grouped {
        graph.set_enum_labels(oid, labels);
    }
    Ok(())
}

fn type_facts(row: &sqlx::postgres::PgRow) -> Result<TypeFacts, SourceError> {
    Ok(TypeFacts {
        oid: session::cell_i64(row, 0)?,
        name: session::cell_string(row, 1)?,
        schema: session::cell_string(row, 2)?,
        typtype: session::cell_string(row, 3)?,
        typlen: session::cell_i32(row, 4)?,
        typelem: session::cell_i64(row, 5)?,
        typbasetype: session::cell_i64(row, 6)?,
        typtypmod: session::cell_i32(row, 7)?,
        typsend_oid: session::cell_i64(row, 8)?,
        typsend_name: session::cell_string(row, 9)?,
        typsend_schema: session::cell_string(row, 10)?,
    })
}

async fn fail_open(session: PgSession, err: SourceError) -> SourceError {
    match release_session(session).await {
        Ok(()) => err,
        Err(cleanup) => cleanup,
    }
}

async fn fail_pair(data: PgSession, guard: PgSession, err: SourceError) -> SourceError {
    let data_res = release_session(data).await;
    let guard_res = release_session(guard).await;
    match (data_res, guard_res) {
        (Ok(()), Ok(())) => err,
        (Err(cleanup), _) => cleanup,
        (Ok(()), Err(cleanup)) => cleanup,
    }
}

async fn release_session(session: PgSession) -> Result<(), SourceError> {
    session.shutdown().await
}

struct ObservedAttr {
    attnum: i32,
    name: String,
    type_oid: i64,
    typmod: i32,
    not_null: bool,
    /// Selected-column `SELECT` from this row's table ACL and column ACL.
    select: Option<bool>,
}

struct ObservedTable {
    ord: i32,
    oid: i64,
    relkind: String,
    attrs: Vec<ObservedAttr>,
    pk_attnums: Vec<i32>,
    pk_names: Vec<String>,
}

fn observation_limit(catalog: &SourceCatalog) -> Result<i32, SourceError> {
    if catalog.tables.is_empty() {
        return Err(SourceError::catalog("schema_drift"));
    }
    let mut sum = 0_i64;
    for table in &catalog.tables {
        let width = i64::try_from(table.columns.len().max(1))
            .map_err(|_| SourceError::catalog("schema_drift"))?;
        sum = sum
            .checked_add(width)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| SourceError::catalog("schema_drift"))?;
    }
    i32::try_from(sum).map_err(|_| SourceError::catalog("schema_drift"))
}

async fn observe_current(
    guard: &mut PgSession,
    node: &NodeFact,
    catalog: &SourceCatalog,
    access: &[TableAccess],
) -> Result<(), SourceError> {
    let limit = observation_limit(catalog)?;
    let bound = u32::try_from(limit).map_err(|_| SourceError::catalog("schema_drift"))?;
    let schemas: Vec<String> = catalog.tables.iter().map(|table| table.schema.clone()).collect();
    let names: Vec<String> = catalog.tables.iter().map(|table| table.name.clone()).collect();
    guard.arm_rows(bound);
    let rows = session::fetch_all(
        guard,
        sqlx::query(sql::CURRENT_CATALOG)
            .bind(&schemas)
            .bind(&names)
            .bind(limit),
    )
    .await?;
    if rows.is_empty() {
        return Err(SourceError::catalog("schema_drift"));
    }
    for row in &rows {
        match_guard_node(row, node)?;
    }
    let row_count = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    if row_count >= i64::from(limit) {
        return Err(SourceError::catalog("schema_drift"));
    }
    let mut used = 0_u64;
    let max_bytes = guard.budget.max_bytes();
    let mut grouped: Vec<ObservedTable> = Vec::new();
    let mut current: Option<ObservedTable> = None;
    for row in &rows {
        let ord = session::cell_i32(row, 5)?;
        let charge_keys = current.as_ref().is_none_or(|open| open.ord != ord);
        let table = read_observed_row(row, &mut used, max_bytes, charge_keys)?;
        match current.as_mut() {
            Some(open) if open.ord == table.ord => {
                if open.oid != table.oid
                    || open.relkind != table.relkind
                    || open.pk_attnums != table.pk_attnums
                    || open.pk_names != table.pk_names
                {
                    return Err(SourceError::catalog("schema_drift"));
                }
                if table.attrs.is_empty() {
                    return Err(SourceError::catalog("schema_drift"));
                }
                open.attrs.extend(table.attrs);
            }
            _ => {
                if let Some(done) = current.take() {
                    grouped.push(done);
                }
                current = Some(table);
            }
        }
    }
    if let Some(done) = current {
        grouped.push(done);
    }
    if grouped.len() != catalog.tables.len() {
        return Err(SourceError::catalog("schema_drift"));
    }
    for (index, expected) in catalog.tables.iter().enumerate() {
        let got = &grouped[index];
        let want_ord = i32::try_from(index + 1).map_err(|_| SourceError::catalog("schema_drift"))?;
        if got.ord != want_ord || got.relkind != "r" {
            return Err(SourceError::catalog("schema_drift"));
        }
        let relation_oid = access
            .iter()
            .find(|item| item.schema == expected.schema && item.name == expected.name)
            .map(|item| item.relation_oid)
            .ok_or_else(|| SourceError::catalog("schema_drift"))?;
        if got.oid != relation_oid || !observed_attrs_match(&expected.columns, &got.attrs) {
            return Err(SourceError::catalog("schema_drift"));
        }
        let want_pk: Vec<(i32, &str)> = expected
            .primary_key
            .iter()
            .map(|key| (key.attnum, key.name.as_str()))
            .collect();
        let got_pk: Vec<(i32, &str)> = got
            .pk_attnums
            .iter()
            .copied()
            .zip(got.pk_names.iter().map(String::as_str))
            .collect();
        if want_pk != got_pk {
            return Err(SourceError::catalog("schema_drift"));
        }
        // Same row as the required role. The cell is the row's ACL, not
        // has_column_privilege. A later scan 42501 stays the race backstop.
        if got.attrs.iter().any(|attr| attr.select != Some(true)) {
            return Err(SourceError::privilege("select_not_granted"));
        }
    }
    drop(grouped);
    Ok(())
}

fn match_guard_node(row: &sqlx::postgres::PgRow, node: &NodeFact) -> Result<(), SourceError> {
    let in_recovery = session::cell_bool(row, 0)?;
    let postmaster = session::cell_string(row, 1)?;
    let database = session::cell_string(row, 2)?;
    let session_user = session::cell_string(row, 3)?;
    let current_user = session::cell_string(row, 4)?;
    if in_recovery != node.in_recovery || postmaster != node.postmaster_start {
        return Err(SourceError::protocol("read_node"));
    }
    if database != node.database
        || session_user != node.session_user
        || current_user != node.current_user
    {
        return Err(SourceError::protocol("identity_drift"));
    }
    Ok(())
}

fn read_observed_row(
    row: &sqlx::postgres::PgRow,
    used: &mut u64,
    max_bytes: u64,
    charge_keys: bool,
) -> Result<ObservedTable, SourceError> {
    let ord = session::cell_i32(row, 5)?;
    let Some(oid) = session::cell_opt_i64(row, 6)? else {
        return Err(SourceError::catalog("schema_drift"));
    };
    let Some(relkind) = session::cell_opt_string(row, 7)? else {
        return Err(SourceError::catalog("schema_drift"));
    };
    if relkind != "r" {
        return Err(SourceError::catalog("schema_drift"));
    }
    let relkind = retain_name(used, max_bytes, relkind, charge_keys)?;
    let pk_attnums = cell_i32_array(row, 13)?;
    let pk_name_cells = cell_text_array(row, 14)?;
    if pk_attnums.len() != pk_name_cells.len() {
        return Err(SourceError::catalog("schema_drift"));
    }
    let mut pk_names = Vec::with_capacity(pk_name_cells.len());
    for name in pk_name_cells {
        pk_names.push(retain_name(used, max_bytes, name, charge_keys)?);
    }
    let attr = read_observed_attr(row, used, max_bytes)?;
    Ok(ObservedTable {
        ord,
        oid,
        relkind,
        attrs: attr.into_iter().collect(),
        pk_attnums,
        pk_names,
    })
}

fn read_observed_attr(
    row: &sqlx::postgres::PgRow,
    used: &mut u64,
    max_bytes: u64,
) -> Result<Option<ObservedAttr>, SourceError> {
    let attnum = session::cell_opt_i32(row, 8)?;
    let name = session::cell_opt_string(row, 9)?;
    let type_oid = session::cell_opt_i64(row, 10)?;
    let typmod = session::cell_opt_i32(row, 11)?;
    let not_null = session::cell_opt_bool(row, 12)?;
    let select = session::cell_opt_bool(row, 15)?;
    match (attnum, name, type_oid, typmod, not_null) {
        (None, None, None, None, None) => Ok(None),
        (Some(attnum), Some(name), Some(type_oid), Some(typmod), Some(not_null)) => {
            if attnum <= 0 || name.is_empty() {
                return Err(SourceError::catalog("schema_drift"));
            }
            Ok(Some(ObservedAttr {
                attnum,
                name: retain_name(used, max_bytes, name, true)?,
                type_oid,
                typmod,
                not_null,
                select,
            }))
        }
        _ => Err(SourceError::catalog("decode")),
    }
}

fn observed_attrs_match(want: &[SourceColumn], got: &[ObservedAttr]) -> bool {
    if want.len() != got.len() {
        return false;
    }
    let mut want: Vec<&SourceColumn> = want.iter().collect();
    want.sort_by_key(|column| column.attnum);
    let mut got: Vec<&ObservedAttr> = got.iter().collect();
    got.sort_by_key(|attr| attr.attnum);
    if got.windows(2).any(|pair| pair[0].attnum == pair[1].attnum) {
        return false;
    }
    want.iter().zip(got.iter()).all(|(want, got)| {
        want.attnum == got.attnum
            && want.name == got.name
            && want.type_oid == got.type_oid
            && want.typmod == got.typmod
            && want.not_null == got.not_null
    })
}

fn retain_name(
    used: &mut u64,
    max_bytes: u64,
    name: String,
    charge_bytes: bool,
) -> Result<String, SourceError> {
    if name.is_empty() || name.len() > 63 || name.as_bytes().contains(&0) {
        return Err(SourceError::catalog("schema_drift"));
    }
    if charge_bytes {
        charge(used, max_bytes, name.len() as u64)?;
    }
    Ok(name)
}

fn cell_i32_array(row: &sqlx::postgres::PgRow, index: usize) -> Result<Vec<i32>, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

fn cell_text_array(row: &sqlx::postgres::PgRow, index: usize) -> Result<Vec<String>, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

fn ensure_deadline(deadline: Instant) -> Result<(), SourceError> {
    if Instant::now() >= deadline {
        Err(SourceError::budget("deadline"))
    } else {
        Ok(())
    }
}

fn charge(used: &mut u64, max: u64, extra: u64) -> Result<(), SourceError> {
    let next = used
        .checked_add(extra)
        .ok_or_else(|| SourceError::budget("max_bytes"))?;
    if next > max {
        return Err(SourceError::budget("max_bytes"));
    }
    *used = next;
    Ok(())
}

fn push_line(body: &mut String, key: &str, value: &str) {
    body.push_str(key);
    body.push('=');
    body.push_str(value);
    body.push('\n');
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    let hashed = Sha256::digest(bytes);
    let mut out = [0_u8; 32];
    out.copy_from_slice(&hashed);
    out
}

fn decode_hex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0_u8; 32];
    for index in 0..32 {
        out[index] = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::config::{
        PgBudget, PgCredentials, PgEndpoint, PgTlsConfig, SecretRef, SelectedTable, SourceId,
        TableSelection,
    };

    fn pem() -> &'static [u8] {
        b"-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n"
    }

    fn other_pem() -> &'static [u8] {
        b"-----BEGIN CERTIFICATE-----\nMIIBXX\n-----END CERTIFICATE-----\n"
    }

    fn config(host: &str, password: &str, pem_bytes: &[u8]) -> PgSourceConfig {
        PgSourceConfig::try_new(
            SourceId::new("snap_src").unwrap(),
            PgEndpoint::new(host, 5432, "snapdb").unwrap(),
            PgCredentials::new("oo_snap_reader", password).unwrap(),
            SecretRef::new("secret://pg/snap/reader").unwrap(),
            PgTlsConfig::from_ca_pem(pem_bytes).unwrap(),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(30),
                Duration::from_secs(5),
                1000,
                1024 * 1024,
            )
            .unwrap(),
            TableSelection::new(vec![
                SelectedTable::new("snap", "parent").unwrap(),
                SelectedTable::new("snap", "child").unwrap(),
            ])
            .unwrap(),
        )
        .unwrap()
    }

    fn admitted() -> AdmittedCatalog {
        AdmittedCatalog {
            tables: vec![AdmittedTable {
                schema: "snap".to_string(),
                name: "parent".to_string(),
                columns: vec![AdmittedColumn {
                    attnum: 1,
                    name: "id".to_string(),
                    type_oid: OID_INT4,
                    typmod: -1,
                    not_null: true,
                }],
                primary_key: vec!["id".to_string()],
            }],
        }
    }

    #[test]
    fn config_identity_omits_password_and_ca() {
        let left = config_identity(&config("oo-snap-pg", "one-secret", pem()));
        let right = config_identity(&config("oo-snap-pg", "other-secret", other_pem()));
        assert_eq!(left, right);
        let moved = config_identity(&config("oo-snap-other", "one-secret", pem()));
        assert_ne!(left, moved);
        let text = format!("{:?}", config("oo-snap-pg", "one-secret", pem()));
        assert!(!text.contains("one-secret"));
        assert!(!text.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn type_policy_identity_tracks_the_unsupported_policy() {
        let preserve = type_policy_identity(PgTypePolicy::PRESERVE_ROW);
        let fail = type_policy_identity(PgTypePolicy {
            unsupported: UnsupportedColumnPolicy::FailRead,
            reject: crate::types::RejectCellPolicy::PreserveRow,
        });
        assert_ne!(preserve, fail);
    }

    #[test]
    fn input_digest_uses_schema_hash_bytes_and_generation() {
        let config_id = [1_u8; 32];
        let policy_id = [2_u8; 32];
        let hash = "ab".repeat(32);
        let first = snapshot_input_digest(&config_id, &policy_id, &hash, 1).unwrap();
        let second = snapshot_input_digest(&config_id, &policy_id, &hash, 2).unwrap();
        assert_ne!(first, second);
        assert!(snapshot_input_digest(&config_id, &policy_id, "zz", 1).is_err());
    }

    #[tokio::test]
    async fn local_refusals_do_not_connect() {
        let source = config("oo-snap-pg", "one-secret", pem());
        let mut request = SnapshotRequest {
            config_identity: config_identity(&source),
            type_policy_identity: type_policy_identity(PgTypePolicy::PRESERVE_ROW),
            config: source,
            admitted: admitted(),
            policy: PgTypePolicy::PRESERVE_ROW,
            generation: 0,
            deadline: Instant::now() + Duration::from_secs(30),
            expected_principal: "oo_snap_reader".to_string(),
            expected_database: "snapdb".to_string(),
            expected_in_recovery: false,
            expected_postmaster_start: "unused".to_string(),
        };
        let err = SnapshotOwner::open(request.clone()).await.unwrap_err();
        assert_eq!(err.fact(), Some("generation_invalid"));
        request.generation = 1;
        request.config_identity[0] ^= 0xff;
        let err = SnapshotOwner::open(request.clone()).await.unwrap_err();
        assert_eq!(err.fact(), Some("config_identity"));
        request.config_identity = config_identity(&request.config);
        request.type_policy_identity[0] ^= 0xff;
        let err = SnapshotOwner::open(request.clone()).await.unwrap_err();
        assert_eq!(err.fact(), Some("type_policy_identity"));
        request.type_policy_identity = type_policy_identity(PgTypePolicy::PRESERVE_ROW);
        request.deadline = Instant::now().checked_sub(Duration::from_secs(5)).unwrap();
        let err = SnapshotOwner::open(request).await.unwrap_err();
        assert_eq!(err.fact(), Some("deadline"));
    }

    #[test]
    fn cap_refusal_does_not_consume_generation() {
        let _ledger = super::session::lock_ledger();
        let held = super::session::try_reserve("cap_src", 4).unwrap();
        let source = single_config("cap_src");
        let mut request = SnapshotRequest {
            config_identity: config_identity(&source),
            type_policy_identity: type_policy_identity(PgTypePolicy::PRESERVE_ROW),
            config: source,
            admitted: admitted(),
            policy: PgTypePolicy::PRESERVE_ROW,
            generation: 0,
            deadline: Instant::now() + Duration::from_secs(30),
            expected_principal: "oo_snap_reader".to_string(),
            expected_database: "snapdb".to_string(),
            expected_in_recovery: false,
            expected_postmaster_start: "unused".to_string(),
        };
        let err = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(SnapshotOwner::open(request.clone()))
            .unwrap_err();
        assert_eq!(err.fact(), Some("generation_invalid"));
        assert!(super::session::try_reserve("cap_src", 1).is_err());
        drop(held);
        let held = super::session::try_reserve("cap_src", 3).unwrap();
        request.generation = 41;
        let err = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(SnapshotOwner::open(request))
            .unwrap_err();
        assert_eq!(err.fact(), Some("max_connections"));
        let generations = super::GENERATIONS.lock().unwrap_or_else(|err| err.into_inner());
        assert!(!generations.contains(&("cap_src".to_string(), 41)));
        drop(generations);
        drop(held);
    }

    #[test]
    fn no_key_and_wide_key_selection_does_not_connect() {
        let source = PgSourceConfig::try_new(
            SourceId::new("snap_src").unwrap(),
            PgEndpoint::new("oo-snap-pg", 5432, "snapdb").unwrap(),
            PgCredentials::new("oo_snap_reader", "one-secret").unwrap(),
            SecretRef::new("secret://pg/snap/reader").unwrap(),
            PgTlsConfig::from_ca_pem(pem()).unwrap(),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(30),
                Duration::from_secs(5),
                1000,
                1024 * 1024,
            )
            .unwrap(),
            TableSelection::new(vec![
                SelectedTable::new("snap", "nokey").unwrap(),
                SelectedTable::new("snap", "wide").unwrap(),
                SelectedTable::new("snap", "zero").unwrap(),
            ])
            .unwrap(),
        )
        .unwrap();
        let mut wide_columns = Vec::new();
        let mut wide_key = Vec::new();
        for index in 1..=9 {
            let name = format!("c{index}");
            wide_columns.push(AdmittedColumn {
                attnum: index,
                name: name.clone(),
                type_oid: OID_INT4,
                typmod: -1,
                not_null: true,
            });
            wide_key.push(name);
        }
        let admitted = AdmittedCatalog {
            tables: vec![
                AdmittedTable {
                    schema: "snap".to_string(),
                    name: "nokey".to_string(),
                    columns: vec![AdmittedColumn {
                        attnum: 1,
                        name: "note".to_string(),
                        type_oid: OID_TEXT,
                        typmod: -1,
                        not_null: false,
                    }],
                    primary_key: Vec::new(),
                },
                AdmittedTable {
                    schema: "snap".to_string(),
                    name: "wide".to_string(),
                    columns: wide_columns,
                    primary_key: wide_key,
                },
                AdmittedTable {
                    schema: "snap".to_string(),
                    name: "zero".to_string(),
                    columns: Vec::new(),
                    primary_key: Vec::new(),
                },
            ],
        };
        selection_matches(&source, &admitted).expect("no-key and wide key stay selectable");
        let mut missing = admitted.clone();
        missing.tables[0].primary_key.push("absent".to_string());
        let err = selection_matches(&source, &missing).expect_err("missing key column");
        assert_eq!(err.fact(), Some("probe_key"));
        let mut too_wide = admitted;
        let extra: Vec<String> = (10..=33).map(|index| format!("c{index}")).collect();
        too_wide.tables[1].primary_key.extend(extra);
        let err = selection_matches(&source, &too_wide).expect_err("past index max");
        assert_eq!(err.fact(), Some("selection"));
    }

    fn single_config(source_id: &str) -> PgSourceConfig {
        PgSourceConfig::try_new(
            SourceId::new(source_id).unwrap(),
            PgEndpoint::new("oo-snap-pg", 5432, "snapdb").unwrap(),
            PgCredentials::new("oo_snap_reader", "one-secret").unwrap(),
            SecretRef::new("secret://pg/snap/reader").unwrap(),
            PgTlsConfig::from_ca_pem(pem()).unwrap(),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(30),
                Duration::from_secs(5),
                1000,
                1024 * 1024,
            )
            .unwrap(),
            TableSelection::new(vec![SelectedTable::new("snap", "parent").unwrap()]).unwrap(),
        )
        .unwrap()
    }
}
