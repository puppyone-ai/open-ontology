//! Permission-bound `SELECT` for one trusted runtime.
//!
//! [`DedicatedSession::execute_trusted_select`] is the only entry. It calls
//! [`crate::sql_guard::guard_select`] on the caller's original SQL and runs
//! `GuardedPlan::executed_sql`. The caller has no field for executed SQL.
//!
//! Untrusted parser admission stays closed. A 500 ms process-isolated parser
//! worker does not exist in this crate, so [`UNTRUSTED_ADMISSION`] is
//! [`UntrustedAdmission::Blocked`] and there is no public function that would
//! accept an untrusted statement.
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
//! Dropping the stream sets shutdown. A database call that is already polled
//! is cancelled on the bound backend and kept polled until the server answers
//! or 750 ms passes. A call that has not been polled is not started. A
//! consumed answer is rolled back. A client deadline, or a call that does not
//! finish inside that cleanup bound, drops the socket and does not claim a
//! rollback.
//! Failure drops the socket even after a confirmed rollback. A late
//! [`CancelHandle`] carries the generation captured when the query started.
//! The next query bumps that generation, so the old handle does not call
//! `cancel_query`.
//!
//! The audit trait must return before the transaction begins. The hook in the
//! live fixture writes through a second client. That callback is not a
//! production audit WAL. Guard refusal does not call the hook. If the stream
//! is aborted, completion is `partial` or `failed`, never `succeeded`.
//!
//! Result objects are built here. Another crate cannot construct
//! [`QuerySummary`]. Cell text is the driver's typed value for int2, int4,
//! int8, text, bool, and null. Every other result OID is refused before the
//! portal is executed.

use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_postgres::{Client, Error as PgError, IsolationLevel, Row, Transaction};

use crate::policy::TlsConnection;
use crate::sql_guard::{
    guard_select, GuardError, GuardedPlan, PolicyCatalog, PolicyIdentity,
};
use oo_core::api::ErrorCode;

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
const OID_INT8: u32 = 20;
const OID_INT2: u32 = 21;
const OID_INT4: u32 = 23;
const OID_TEXT: u32 = 25;
const TIMEOUT_MARGIN: Duration = Duration::from_millis(250);

/// How long a dropped call may wait for the canceled server response and for
/// `ROLLBACK`. This is not a per-page query budget and does not refresh the
/// whole-query [`Instant`].
const CLEANUP_BOUND: Duration = Duration::from_millis(750);

/// Why an untrusted statement cannot be admitted by this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UntrustedAdmission {
    /// No 500 ms process-isolated parser worker is implemented.
    Blocked,
}

/// Untrusted admission is unreachable until that worker exists.
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

/// One decoded cell. Integers are canonical decimal text from the typed value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    /// SQL NULL.
    Null,
    /// `bool`.
    Bool(bool),
    /// int2, int4, or int8 rendered with `Display`.
    Int(String),
    /// `text` only. `varchar` is refused before decode.
    Text(String),
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
        EndGeneration { shared, armed: true }
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
        let shared = Arc::new(Mutex::new(Shared {
            generation: 0,
            accepting_cancel: false,
            backend_pid: bound.0,
            backend_start: bound.1,
            bound_role: None,
            open: true,
            summary: None,
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
        let limits = Limits {
            max_rows: request.max_rows,
            max_result_bytes: request.max_result_bytes,
            wall,
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

    /// Refuse later cancels, then drop the socket. This does not claim a rollback.
    pub fn close(&mut self) {
        {
            let mut guard = lock(&self.shared);
            guard.accepting_cancel = false;
            guard.open = false;
        }
        self.client.take();
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
    /// The gate is checked after that observation returns and before
    /// `cancel_query`. There is no await between the check and the cancel call.
    pub async fn cancel(&self, observer: &Client) -> Result<CancelOutcome, ExecError> {
        let observed = observe_backend(observer, self.pid).await?;
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
        if !send {
            return Ok(CancelOutcome::NotSent);
        }
        self.token
            .cancel_query(self.tls.clone())
            .await
            .map_err(|_| ExecError::Protocol { step: "cancel" })?;
        Ok(CancelOutcome::Sent)
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
        &rows,
    )
    .await;
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
    rows: &mpsc::Sender<StreamItem>,
) -> Attempt {
    let backend_start = lock(shared).backend_start.clone();
    let generation = lock(shared).generation;
    let pid = lock(shared).backend_pid;
    let mut hot = HotWait {
        shutdown,
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
    };
    let tx = match open_read_only(client, &mut hot).await {
        Ok(tx) => tx,
        Err(kind) => {
            if kind.desync {
                send_bound_cancel(&hot.cancel).await;
            }
            return publish_stop(rows, kind, false).await;
        }
    };
    if shutdown_requested(hot.shutdown) {
        return cleanup(tx, rows, consumer_stop(0, 0), &hot).await;
    }
    if let Err(kind) = apply_session(&tx, role, &mut hot).await {
        return cleanup(tx, rows, kind, &hot).await;
    }
    let statement = match prepare_user(&tx, executed_sql, &mut hot).await {
        Ok(statement) => statement,
        Err(kind) => return cleanup(tx, rows, kind, &hot).await,
    };
    if let Err(err) = allowed_result_types(&statement) {
        let kind = StopKind::synced(err);
        return cleanup(tx, rows, kind, &hot).await;
    }
    if !statement.params().is_empty() {
        let kind = StopKind::synced(ExecError::Protocol { step: "parameters" });
        return cleanup(tx, rows, kind, &hot).await;
    }
    let portal = match bind_portal(&tx, &statement, &mut hot).await {
        Ok(portal) => portal,
        Err(kind) => return cleanup(tx, rows, kind, &hot).await,
    };
    let read = read_pages(&tx, &portal, rows, &mut hot, limits).await;
    match read {
        Read::Finished {
            truncated,
            returned_rows,
            result_bytes,
        } => {
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
        Read::Stop(kind) => cleanup(tx, rows, kind, &hot).await,
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
    cancel: BoundCancel<'a>,
    started: Instant,
    wall: Duration,
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
/// would leave the portal response on the socket. The cleanup sleep starts
/// only after a bound cancel and does not replace the whole-query budget.
async fn await_hot<T>(future: impl Future<Output = T> + Send, hot: &mut HotWait<'_>) -> Hot<T> {
    if shutdown_requested(hot.shutdown) {
        return Hot::Consumer;
    }
    let Some(remain) = remaining(hot.started, hot.wall) else {
        return Hot::NotStartedDeadline;
    };
    let shutdown = &mut *hot.shutdown;
    let cancel = &hot.cancel;
    let mut future = std::pin::pin!(future);
    let mut deadline = std::pin::pin!(tokio::time::sleep(remain));
    let mut cancel_sent = false;
    loop {
        if cancel_sent {
            let outcome = tokio::select! {
                biased;
                result = future.as_mut() => Hot::Ready(result),
                () = tokio::time::sleep(CLEANUP_BOUND) => Hot::Abandoned,
            };
            return outcome;
        }
        let outcome = tokio::select! {
            biased;
            changed = shutdown.changed() => changed.is_err(),
            result = future.as_mut() => return Hot::Ready(result),
            () = deadline.as_mut() => {
                send_bound_cancel(cancel).await;
                return Hot::Deadline;
            }
        };
        if outcome || shutdown_requested(shutdown) {
            send_bound_cancel(cancel).await;
            cancel_sent = true;
        }
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
/// No second session is opened.
async fn send_bound_cancel(cancel: &BoundCancel<'_>) {
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
        return;
    }
    let tls = cancel.tls.clone();
    let _ = cancel.token.cancel_query(tls).await;
}

async fn cleanup(
    tx: Transaction<'_>,
    rows: &mpsc::Sender<StreamItem>,
    kind: StopKind,
    hot: &HotWait<'_>,
) -> Attempt {
    if kind.desync {
        send_bound_cancel(&hot.cancel).await;
        drop(tx);
        return publish_stop(rows, kind, false).await;
    }
    let confirmed = if kind.partial {
        rollback_bounded(tx, CLEANUP_BOUND).await
    } else {
        rollback_confirmed(tx, hot.started, hot.wall).await
    };
    publish_stop(rows, kind, confirmed).await
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
        let row_bytes = cells.iter().map(cell_bytes).fold(0_u64, u64::saturating_add);
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
        return Err(StopKind::synced(ExecError::Protocol { step: "current_user" }));
    }
    if !search_path_empty(&search_path) {
        return Err(StopKind::synced(ExecError::Protocol { step: "search_path" }));
    }
    if timeout_setting != ms.to_string() {
        return Err(StopKind::synced(ExecError::Protocol { step: "statement_timeout" }));
    }
    Ok(())
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
        cells.push(decode_cell(row, index, column.type_().oid())?);
    }
    Ok(cells)
}

fn decode_cell(row: &Row, index: usize, oid: u32) -> Result<Cell, ExecError> {
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
        OID_TEXT => match row
            .try_get::<_, Option<String>>(index)
            .map_err(|_| ExecError::Protocol { step: "decode" })?
        {
            None => Ok(Cell::Null),
            Some(value) => Ok(Cell::Text(value)),
        },
        other => Err(ExecError::TypeRefused { oid: other }),
    }
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
    if deadline_owned
        && let ExecError::Canceled { sqlstate } = mapped
    {
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
    wall.checked_sub(started.elapsed()).filter(|remain| !remain.is_zero())
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
    matches!(oid, OID_BOOL | OID_INT8 | OID_INT2 | OID_INT4 | OID_TEXT)
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
        assert_eq!(role_bind_decision(None, "oo_r_", "oo_r_"), RoleBind::Invalid);
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
    fn role_shape_accepts_only_the_nologin_prefix() {
        assert!(role_shape_ok("oo_r_reader_a"));
        assert_eq!(quote_role("oo_r_reader_a").as_deref(), Ok("\"oo_r_reader_a\""));
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
        assert!(matches!(int_cell(Some(i64::MIN)), Ok(Cell::Int(text)) if text == "-9223372036854775808"));
    }

    #[test]
    fn bootstrap_oids_match_the_driver_and_other_types_are_refused() {
        assert_eq!(Type::BOOL.oid(), OID_BOOL);
        assert_eq!(Type::INT8.oid(), OID_INT8);
        assert_eq!(Type::INT2.oid(), OID_INT2);
        assert_eq!(Type::INT4.oid(), OID_INT4);
        assert_eq!(Type::TEXT.oid(), OID_TEXT);
        assert!(oid_allowed(Type::BOOL.oid()));
        assert!(oid_allowed(Type::INT8.oid()));
        assert!(!oid_allowed(Type::VARCHAR.oid()));
        assert!(!oid_allowed(Type::NUMERIC.oid()));
        assert!(!oid_allowed(705));
        assert_eq!(PORTAL_PAGE_ROWS, 1);
        assert_eq!(UNTRUSTED_ADMISSION, UntrustedAdmission::Blocked);
    }

    #[test]
    fn error_codes_use_the_existing_enum_and_hide_server_text() {
        assert_eq!(ExecError::AuditUnavailable.code(), ErrorCode::AuditUnavailable);
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
}
