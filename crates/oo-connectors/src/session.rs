//! One read-only PostgreSQL session. The connection is not pooled and is not
//! printed: the driver options retain the password.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode, PgTransactionManager};
use sqlx::{ConnectOptions, Connection, Row};
use sqlx_core::sql_str::SqlSafeStr;
use sqlx_core::transaction::TransactionManager;

use crate::config::{PgSourceConfig, TableSelection};
use crate::error::{SourceError, map_connect, map_sqlx};
use crate::model::{
    AccessReport, BlockReason, CdcPrerequisite, DbaItem, Fact, VersionFact, WriteDetermination,
    admit_version, connector_capabilities, tls_report,
};
use crate::sql;

const MAX_CONNECTIONS: u8 = 4;
pub(crate) const MAX_ROWS_PER_SEC: u32 = 50_000;
const ROW_WINDOW: Duration = Duration::from_secs(1);
const MAX_TRACKED_SOURCES: usize = 64;
/// PostgreSQL `MaxHeapAttributeNumber`.
pub(crate) const MAX_HEAP_ATTRIBUTES: u32 = 1_600;
/// PostgreSQL `INDEX_MAX_KEYS`.
pub(crate) const INDEX_MAX_KEYS: u32 = 32;
/// Unarmed `fetch_all` (snapshot relation reads) reserves the attribute ceiling.
pub(crate) const UNARMED_FETCH_LIMIT: u32 = MAX_HEAP_ATTRIBUTES;
/// Sentinel index count for unique and foreign-key rows. Not a server maximum.
pub(crate) const KEY_INDEX_SENTINEL: u32 = 64;
pub(crate) const KEY_ROW_SENTINEL: u32 = KEY_INDEX_SENTINEL * INDEX_MAX_KEYS;
pub(crate) const POLICY_SENTINEL: u32 = 128;
pub(crate) const ACL_SENTINEL: u32 = 256;
pub(crate) const ENUM_LABEL_SENTINEL: u32 = 4_096;
pub(crate) const DOMAIN_CHECK_SENTINEL: u32 = 32;
/// One past the membership width that still registers as incomplete.
pub(crate) const ASSUMABLE_LIMIT: u32 = 65;
pub(crate) const DIRECT_MEMBER_SENTINEL: u32 = 256;
pub(crate) const CATALOG_LIST_SENTINEL: u32 = 1_024;

struct SourceEntry {
    in_use: u8,
    rows_left: u32,
    window_started: Instant,
}

static LEDGER: Mutex<BTreeMap<String, SourceEntry>> = Mutex::new(BTreeMap::new());

pub(crate) struct ConnPermit {
    source_id: String,
}

impl Drop for ConnPermit {
    fn drop(&mut self) {
        release_slot(&self.source_id);
    }
}

fn ledger() -> Result<std::sync::MutexGuard<'static, BTreeMap<String, SourceEntry>>, SourceError> {
    LEDGER.lock().map_err(|_| SourceError::budget("max_connections"))
}

fn prepare(entries: &mut BTreeMap<String, SourceEntry>, now: Instant) {
    entries.retain(|_, entry| {
        entry.in_use > 0 || now.duration_since(entry.window_started) < ROW_WINDOW
    });
    for entry in entries.values_mut() {
        if now.duration_since(entry.window_started) >= ROW_WINDOW {
            entry.rows_left = MAX_ROWS_PER_SEC;
            entry.window_started = now;
        }
    }
}

fn fresh_entry(now: Instant) -> SourceEntry {
    SourceEntry {
        in_use: 0,
        rows_left: MAX_ROWS_PER_SEC,
        window_started: now,
    }
}

pub(crate) fn try_reserve(source_id: &str, count: u8) -> Result<Vec<ConnPermit>, SourceError> {
    if count == 0 || count > MAX_CONNECTIONS {
        return Err(SourceError::budget("max_connections"));
    }
    let mut entries = ledger()?;
    let now = Instant::now();
    prepare(&mut entries, now);
    let entry = if let Some(entry) = entries.get_mut(source_id) {
        entry
    } else if entries.len() >= MAX_TRACKED_SOURCES {
        return Err(SourceError::budget("max_connections"));
    } else {
        entries.insert(source_id.to_string(), fresh_entry(now));
        entries.get_mut(source_id).ok_or_else(|| SourceError::budget("max_connections"))?
    };
    if u16::from(entry.in_use) + u16::from(count) > u16::from(MAX_CONNECTIONS) {
        return Err(SourceError::budget("max_connections"));
    }
    entry.in_use = entry.in_use.saturating_add(count);
    Ok((0..count)
        .map(|_| ConnPermit {
            source_id: source_id.to_string(),
        })
        .collect())
}

pub(crate) fn try_charge_rows(source_id: &str, rows: u32) -> Result<(), SourceError> {
    if rows == 0 {
        return Ok(());
    }
    debit_rows(source_id, rows).map(|_| ())
}

fn debit_rows(source_id: &str, rows: u32) -> Result<Instant, SourceError> {
    let mut entries = LEDGER.lock().map_err(|_| SourceError::budget("max_rows_per_sec"))?;
    let now = Instant::now();
    prepare(&mut entries, now);
    let entry = if let Some(entry) = entries.get_mut(source_id) {
        entry
    } else if entries.len() >= MAX_TRACKED_SOURCES {
        return Err(SourceError::budget("max_connections"));
    } else {
        entries.insert(source_id.to_string(), fresh_entry(now));
        entries.get_mut(source_id).ok_or_else(|| SourceError::budget("max_rows_per_sec"))?
    };
    if entry.rows_left < rows {
        return Err(SourceError::budget("max_rows_per_sec"));
    }
    entry.rows_left -= rows;
    Ok(entry.window_started)
}

/// Debit held until `settle`. Dropping the hold keeps the full debit.
pub(crate) struct RowHold {
    source_id: String,
    reserved: u32,
    window_started: Instant,
}

impl RowHold {
    /// Release `reserved - observed` only while this window is still the active one.
    pub(crate) fn settle(self, observed: usize) {
        let observed = u32::try_from(observed).unwrap_or(self.reserved);
        if observed >= self.reserved {
            return;
        }
        settle_rows(&self.source_id, self.reserved - observed, self.window_started);
    }
}

pub(crate) fn try_reserve_rows(source_id: &str, rows: u32) -> Result<RowHold, SourceError> {
    if rows == 0 || rows > MAX_ROWS_PER_SEC {
        return Err(SourceError::budget("max_rows_per_sec"));
    }
    let window_started = debit_rows(source_id, rows)?;
    Ok(RowHold {
        source_id: source_id.to_string(),
        reserved: rows,
        window_started,
    })
}

/// Refund unused allowance only into the same open window. An expired, refilled,
/// evicted, or poisoned entry keeps the debit. A later window is not credited.
fn settle_rows(source_id: &str, refund: u32, window_started: Instant) {
    if refund == 0 {
        return;
    }
    let Ok(mut entries) = LEDGER.lock() else {
        return;
    };
    let Some(entry) = entries.get_mut(source_id) else {
        return;
    };
    if entry.window_started != window_started {
        return;
    }
    if Instant::now().duration_since(entry.window_started) >= ROW_WINDOW {
        return;
    }
    let room = MAX_ROWS_PER_SEC.saturating_sub(entry.rows_left);
    entry.rows_left = entry.rows_left.saturating_add(refund.min(room));
}

/// Row bound chosen before SQL is sent.
#[derive(Clone, Copy)]
pub(crate) struct AdmittedLimit {
    pub limit: u32,
    pub natural: bool,
}

pub(crate) enum Admit {
    /// Reserve `wanted` in full or refuse before SQL.
    Exact { wanted: u32, natural: bool },
    /// Reserve `min(wanted, window, available)`. Natural only when `wanted` itself is reserved.
    Ceiling { wanted: u32, natural_when_full: bool },
}

pub(crate) fn admit_limit(source_id: &str, admit: Admit) -> Result<AdmittedLimit, SourceError> {
    let available = rows_available(source_id)?;
    match admit {
        Admit::Exact { wanted, natural } => {
            if wanted == 0 || wanted > MAX_ROWS_PER_SEC || available < wanted {
                return Err(SourceError::budget("max_rows_per_sec"));
            }
            Ok(AdmittedLimit {
                limit: wanted,
                natural,
            })
        }
        Admit::Ceiling {
            wanted,
            natural_when_full,
        } => {
            let capped = wanted.min(MAX_ROWS_PER_SEC);
            if capped == 0 || available == 0 {
                return Err(SourceError::budget("max_rows_per_sec"));
            }
            let limit = capped.min(available);
            Ok(AdmittedLimit {
                limit,
                natural: natural_when_full && limit == wanted,
            })
        }
    }
}

fn rows_available(source_id: &str) -> Result<u32, SourceError> {
    let mut entries = LEDGER.lock().map_err(|_| SourceError::budget("max_rows_per_sec"))?;
    let now = Instant::now();
    prepare(&mut entries, now);
    if let Some(entry) = entries.get(source_id) {
        Ok(entry.rows_left)
    } else if entries.len() >= MAX_TRACKED_SOURCES {
        Err(SourceError::budget("max_connections"))
    } else {
        Ok(MAX_ROWS_PER_SEC)
    }
}

/// `count * per` when that product fits in `u32`. The bool is true when the
/// product also fits in one rate window, so equality can be a complete result.
pub(crate) fn scale_rows(count: usize, per: u32) -> (u32, bool) {
    let Some(count) = u32::try_from(count).ok() else {
        return (u32::MAX, false);
    };
    match count.checked_mul(per) {
        Some(0) => (1, true),
        Some(product) if product <= MAX_ROWS_PER_SEC => (product, true),
        Some(product) => (product, false),
        None => (u32::MAX, false),
    }
}

/// Natural cardinality of a count or a scan product. Zero reserves one row.
/// A product that does not fit the window reserves the window and is not natural.
pub(crate) fn admit_cardinality(source_id: &str, natural_max: i64) -> Result<AdmittedLimit, SourceError> {
    if natural_max < 0 {
        return Err(SourceError::budget("max_rows_per_sec"));
    }
    if natural_max == 0 {
        return admit_limit(source_id, Admit::Exact { wanted: 1, natural: true });
    }
    match u32::try_from(natural_max) {
        Ok(wanted) if wanted <= MAX_ROWS_PER_SEC => {
            admit_limit(source_id, Admit::Exact { wanted, natural: true })
        }
        _ => admit_limit(
            source_id,
            Admit::Ceiling {
                wanted: MAX_ROWS_PER_SEC,
                natural_when_full: false,
            },
        ),
    }
}

pub(crate) fn limit_arg(limit: u32) -> i64 {
    i64::from(limit)
}

/// Embedded reserved keywords plus one, so a longer server list cannot look complete.
pub(crate) fn reserved_keyword_limit() -> u32 {
    u32::try_from(crate::ident::PG17_RESERVED.len())
        .unwrap_or(u32::MAX)
        .saturating_add(1)
}

pub(crate) fn filled_sentinel(admitted: &AdmittedLimit, len: usize) -> bool {
    let limit = usize::try_from(admitted.limit).unwrap_or(usize::MAX);
    len > limit || (!admitted.natural && len == limit)
}

pub(crate) fn reject_if_truncated(
    len: usize,
    admitted: &AdmittedLimit,
    fact: &'static str,
) -> Result<(), SourceError> {
    if filled_sentinel(admitted, len) {
        Err(SourceError::catalog(fact))
    } else {
        Ok(())
    }
}

#[cfg(test)]
fn raw_rows_left(source_id: &str) -> Option<u32> {
    let entries = LEDGER.lock().ok()?;
    entries.get(source_id).map(|entry| entry.rows_left)
}

fn release_slot(source_id: &str) {
    let Ok(mut entries) = LEDGER.lock() else {
        return;
    };
    if let Some(entry) = entries.get_mut(source_id) {
        entry.in_use = entry.in_use.saturating_sub(1);
    }
}

#[cfg(test)]
pub(crate) struct LedgerLock {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
pub(crate) fn lock_ledger() -> LedgerLock {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = match LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Ok(mut entries) = LEDGER.lock() {
        entries.clear();
    }
    LedgerLock { _guard: guard }
}

#[cfg(test)]
impl Drop for LedgerLock {
    fn drop(&mut self) {
        if let Ok(mut entries) = LEDGER.lock() {
            entries.clear();
        }
    }
}

#[cfg(test)]
pub(crate) fn age_ledger_window(source_id: &str, by: Duration) {
    let Ok(mut entries) = LEDGER.lock() else {
        return;
    };
    if let Some(entry) = entries.get_mut(source_id)
        && let Some(started) = entry.window_started.checked_sub(by)
    {
        entry.window_started = started;
    }
}

/// Configured connector. `connect` opens a fresh session every time.
pub struct PgConnector {
    config: PgSourceConfig,
}

impl PgConnector {
    /// Store a validated configuration.
    pub fn new(config: PgSourceConfig) -> Self {
        Self { config }
    }

    /// Open one TLS session and read the server version.
    ///
    /// An untested version still returns a session. `inspect_access` then
    /// blocks registration. A failed version query fails closed.
    pub async fn connect(&self) -> Result<PgSession, SourceError> {
        let mut permits = try_reserve(self.config.source_id().as_str(), 1)?;
        let Some(permit) = permits.pop() else {
            return Err(SourceError::budget("max_connections"));
        };
        self.connect_reserved(permit).await
    }

    /// Open one session on a permit reserved by the caller.
    /// The permit stays held until the socket closes.
    pub(crate) async fn connect_reserved(&self, permit: ConnPermit) -> Result<PgSession, SourceError> {
        let options = PgConnectOptions::from_explicit()
            .host(self.config.endpoint().host())
            .port(self.config.endpoint().port())
            .username(self.config.credentials().username())
            .password(self.config.credentials().password())
            .database(self.config.endpoint().database())
            .application_name(sql::APPLICATION_NAME)
            .extra_float_digits(2i8)
            .ssl_mode(PgSslMode::VerifyFull)
            .ssl_root_cert_from_pem(self.config.tls().ca_pem().to_vec());
        if options.get_options().is_some() {
            return Err(SourceError::config("startup_options"));
        }
        let options = options.disable_statement_logging();
        let connect_timeout = self.config.budget().connect_timeout();
        let connect = options.connect();
        let conn = match tokio::time::timeout(connect_timeout, connect).await {
            Ok(Ok(conn)) => conn,
            Ok(Err(err)) => return Err(map_connect(err)),
            Err(_) => return Err(SourceError::budget("connect_timeout")),
        };
        let mut session = PgSession {
            conn,
            permit: Some(permit),
            source_id: self.config.source_id().as_str().to_string(),
            secret_ref: self.config.secret_ref().as_str().to_string(),
            selection: self.config.selection().clone(),
            budget: self.config.budget(),
            version_num: 0,
            version_raw: String::new(),
            major: 0,
            minor: 0,
            admission: "unknown",
            poisoned: false,
            in_transaction: false,
            identity_aligned: false,
            startup_clean: false,
            negotiated_tls: String::new(),
            session_user: String::new(),
            current_user: String::new(),
            database_name: String::new(),
            row_bound: None,
            skip_rollback: false,
        };
        session.read_identity().await?;
        session.apply_guards().await?;
        session.read_version().await?;
        Ok(session)
    }
}

impl fmt::Debug for PgConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgConnector")
            .field("source_id", self.config.source_id())
            .field("secret_ref", self.config.secret_ref())
            .finish()
    }
}

/// A connected source. Dropping it closes the socket. There is no `COMMIT`.
pub struct PgSession {
    pub(crate) conn: PgConnection,
    permit: Option<ConnPermit>,
    pub(crate) source_id: String,
    pub(crate) secret_ref: String,
    pub(crate) selection: TableSelection,
    pub(crate) budget: crate::config::PgBudget,
    pub(crate) version_num: i64,
    pub(crate) version_raw: String,
    pub(crate) major: i64,
    pub(crate) minor: i64,
    pub(crate) admission: &'static str,
    pub(crate) poisoned: bool,
    pub(crate) in_transaction: bool,
    pub(crate) identity_aligned: bool,
    pub(crate) startup_clean: bool,
    pub(crate) negotiated_tls: String,
    pub(crate) session_user: String,
    pub(crate) current_user: String,
    pub(crate) database_name: String,
    row_bound: Option<u32>,
    skip_rollback: bool,
}

impl fmt::Debug for PgSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgSession")
            .field("source_id", &self.source_id)
            .field("secret_ref", &self.secret_ref)
            .field("admission", &self.admission)
            .field("connected", &true)
            .finish()
    }
}

impl PgSession {
    /// Read-only access inspection.
    pub async fn inspect_access(&mut self) -> Result<AccessReport, SourceError> {
        self.ensure_open()?;
        if !self.identity_aligned || !self.startup_clean {
            return Ok(self.session_block_report());
        }
        if self.admission != "tested" {
            return Ok(self.version_block_report());
        }
        self.begin().await?;
        let result = self.collect_access().await;
        self.finish(result).await
    }

    /// Authoritative snapshot of the selected tables.
    pub async fn catalog(&mut self) -> Result<crate::model::SourceCatalog, SourceError> {
        self.ensure_open()?;
        if !self.identity_aligned {
            return Err(SourceError::config("session_user_mismatch"));
        }
        if !self.startup_clean {
            return Err(SourceError::config("startup_options"));
        }
        if self.admission != "tested" {
            return Err(SourceError::version(match self.admission {
                "below_minimum" => "version_below_minimum",
                "untested" => "version_untested",
                _ => "version_unknown",
            }));
        }
        self.begin().await?;
        let result = self.collect_catalog().await;
        self.finish(result).await
    }

    pub(crate) fn ensure_open(&self) -> Result<(), SourceError> {
        if self.poisoned {
            Err(SourceError::connect("session_poisoned"))
        } else {
            Ok(())
        }
    }

    async fn read_identity(&mut self) -> Result<(), SourceError> {
        try_charge_rows(&self.source_id, 1)?;
        let allow = [
            "DateStyle",
            "client_encoding",
            "TimeZone",
            "extra_float_digits",
            "application_name",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let budget = self.budget.statement_timeout();
        let result = tokio::time::timeout(
            budget,
            sqlx::query(sql::IDENTITY)
                .bind(&allow)
                .fetch_one(&mut self.conn),
        )
        .await;
        let row = match result {
            Ok(Ok(row)) => row,
            Ok(Err(err)) => return Err(map_sqlx(err)),
            Err(_) => {
                self.poisoned = true;
                return Err(SourceError::budget("statement_timeout"));
            }
        };
        let session_user = cell_string(&row, 0)?;
        let current_user = cell_string(&row, 1)?;
        let ssl = cell_opt_bool(&row, 2)?;
        let version = cell_opt_string(&row, 3)?;
        let extra = cell_string(&row, 4)?;
        let database = cell_string(&row, 5)?;
        let negotiated = match classify_server_tls(ssl, version)? {
            ServerTls::Observed(version) => version,
            ServerTls::Unknown => "unknown".to_string(),
        };
        if session_user.is_empty()
            || current_user.is_empty()
            || session_user.len() > 256
            || current_user.len() > 256
            || database.is_empty()
        {
            return Err(SourceError::catalog("identity"));
        }
        self.identity_aligned = session_user == current_user;
        self.startup_clean = extra.is_empty();
        self.negotiated_tls = negotiated;
        self.session_user = session_user;
        self.current_user = current_user;
        self.database_name = database;
        Ok(())
    }

    pub(crate) fn tls(&self) -> crate::model::TlsReport {
        tls_report(&self.negotiated_tls)
    }

    async fn apply_guards(&mut self) -> Result<(), SourceError> {
        try_charge_rows(&self.source_id, 1)?;
        let statement_ms = duration_ms(self.budget.statement_timeout())?;
        let lock_ms = duration_ms(self.budget.lock_timeout())?;
        let budget = self.budget.statement_timeout();
        let result = tokio::time::timeout(
            budget,
            sqlx::query(sql::SESSION_GUARDS)
                .bind(statement_ms)
                .bind(lock_ms)
                .execute(&mut self.conn),
        )
        .await;
        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(map_sqlx(err)),
            Err(_) => {
                self.poisoned = true;
                Err(SourceError::budget("statement_timeout"))
            }
        }
    }

    async fn read_version(&mut self) -> Result<(), SourceError> {
        try_charge_rows(&self.source_id, 1)?;
        let budget = self.budget.statement_timeout();
        let result =
            tokio::time::timeout(budget, sqlx::query(sql::VERSION).fetch_one(&mut self.conn)).await;
        let row = match result {
            Ok(Ok(row)) => row,
            Ok(Err(err)) => return Err(map_sqlx(err)),
            Err(_) => {
                self.poisoned = true;
                return Err(SourceError::budget("statement_timeout"));
            }
        };
        let num: i64 = row
            .try_get(0)
            .map_err(|_| SourceError::version("version_unknown"))?;
        let raw: String = row
            .try_get(1)
            .map_err(|_| SourceError::version("version_unknown"))?;
        if num <= 0 || raw.is_empty() || raw.len() > 128 || raw.contains('\0') {
            return Err(SourceError::version("version_unknown"));
        }
        self.version_num = num;
        self.major = num / 10_000;
        self.minor = num % 10_000;
        self.version_raw = raw;
        self.admission = admit_version(self.major, self.minor, &self.version_raw);
        Ok(())
    }

    pub(crate) async fn begin(&mut self) -> Result<(), SourceError> {
        self.begin_tracked(sql::BEGIN).await
    }

    pub(crate) async fn begin_read_committed(&mut self) -> Result<(), SourceError> {
        self.begin_tracked(sql::BEGIN_CURRENT).await
    }

    /// Send the snapshot `START TRANSACTION` through the driver transaction
    /// manager so `transaction_depth` is 1. A named portal refuses depth 0.
    /// The SQL text stays the caller statement. This does not `COMMIT`.
    async fn begin_tracked(&mut self, sql: &'static str) -> Result<(), SourceError> {
        self.ensure_open()?;
        if self.in_transaction {
            return Err(SourceError::protocol("nested_transaction"));
        }
        try_charge_rows(&self.source_id, 1)?;
        let budget = self.budget.statement_timeout();
        let started = tokio::time::timeout(
            budget,
            PgTransactionManager::begin(&mut self.conn, Some(sql.into_sql_str())),
        )
        .await;
        match started {
            Ok(Ok(())) => {
                self.in_transaction = true;
                Ok(())
            }
            Ok(Err(err)) => Err(map_sqlx(err)),
            Err(_) => {
                self.poisoned = true;
                Err(SourceError::budget("statement_timeout"))
            }
        }
    }

    pub(crate) fn arm_rows(&mut self, bound: u32) {
        self.row_bound = Some(bound);
    }

    async fn execute_charged(&mut self, sql: &'static str) -> Result<(), SourceError> {
        try_charge_rows(&self.source_id, 1)?;
        let budget = self.budget.statement_timeout();
        let result = tokio::time::timeout(budget, sqlx::query(sql).execute(&mut self.conn)).await;
        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(map_sqlx(err)),
            Err(_) => {
                self.poisoned = true;
                Err(SourceError::budget("statement_timeout"))
            }
        }
    }

    pub(crate) async fn execute_control(&mut self, sql: &'static str) -> Result<(), SourceError> {
        self.ensure_open()?;
        self.execute_charged(sql).await
    }

    /// The socket is closed without `ROLLBACK`. Used when the portal or the
    /// receive path has already left the connection unable to take a command.
    pub(crate) fn mark_skip_rollback(&mut self) {
        self.skip_rollback = true;
        self.poisoned = true;
        self.in_transaction = false;
    }

    /// `ROLLBACK` will be skipped. The socket close is not a confirmed transaction end.
    pub(crate) fn rollback_skipped(&self) -> bool {
        self.skip_rollback
    }

    pub(crate) async fn shutdown(mut self) -> Result<(), SourceError> {
        if self.skip_rollback {
            self.in_transaction = false;
        }
        let rolled = self.finish(Ok(())).await.map(|_| ());
        let permit = self.permit.take();
        let PgSession { conn, .. } = self;
        let closed = Connection::close(conn).await.map_err(map_sqlx);
        drop(permit);
        match (rolled, closed) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(err), _) => Err(err),
            (Ok(()), Err(_)) => Err(SourceError::protocol("cleanup_failed")),
        }
    }

    pub(crate) async fn finish<T>(&mut self, result: Result<T, SourceError>) -> Result<T, SourceError> {
        let rolled = self.rollback().await;
        match (result, rolled) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(err), _) => Err(err),
            (Ok(_), Err(err)) => Err(err),
        }
    }

    async fn rollback(&mut self) -> Result<(), SourceError> {
        if !self.in_transaction {
            return Ok(());
        }
        self.in_transaction = false;
        match self.execute_charged(sql::ROLLBACK).await {
            Ok(()) => Ok(()),
            Err(err) => {
                self.poisoned = true;
                Err(err)
            }
        }
    }

    pub(crate) fn version_fact(&self) -> VersionFact {
        VersionFact {
            server_version_num: self.version_num,
            server_version: self.version_raw.clone(),
            major: self.major,
            minor: self.minor,
            declared_minimum_major: crate::DECLARED_MIN_MAJOR,
            tested_versions: crate::TESTED_VERSIONS
                .iter()
                .map(|item| (*item).to_string())
                .collect(),
            admission: self.admission.to_string(),
        }
    }

    fn version_block_report(&self) -> AccessReport {
        let code = match self.admission {
            "below_minimum" => "version_below_minimum",
            "untested" => "version_untested",
            _ => "version_unknown",
        };
        AccessReport {
            protocol: crate::PROTOCOL.to_string(),
            core_contract: crate::CORE_BOUNDARY.to_string(),
            source_id: self.source_id.clone(),
            secret_ref: self.secret_ref.clone(),
            tls: self.tls(),
            version: self.version_fact(),
            registration_allowed: false,
            block_reasons: vec![BlockReason::new(code, code)],
            principal: Fact::not_collected("not_collected_untested_version"),
            in_recovery: Fact::not_collected("not_collected_untested_version"),
            role: Fact::not_collected("not_collected_untested_version"),
            memberships: Vec::new(),
            write: WriteDetermination {
                state: "not_collected".to_string(),
                findings: Vec::new(),
            },
            security_definers: Vec::new(),
            rls: Vec::new(),
            publications: Fact::not_collected("not_collected_untested_version"),
            replication_slots: Fact::not_collected("not_collected_untested_version"),
            cdc: CdcPrerequisite {
                wal_level: Fact::not_collected("not_collected_untested_version"),
                replication_attribute: Fact::not_collected("not_collected_untested_version"),
                publication_for_selected: Fact::not_collected("not_collected_untested_version"),
                implemented: false,
            },
            capabilities: connector_capabilities(),
            dba_checklist: vec![DbaItem {
                id: "version".to_string(),
                level: "info".to_string(),
                risk:
                    "This server version is outside the tested set. No source object was created."
                        .to_string(),
                rollback: "No source change was made.".to_string(),
                statement: String::new(),
            }],
        }
    }

    fn session_block_report(&self) -> AccessReport {
        let mut blocks = Vec::new();
        if !self.identity_aligned {
            blocks.push(BlockReason::new("role_mismatch", "session_user"));
        }
        if !self.startup_clean {
            blocks.push(BlockReason::new("startup_option", "startup_options"));
        }
        AccessReport {
            protocol: crate::PROTOCOL.to_string(),
            core_contract: crate::CORE_BOUNDARY.to_string(),
            source_id: self.source_id.clone(),
            secret_ref: self.secret_ref.clone(),
            tls: self.tls(),
            version: self.version_fact(),
            registration_allowed: false,
            block_reasons: blocks,
            principal: crate::model::Fact::known(crate::model::Principal {
                current_user: self.current_user.clone(),
                session_user: self.session_user.clone(),
                database: self.database_name.clone(),
            }),
            in_recovery: crate::model::Fact::not_collected("not_collected_session_rejected"),
            role: crate::model::Fact::not_collected("not_collected_session_rejected"),
            memberships: Vec::new(),
            write: WriteDetermination {
                state: "unknown".to_string(),
                findings: Vec::new(),
            },
            security_definers: Vec::new(),
            rls: Vec::new(),
            publications: crate::model::Fact::not_collected("not_collected_session_rejected"),
            replication_slots: crate::model::Fact::not_collected("not_collected_session_rejected"),
            cdc: CdcPrerequisite {
                wal_level: crate::model::Fact::not_collected("not_collected_session_rejected"),
                replication_attribute: crate::model::Fact::not_collected(
                    "not_collected_session_rejected",
                ),
                publication_for_selected: crate::model::Fact::not_collected(
                    "not_collected_session_rejected",
                ),
                implemented: false,
            },
            capabilities: connector_capabilities(),
            dba_checklist: Vec::new(),
        }
    }
}

/// Server `pg_stat_ssl` fact for this backend.
#[derive(Debug)]
enum ServerTls {
    /// `ssl = true` and a `TLSv1.` version string.
    Observed(String),
    /// No row. Not plaintext, and not a negotiated version.
    Unknown,
}

/// A null `pg_stat_ssl` row is unknown. `ssl = false` is plaintext.
fn classify_server_tls(
    ssl: Option<bool>,
    version: Option<String>,
) -> Result<ServerTls, SourceError> {
    match ssl {
        Some(false) => Err(SourceError::tls("tls_not_negotiated")),
        None => {
            if version.is_some() {
                Err(SourceError::tls("tls_version_unknown"))
            } else {
                Ok(ServerTls::Unknown)
            }
        }
        Some(true) => {
            let Some(version) = version else {
                return Err(SourceError::tls("tls_version_unknown"));
            };
            if !version.starts_with("TLSv1.") || version.len() > 32 || version.contains('\0') {
                return Err(SourceError::tls("tls_version_unknown"));
            }
            Ok(ServerTls::Observed(version))
        }
    }
}

pub(crate) fn duration_ms(value: Duration) -> Result<String, SourceError> {
    let ms = value.as_millis();
    if ms == 0 || ms > 120_000 {
        return Err(SourceError::config("timeout"));
    }
    Ok(ms.to_string())
}

pub(crate) fn within_rows(len: usize, max_rows: u32) -> Result<(), SourceError> {
    if len > usize::try_from(max_rows).unwrap_or(usize::MAX) {
        Err(SourceError::budget("max_rows"))
    } else {
        Ok(())
    }
}

pub(crate) async fn fetch_all(
    session: &mut PgSession,
    query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
) -> Result<Vec<sqlx::postgres::PgRow>, SourceError> {
    let bound = session.row_bound.take().unwrap_or(UNARMED_FETCH_LIMIT);
    if bound == 0 || bound > MAX_ROWS_PER_SEC {
        return Err(SourceError::budget("max_rows_per_sec"));
    }
    let hold = try_reserve_rows(&session.source_id, bound)?;
    let budget = session.budget.statement_timeout();
    let result = tokio::time::timeout(budget, query.fetch_all(&mut session.conn)).await;
    match result {
        Ok(Ok(rows)) => {
            let observed = rows.len();
            if observed > usize::try_from(bound).unwrap_or(usize::MAX) {
                drop(hold);
                return Err(SourceError::budget("max_rows"));
            }
            hold.settle(observed);
            within_rows(observed, session.budget.max_rows())?;
            Ok(rows)
        }
        Ok(Err(err)) => Err(map_sqlx(err)),
        Err(_) => {
            session.poisoned = true;
            Err(SourceError::budget("statement_timeout"))
        }
    }
}

pub(crate) async fn fetch_one(
    session: &mut PgSession,
    query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
) -> Result<sqlx::postgres::PgRow, SourceError> {
    try_charge_rows(&session.source_id, 1)?;
    let budget = session.budget.statement_timeout();
    let result = tokio::time::timeout(budget, query.fetch_one(&mut session.conn)).await;
    match result {
        Ok(Ok(row)) => Ok(row),
        Ok(Err(err)) => Err(map_sqlx(err)),
        Err(_) => {
            session.poisoned = true;
            Err(SourceError::budget("statement_timeout"))
        }
    }
}

pub(crate) fn cell_string(
    row: &sqlx::postgres::PgRow,
    index: usize,
) -> Result<String, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_opt_string(
    row: &sqlx::postgres::PgRow,
    index: usize,
) -> Result<Option<String>, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_i64(row: &sqlx::postgres::PgRow, index: usize) -> Result<i64, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_opt_i64(
    row: &sqlx::postgres::PgRow,
    index: usize,
) -> Result<Option<i64>, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_i32(row: &sqlx::postgres::PgRow, index: usize) -> Result<i32, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_opt_i32(
    row: &sqlx::postgres::PgRow,
    index: usize,
) -> Result<Option<i32>, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_bool(row: &sqlx::postgres::PgRow, index: usize) -> Result<bool, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_opt_bool(
    row: &sqlx::postgres::PgRow,
    index: usize,
) -> Result<Option<bool>, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

#[cfg(test)]
mod tests {
    use super::{ServerTls, classify_server_tls};
    use crate::error::SourceErrorKind;

    #[test]
    fn missing_ssl_row_is_unknown() {
        assert!(matches!(
            classify_server_tls(None, None).unwrap(),
            ServerTls::Unknown
        ));
    }

    #[test]
    fn false_ssl_is_plaintext() {
        let err = classify_server_tls(Some(false), None).unwrap_err();
        assert_eq!(err.kind(), SourceErrorKind::Tls);
        assert_eq!(err.fact(), Some("tls_not_negotiated"));
    }

    #[test]
    fn true_ssl_keeps_only_a_tls_version() {
        let err = classify_server_tls(Some(true), None).unwrap_err();
        assert_eq!(err.fact(), Some("tls_version_unknown"));
        match classify_server_tls(Some(true), Some("TLSv1.3".to_string())).unwrap() {
            ServerTls::Observed(version) => assert_eq!(version, "TLSv1.3"),
            ServerTls::Unknown => panic!("missing version was accepted"),
        }
        let bad = classify_server_tls(Some(true), Some("ssl".to_string())).unwrap_err();
        assert_eq!(bad.fact(), Some("tls_version_unknown"));
    }

    #[test]
    fn version_without_ssl_is_not_success() {
        let err = classify_server_tls(None, Some("TLSv1.3".to_string())).unwrap_err();
        assert_eq!(err.fact(), Some("tls_version_unknown"));
    }

    #[test]
    fn ledger_cap_is_four_and_a_live_window_keeps_its_debit() {
        let _lock = super::lock_ledger();
        let first = super::try_reserve("a125_cap", 2).unwrap();
        let second = super::try_reserve("a125_cap", 2).unwrap();
        let fifth = match super::try_reserve("a125_cap", 1) {
            Err(err) => err,
            Ok(permits) => {
                drop(permits);
                panic!("fifth reserve succeeded");
            }
        };
        assert_eq!(fifth.fact(), Some("max_connections"));
        drop(second);
        let restored = super::try_reserve("a125_cap", 2).unwrap();
        drop(restored);
        drop(first);

        let held = super::try_reserve("a125_busy", 1).unwrap();
        super::try_charge_rows("a125_busy", 50_000).unwrap();
        super::age_ledger_window("a125_busy", std::time::Duration::from_secs(2));
        super::try_charge_rows("a125_busy", 1).unwrap();
        drop(held);

        super::try_charge_rows("a125_rows", 50_000).unwrap();
        let over = super::try_charge_rows("a125_rows", 1).unwrap_err();
        assert_eq!(over.fact(), Some("max_rows_per_sec"));
        super::try_charge_rows("a125_rows", 0).unwrap();
        let still = super::try_charge_rows("a125_rows", 1).unwrap_err();
        assert_eq!(still.fact(), Some("max_rows_per_sec"));
        for index in 0..61 {
            let permit = super::try_reserve(&format!("a125_pad_{index}"), 1).unwrap();
            drop(permit);
        }
        let blocked = match super::try_reserve("a125_extra", 1) {
            Err(err) => err,
            Ok(permits) => {
                drop(permits);
                panic!("extra reserve succeeded");
            }
        };
        assert_eq!(blocked.fact(), Some("max_connections"));
        let kept = super::try_charge_rows("a125_rows", 1).unwrap_err();
        assert_eq!(kept.fact(), Some("max_rows_per_sec"));
    }

    #[test]
    fn reservation_settles_the_unused_allowance() {
        let _lock = super::lock_ledger();
        super::try_reserve_rows("a128_settle", 100).unwrap().settle(25);
        assert_eq!(super::raw_rows_left("a128_settle"), Some(49_975));
        let over = match super::try_reserve_rows("a128_settle", 49_976) {
            Err(err) => err,
            Ok(hold) => {
                drop(hold);
                panic!("reserve above the remaining allowance succeeded");
            }
        };
        assert_eq!(over.fact(), Some("max_rows_per_sec"));
        super::try_reserve_rows("a128_settle", 49_975).unwrap().settle(49_975);
        assert_eq!(super::raw_rows_left("a128_settle"), Some(0));
        for index in 0..63 {
            let permit = super::try_reserve(&format!("a128_pad_{index}"), 1).unwrap();
            drop(permit);
        }
        let kept = match super::try_reserve_rows("a128_settle", 1) {
            Err(err) => err,
            Ok(hold) => {
                drop(hold);
                panic!("eviction reset an exhausted debit");
            }
        };
        assert_eq!(kept.fact(), Some("max_rows_per_sec"));
    }

    #[test]
    fn reservation_abandon_keeps_the_debit() {
        let _lock = super::lock_ledger();
        let hold = super::try_reserve_rows("a128_abandon", 80).unwrap();
        drop(hold);
        assert_eq!(super::raw_rows_left("a128_abandon"), Some(49_920));
        let over = match super::try_reserve_rows("a128_abandon", 49_921) {
            Err(err) => err,
            Ok(extra) => {
                drop(extra);
                panic!("reserve above the abandoned debit succeeded");
            }
        };
        assert_eq!(over.fact(), Some("max_rows_per_sec"));
        super::try_reserve_rows("a128_abandon", 49_920).unwrap().settle(0);
        assert_eq!(super::raw_rows_left("a128_abandon"), Some(49_920));
    }

    #[test]
    fn reservation_does_not_refund_after_the_window() {
        let _lock = super::lock_ledger();
        let hold = super::try_reserve_rows("a128_roll", 100).unwrap();
        super::age_ledger_window("a128_roll", std::time::Duration::from_secs(2));
        hold.settle(10);
        assert_eq!(super::raw_rows_left("a128_roll"), Some(49_900));
        assert_eq!(super::ASSUMABLE_LIMIT, 65);
        assert!(crate::sql::ASSUMABLE.contains("LIMIT 65"));
    }
}
