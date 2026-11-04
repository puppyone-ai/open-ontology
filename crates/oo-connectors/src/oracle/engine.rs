//! [`SourceEngine`] for Oracle 11g-23ai over the `oracledb` thin driver.
//!
//! Application-generated snapshot SQL consists of `SELECT` statements. The
//! pinned thin driver also initializes the session time zone; strict T7 audit
//! acceptance is still pending. The adapter does not run `EXPLAIN PLAN`
//! (which inserts into `PLAN_TABLE`), creates no `DBMS_PARALLEL_EXECUTE`
//! task and takes no lock. Index use of the keyset query is verified from
//! the shared cursor in `V$SQL_PLAN` when that view is readable.
//!
//! Source protection (docs/product/02 §3): one session per engine, a client
//! call timeout on every round trip, a duty cycle between chunk reads, and a
//! pause while other active user sessions in `V$SESSION` exceed a threshold.
//!
//! The driver is synchronous. Futures returned here run blocking driver
//! calls inline and await only between chunks (duty cycle, pause), so the
//! engine belongs on a blocking-capable worker, one task per session.

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arrow_array::{ArrayRef, RecordBatch, StringArray};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType, Field, Schema};
use oracledb::{Connection, ErrorKind, OracleNumber, ToDbValue};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::engine::{
    Capability, CapabilityState, ChunkReceipt, ChunkSink, ChunkSpec, ChunkStrategy,
    ENGINE_PROTOCOL, EngineFamily, EngineVersion, KeyShape, KeyValue, PhysicalRangeKind,
    ProbeReport, RangeDigest, ReadBudget, Requirement, SourceEngine, TableChunkPlan, TableRef,
    Watermark,
};
use crate::{SourceError, SourceErrorKind};

use super::keyset::{
    self, Bind, Bound, ColKind, Column, Extent, PageRange, Sql, boundary_probe_sql, digest_sql,
    page_sql,
};
use super::rules::{self, Dialect, Multitenant};
use super::worker::{DriverError, Worker};

#[path = "logminer.rs"]
pub mod logminer;

#[path = "c5.rs"]
pub mod c5;

#[cfg(test)]
#[path = "../../../../testing/engines/oracle/s2.rs"]
mod s2;

/// Connection settings. The password comes from the caller's secret
/// reference and is never logged, echoed or put on a command line.
pub struct OracleConnectConfig {
    pub host: String,
    pub port: u16,
    /// PDB (or non-CDB) service name.
    pub service: String,
    pub user: String,
    pub password: Zeroizing<String>,
    /// Tables whose grants and supplemental logging the probe checks.
    pub scope: Vec<TableRef>,
    /// Pause chunk reads while more than this many other user sessions are
    /// ACTIVE in `V$SESSION`. `None` disables the gate.
    pub active_session_limit: Option<u32>,
    pub pause_poll: Duration,
    /// Longest pause before a read fails with `source_busy`.
    pub pause_max: Duration,
    /// ORA-01555 retries per chunk, each with half the page size.
    pub snapshot_retries: u32,
}

impl OracleConnectConfig {
    pub fn new(
        host: &str,
        port: u16,
        service: &str,
        user: &str,
        password: Zeroizing<String>,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            service: service.into(),
            user: user.into(),
            password,
            scope: Vec::new(),
            active_session_limit: Some(16),
            pause_poll: Duration::from_millis(500),
            pause_max: Duration::from_secs(60),
            snapshot_retries: 4,
        }
    }
}

/// How the keyset query was checked against the key index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum IndexCheck {
    /// The shared cursor reads the key index in key order (no sort).
    Verified {
        index: String,
        operations: Vec<String>,
    },
    /// The cursor does not use the key index or sorts; reads stay correct
    /// but cost a scan per chunk.
    NotUsed {
        operations: Vec<String>,
    },
    Unavailable {
        reason: String,
    },
    NotApplicable,
}

/// Facts behind one table plan, for `oo doctor` and evidence.
#[derive(Clone, Debug, Serialize)]
pub struct PlanFacts {
    pub plan_revision: u32,
    pub key_index: Option<String>,
    pub boundary_method: &'static str,
    pub index_check: IndexCheck,
    /// SCN every physical (ROWID) chunk must be read at.
    pub snapshot_scn: Option<u64>,
    pub nls_comp: String,
    pub nls_sort: String,
    pub empty_string_is_null_columns: Vec<String>,
    pub chunk_rows: u64,
}

/// Resource counters for T8.
#[derive(Clone, Debug, Default, Serialize)]
pub struct EngineStats {
    pub sessions: u32,
    pub chunks: u64,
    pub rows: u64,
    pub ipc_bytes: u64,
    pub read_ms: u64,
    pub paced_sleep_ms: u64,
    pub busy_pauses: u64,
    pub busy_pause_ms: u64,
    pub snapshot_retries: u64,
    pub fresh_scn_retries: u64,
    pub peak_other_active_sessions: u32,
}

/// A chunk-local staging sink. Reset must discard all rows and receipt state,
/// not delete an already published range. Publish only after a successful read
/// and chunk-window correction (or C5 verification).
pub trait RetryableChunkSink: ChunkSink {
    fn reset(&mut self) -> Result<(), SourceError>;
}

#[derive(Clone, Debug)]
struct TableMeta {
    columns: Vec<Column>,
    key: KeyShape,
    /// Indexes into `columns`, in key order.
    key_cols: Vec<usize>,
    key_index: Option<String>,
    integer_key: bool,
    num_rows: Option<u64>,
    avg_row_len: Option<u64>,
}

impl TableMeta {
    fn keys(&self) -> Vec<&Column> {
        self.key_cols.iter().map(|i| &self.columns[*i]).collect()
    }
}

#[derive(Clone, Debug)]
struct TableState {
    meta: TableMeta,
    strategy: ChunkStrategy,
    facts: PlanFacts,
    next_chunk_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScnSource {
    VDatabase,
    DbmsFlashback,
}

/// One read-only Oracle session.
pub struct OracleEngine {
    conn: Option<Connection>,
    worker: Option<Worker>,
    cfg: OracleConnectConfig,
    session_user: String,
    version: Option<EngineVersion>,
    dialect: Option<Dialect>,
    scn_source: Option<ScnSource>,
    session_gate: Option<bool>,
    plan_view: Option<bool>,
    extents_view: Option<bool>,
    nls: Option<(String, String)>,
    tables: BTreeMap<TableRef, TableState>,
    next_read_at: Option<Instant>,
    stats: EngineStats,
    last_ora: Option<usize>,
}

type Rows = Vec<Vec<Option<String>>>;

/// Map a driver error to a closed error. Driver text never leaves here.
pub fn map_ora(e: &oracledb::Error) -> SourceError {
    map_error_kind(e.kind())
}

fn map_error_kind(error: &ErrorKind) -> SourceError {
    let (kind, fact) = match error {
        ErrorKind::DbError(db) => match db.code() {
            1555 => (SourceErrorKind::Budget, "ora_01555_snapshot_too_old"),
            1466 | 8180 | 8181 => (SourceErrorKind::Budget, "ora_snapshot_unavailable"),
            942 => (SourceErrorKind::Privilege, "ora_00942_not_readable"),
            1031 => (
                SourceErrorKind::Privilege,
                "ora_01031_insufficient_privileges",
            ),
            1017 | 28000 => (SourceErrorKind::Auth, "authentication_failed"),
            904 => (SourceErrorKind::Catalog, "ora_00904_invalid_identifier"),
            1013 => (SourceErrorKind::Budget, "ora_01013_call_cancelled"),
            1306 => (SourceErrorKind::Protocol, "ora_01306_logminer_not_started"),
            12505 | 12514 | 12541 | 12520 | 12519 => (SourceErrorKind::Connect, "listener"),
            _ => (SourceErrorKind::Protocol, "oracle_database_error"),
        },
        ErrorKind::CallTimeoutExceeded => (SourceErrorKind::Budget, "call_timeout"),
        ErrorKind::ServerVersionNotSupported => (
            SourceErrorKind::Version,
            "thin_driver_server_version_not_supported",
        ),
        ErrorKind::ListenerRefusedConnection(..)
        | ErrorKind::InvalidServiceName(..)
        | ErrorKind::InvalidSid(..) => (SourceErrorKind::Connect, "listener_refused"),
        ErrorKind::DeadConnection => (SourceErrorKind::Connect, "dead_connection"),
        ErrorKind::UnableToRecover => (SourceErrorKind::Connect, "connection_recovery_failed"),
        _ => (SourceErrorKind::Protocol, "oracle_driver_error"),
    };
    SourceError::new(kind, Some(fact), None)
}

fn ora_code(e: &DriverError) -> Option<usize> {
    match e {
        DriverError::Thin(e) => match e.kind() {
            ErrorKind::DbError(db) => Some(db.code()),
            _ => None,
        },
        DriverError::Worker { code, .. } => (*code != 0).then_some(*code),
        DriverError::Source(_) => None,
    }
}

fn map_driver(e: &DriverError) -> SourceError {
    match e {
        DriverError::Thin(e) => map_ora(e),
        DriverError::Source(e) => e.clone(),
        DriverError::Worker { code, timeout } => {
            if *timeout {
                return SourceError::budget("call_timeout");
            }
            let (kind, fact) = match code {
                1555 => (SourceErrorKind::Budget, "ora_01555_snapshot_too_old"),
                1466 | 8180 | 8181 => (SourceErrorKind::Budget, "ora_snapshot_unavailable"),
                942 | 1031 => (SourceErrorKind::Privilege, "oracle_not_readable"),
                904 => (SourceErrorKind::Catalog, "ora_00904_invalid_identifier"),
                1013 => (SourceErrorKind::Budget, "ora_01013_call_cancelled"),
                1017 | 28000 => (SourceErrorKind::Auth, "authentication_failed"),
                _ => (SourceErrorKind::Protocol, "oracle_worker_error"),
            };
            SourceError::new(kind, Some(fact), None)
        }
    }
}

/// Typed driver binds kept alive for one call.
enum BindValue {
    N(OracleNumber),
    S(String),
}

fn bind_values(binds: &[Bind]) -> Result<Vec<BindValue>, SourceError> {
    binds
        .iter()
        .map(|b| match b {
            Bind::Number(n) => n
                .parse::<OracleNumber>()
                .map(BindValue::N)
                .map_err(|_| SourceError::protocol("number_bind")),
            Bind::Text(s) => Ok(BindValue::S(s.clone())),
        })
        .collect()
}

fn positive_era(v: &str) -> Result<String, SourceError> {
    if v.starts_with('-') {
        return Err(SourceError::catalog("bc_era_unsupported"));
    }
    Ok(v.trim_start_matches([' ', '+']).to_owned())
}

/// Canonical cell text for Arrow, from the SQL projection.
fn canonical(kind: ColKind, v: String) -> Result<String, SourceError> {
    match kind {
        ColKind::Number => {
            super::chunk::decimal(&v).map_err(|_| SourceError::protocol("number_text"))
        }
        ColKind::Raw | ColKind::Blob => Ok(v.to_ascii_lowercase()),
        ColKind::Date | ColKind::Timestamp | ColKind::TimestampTz => positive_era(&v),
        ColKind::Char | ColKind::NChar | ColKind::Clob | ColKind::NClob => Ok(v),
    }
}

struct HashWriter {
    hasher: Sha256,
    bytes: u64,
}

impl Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.hasher.update(buf);
        self.bytes += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Arrow schema of a chunk: canonical UTF-8 text per column with the Oracle
/// type and `empty_string_is_null` semantics recorded as field metadata.
pub fn chunk_schema(table: &TableRef, columns: &[ColumnInfo]) -> Schema {
    let fields: Vec<Field> = columns
        .iter()
        .map(|c| {
            let mut md = std::collections::HashMap::new();
            md.insert("oo.oracle.type".to_owned(), c.data_type.clone());
            md.insert("oo.oracle.canonical".to_owned(), c.canonical.to_owned());
            if c.text {
                md.insert(
                    "oo.oracle.empty_string_is_null".to_owned(),
                    "true".to_owned(),
                );
            }
            Field::new(&c.name, DataType::Utf8, true).with_metadata(md)
        })
        .collect();
    let mut md = std::collections::HashMap::new();
    md.insert("oo.source".to_owned(), "oracle".to_owned());
    md.insert("oo.table".to_owned(), table.to_string());
    Schema::new_with_metadata(fields, md)
}

/// Column facts carried into the Arrow schema.
#[derive(Clone, Debug)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub canonical: &'static str,
    pub text: bool,
}

impl From<&Column> for ColumnInfo {
    fn from(c: &Column) -> Self {
        Self {
            name: c.name.clone(),
            data_type: c.data_type.clone(),
            canonical: c.kind.canonical(),
            text: c.kind.is_text(),
        }
    }
}

const BATCH_ROWS: usize = 4096;

fn validate_budget(budget: &ReadBudget) -> Result<(), SourceError> {
    if !(1..=100_000).contains(&budget.max_chunk_rows)
        || !(1..=32 * 1024 * 1024).contains(&budget.max_chunk_bytes)
        || !(1..=100).contains(&budget.duty_cycle_percent)
        || !(1..=3_600_000).contains(&budget.statement_timeout_ms)
    {
        return Err(SourceError::budget("invalid_read_budget"));
    }
    Ok(())
}

fn shrink_page(page: &mut u64, retries: &mut u32, limit: u32) -> bool {
    if *retries >= limit {
        return false;
    }
    *retries += 1;
    *page = (*page / 2).max(1);
    true
}

impl OracleEngine {
    /// Read DBID/resetlogs without starting LogMiner or changing source data.
    pub fn source_epoch(&mut self) -> Result<String, SourceError> {
        self.scalar(
            "SELECT TO_CHAR(dbid)||':'||TO_CHAR(resetlogs_change#) FROM v$database",
            &[],
        )?
        .filter(|epoch| !epoch.is_empty())
        .ok_or(SourceError::catalog("source_epoch"))
    }

    /// Open the one session this engine reads through.
    pub fn connect(cfg: OracleConnectConfig) -> Result<Self, SourceError> {
        Self::connect_driver(cfg, None)
    }

    /// Use a trusted, digest-pinned external P2 worker command. The worker
    /// receives only source credentials through stdin and returns Arrow IPC.
    pub fn connect_worker(
        cfg: OracleConnectConfig,
        command: &mut std::process::Command,
    ) -> Result<Self, SourceError> {
        Self::connect_driver(cfg, Some(command))
    }

    fn connect_driver(
        cfg: OracleConnectConfig,
        command: Option<&mut std::process::Command>,
    ) -> Result<Self, SourceError> {
        if cfg.host.is_empty()
            || cfg.port == 0
            || !cfg
                .service
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        {
            return Err(SourceError::config("oracle_endpoint"));
        }
        let (conn, worker) = if let Some(command) = command {
            (
                None,
                Some(Worker::connect(command, &cfg).map_err(|e| map_driver(&e))?),
            )
        } else {
            let config = oracledb::Config::default()
                .set_stmtcachesize(0)
                .set_credentials(&cfg.user, cfg.password.as_str())
                .set_connect_string(&format!("{}:{}/{}", cfg.host, cfg.port, cfg.service))
                .map_err(|e| map_ora(&e))?;
            let conn = oracledb::connect(config).map_err(|e| map_ora(&e))?;
            conn.set_module("oo-source-engine")
                .map_err(|e| map_ora(&e))?;
            (Some(conn), None)
        };
        let mut engine = Self {
            conn,
            worker,
            cfg,
            session_user: String::new(),
            version: None,
            dialect: None,
            scn_source: None,
            session_gate: None,
            plan_view: None,
            extents_view: None,
            nls: None,
            tables: BTreeMap::new(),
            next_read_at: None,
            stats: EngineStats {
                sessions: 1,
                ..EngineStats::default()
            },
            last_ora: None,
        };
        engine.set_timeout(Duration::from_secs(10))?;
        engine.session_user = engine
            .scalar(
                "SELECT SYS_CONTEXT('USERENV','SESSION_USER') FROM dual",
                &[],
            )?
            .ok_or(SourceError::catalog("session_user"))?;
        keyset::ident(&engine.session_user)?;
        for table in &engine.cfg.scope {
            keyset::ident(&table.schema)?;
            keyset::ident(&table.name)?;
        }
        Ok(engine)
    }

    pub fn stats(&self) -> &EngineStats {
        &self.stats
    }

    pub fn plan_facts(&self, table: &TableRef) -> Option<&PlanFacts> {
        self.tables.get(table).map(|s| &s.facts)
    }

    /// ORA code of the last database error, for evidence only.
    pub fn last_ora_code(&self) -> Option<usize> {
        self.last_ora
    }

    /// Restart a keyed chunk at a fresh SCN when bounded same-SCN retries
    /// exhaust its undo. Never resume after the old cursor: key moves could
    /// cross it. The caller still corrects the receipt's complete SCN window,
    /// or rechecks this range with C5, before publishing the staging sink.
    ///
    /// `SourceEngine::read_chunk` remains streaming/fail-closed for sinks that
    /// cannot discard a partial chunk. Physical ranges require a table replan.
    pub async fn read_chunk_recovering(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut impl RetryableChunkSink,
    ) -> Result<ChunkReceipt, SourceError> {
        validate_budget(budget)?;
        let keyed = self
            .tables
            .get(&chunk.table)
            .is_some_and(|state| matches!(state.strategy, ChunkStrategy::Keyset { .. }));
        let mut retries = 0;
        loop {
            match self.read_chunk(chunk, budget, sink).await {
                Ok(receipt) => return Ok(receipt),
                Err(error) => {
                    sink.reset()?;
                    if !keyed
                        || error.fact() != Some("ora_01555_snapshot_too_old")
                        || retries >= self.cfg.snapshot_retries
                    {
                        return Err(error);
                    }
                    retries += 1;
                    self.stats.fresh_scn_retries += 1;
                }
            }
        }
    }

    fn set_timeout(&self, d: Duration) -> Result<(), SourceError> {
        if let Some(worker) = &self.worker {
            worker.set_timeout(d);
            return Ok(());
        }
        self.conn
            .as_ref()
            .unwrap()
            .set_call_timeout(Some(d))
            .map_err(|e| map_ora(&e))
    }

    /// Run a query and return up to `max_rows` rows of nullable text.
    fn query(&mut self, text: &str, binds: &[Bind], max_rows: usize) -> Result<Rows, SourceError> {
        let rows = self.query_raw(text, binds, max_rows).map_err(|e| {
            self.last_ora = ora_code(&e);
            map_driver(&e)
        })?;
        if rows.len() > max_rows {
            return Err(SourceError::budget("query_row_limit"));
        }
        Ok(rows)
    }

    fn query_raw(&self, text: &str, binds: &[Bind], max_rows: usize) -> Result<Rows, DriverError> {
        if let Some(worker) = &self.worker {
            return worker.query(text, binds, max_rows + 1)?.collect();
        }
        self.query_thin(text, binds, max_rows, false)
            .map_err(Into::into)
    }

    fn sample_statement(&mut self, text: &str, binds: &[Bind]) -> Result<(), SourceError> {
        let result = if let Some(worker) = &self.worker {
            worker.probe(text, binds)
        } else {
            self.query_thin(text, binds, 1, true)
                .map(|_| ())
                .map_err(Into::into)
        };
        result.map_err(|e| {
            self.last_ora = ora_code(&e);
            map_driver(&e)
        })
    }

    fn query_thin(
        &self,
        text: &str,
        binds: &[Bind],
        max_rows: usize,
        discard: bool,
    ) -> Result<Rows, oracledb::Error> {
        // Callers construct numeric binds from validated metadata or keys.
        let values = binds
            .iter()
            .map(|b| match b {
                Bind::Number(n) => n.parse::<OracleNumber>().map(BindValue::N),
                Bind::Text(s) => Ok(BindValue::S(s.clone())),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let refs: Vec<&dyn ToDbValue> = values
            .iter()
            .map(|v| match v {
                BindValue::N(n) => n as &dyn ToDbValue,
                BindValue::S(s) => s as &dyn ToDbValue,
            })
            .collect();
        if discard {
            // Inspect the actual query plan without materializing LOB contents.
            let statement = self
                .conn
                .as_ref()
                .unwrap()
                .statement(text)?
                .fetch_array_size(1)
                .prefetch_rows(0)
                .fetch_lobs()
                .build()?;
            for row in statement.query(&refs)?.take(1) {
                row?;
            }
            return Ok(Vec::new());
        }
        let cursor = self.conn.as_ref().unwrap().query(text, &refs)?;
        let width = cursor.columns().len();
        let mut out = Vec::new();
        for row in cursor {
            let row = row?;
            let mut cells = Vec::with_capacity(width);
            for i in 0..width {
                cells.push(row.get::<Option<String>>(i)?);
            }
            out.push(cells);
            if out.len() > max_rows {
                break;
            }
        }
        Ok(out)
    }

    fn scalar(&mut self, text: &str, binds: &[Bind]) -> Result<Option<String>, SourceError> {
        Ok(self
            .query(text, binds, 1)?
            .into_iter()
            .next()
            .and_then(|r| r.into_iter().next().flatten()))
    }

    /// `Ok(true)` when the view or table answers a zero-row probe.
    fn readable(&mut self, object: &str) -> Result<bool, SourceError> {
        match self.query_raw(&format!("SELECT 1 FROM {object} WHERE ROWNUM < 1"), &[], 1) {
            Ok(_) => Ok(true),
            Err(e) => match ora_code(&e) {
                Some(942 | 1031 | 904) => Ok(false),
                // V$LOGMNR_CONTENTS outside a LogMiner session: visible.
                Some(1306) => Ok(true),
                _ => Err(map_driver(&e)),
            },
        }
    }

    fn ensure_version(&mut self) -> Result<Dialect, SourceError> {
        if let Some(d) = self.dialect {
            return Ok(d);
        }
        let full = self.query_raw(
            "SELECT version_full FROM product_component_version WHERE product LIKE 'Oracle%Database%' AND ROWNUM <= 1",
            &[],
            1,
        );
        let raw = match full {
            Ok(rows) => rows.into_iter().next().and_then(|r| r[0].clone()),
            Err(e) if ora_code(&e) == Some(904) => self.scalar(
                "SELECT version FROM product_component_version WHERE product LIKE 'Oracle%Database%' AND ROWNUM <= 1",
                &[],
            )?,
            Err(e) => return Err(map_driver(&e)),
        }
        .ok_or(SourceError::version("oracle_version_absent"))?;
        let v = EngineVersion::parse(EngineFamily::Oracle, &raw)?;
        if v.major < 11 {
            return Err(SourceError::version("oracle_below_11g"));
        }
        let d = Dialect::for_version(&v);
        self.version = Some(v);
        self.dialect = Some(d);
        Ok(d)
    }

    fn ensure_nls(&mut self) -> Result<(String, String), SourceError> {
        if let Some(n) = &self.nls {
            return Ok(n.clone());
        }
        let rows = self.query(
            "SELECT parameter, value FROM nls_session_parameters WHERE parameter IN ('NLS_COMP','NLS_SORT')",
            &[],
            4,
        )?;
        let get = |p: &str| {
            rows.iter()
                .find(|r| r[0].as_deref() == Some(p))
                .and_then(|r| r[1].clone())
                .unwrap_or_else(|| "BINARY".into())
        };
        let n = (get("NLS_COMP"), get("NLS_SORT"));
        self.nls = Some(n.clone());
        Ok(n)
    }

    /// Current SCN, from `V$DATABASE` or `DBMS_FLASHBACK`.
    pub fn current_scn(&mut self) -> Result<u64, SourceError> {
        let order = match self.scn_source {
            Some(s) => vec![s],
            None => vec![ScnSource::VDatabase, ScnSource::DbmsFlashback],
        };
        let mut last = SourceError::privilege("scn_unreadable");
        for source in order {
            let sql = match source {
                ScnSource::VDatabase => "SELECT TO_CHAR(current_scn) FROM v$database",
                ScnSource::DbmsFlashback => {
                    "SELECT TO_CHAR(DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER) FROM dual"
                }
            };
            match self.scalar(sql, &[]) {
                Ok(Some(v)) => {
                    self.scn_source = Some(source);
                    return v.parse().map_err(|_| SourceError::protocol("scn_text"));
                }
                Ok(None) => last = SourceError::protocol("scn_absent"),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn load_meta(&mut self, t: &TableRef) -> Result<TableMeta, SourceError> {
        let d = self.ensure_version()?;
        keyset::ident(&t.schema)?;
        keyset::ident(&t.name)?;
        let owner = Bind::Text(t.schema.clone());
        let name = Bind::Text(t.name.clone());
        let info = self.query(
            "SELECT TO_CHAR(num_rows), TO_CHAR(avg_row_len), NVL(iot_type,'-'), temporary FROM all_tables WHERE owner = :b1 AND table_name = :b2",
            &[owner.clone(), name.clone()],
            2,
        )?;
        let info = info
            .into_iter()
            .next()
            .ok_or(SourceError::catalog("table_not_visible"))?;
        if info[3].as_deref() == Some("Y") {
            return Err(SourceError::catalog("temporary_table"));
        }
        let iot = info[2].as_deref() != Some("-");
        let collation = if d.column_collation {
            "collation"
        } else {
            "NULL"
        };
        let rows = self.query(
            &format!(
                "SELECT column_name, data_type, nullable, TO_CHAR(data_length), TO_CHAR(data_scale), {collation}, virtual_column FROM all_tab_cols WHERE owner = :b1 AND table_name = :b2 AND hidden_column = 'NO' ORDER BY column_id"
            ),
            &[owner.clone(), name.clone()],
            1001,
        )?;
        if rows.is_empty() || rows.len() > 1000 {
            return Err(SourceError::budget("column_count"));
        }
        let mut columns = Vec::new();
        let mut scales = Vec::new();
        for r in rows {
            if r[6].as_deref() == Some("YES") {
                return Err(SourceError::catalog("virtual_column_requires_policy"));
            }
            let data_type = r[1].clone().unwrap_or_default();
            let kind = ColKind::from_oracle(&data_type)
                .ok_or(SourceError::catalog("unsupported_column_type"))?;
            columns.push(Column {
                name: r[0].clone().ok_or(SourceError::catalog("column_name"))?,
                data_type,
                kind,
                nullable: r[2].as_deref() != Some("N"),
                data_length: r[3].as_deref().and_then(|v| v.parse().ok()).unwrap_or(4000),
                collation: r[5].clone(),
            });
            scales.push(r[4].clone());
        }
        // Primary key, else the narrowest enabled unique index on NOT NULL
        // columns. Reverse-key and function-based indexes cannot range scan.
        let pk = self.query(
            "SELECT cc.column_name, c.index_name FROM all_constraints c JOIN all_cons_columns cc ON cc.owner = c.owner AND cc.constraint_name = c.constraint_name AND cc.table_name = c.table_name WHERE c.owner = :b1 AND c.table_name = :b2 AND c.constraint_type = 'P' AND c.status = 'ENABLED' AND c.validated = 'VALIDATED' AND c.deferrable = 'NOT DEFERRABLE' ORDER BY cc.position",
            &[owner.clone(), name.clone()],
            64,
        )?;
        let position = |columns: &[Column], n: &Option<String>| {
            columns
                .iter()
                .position(|c| Some(&c.name) == n.as_ref())
                .ok_or(SourceError::catalog("key_column_not_selectable"))
        };
        let (key_cols, key_index, unique_only) = if !pk.is_empty() {
            let cols = pk
                .iter()
                .map(|r| position(&columns, &r[0]))
                .collect::<Result<Vec<_>, _>>()?;
            (cols, pk[0][1].clone(), false)
        } else {
            let ux = self.query(
                "SELECT i.index_name, ic.column_name FROM all_indexes i JOIN all_ind_columns ic ON ic.index_owner = i.owner AND ic.index_name = i.index_name WHERE i.table_owner = :b1 AND i.table_name = :b2 AND i.uniqueness = 'UNIQUE' AND i.index_type = 'NORMAL' AND i.status IN ('VALID','N/A') ORDER BY i.index_name, ic.column_position",
                &[owner.clone(), name.clone()],
                512,
            )?;
            let mut by_index: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
            for r in ux {
                by_index
                    .entry(r[0].clone().unwrap_or_default())
                    .or_default()
                    .push(r[1].clone());
            }
            let mut best: Option<(String, Vec<usize>)> = None;
            for (index, names) in by_index {
                let Ok(cols) = names
                    .iter()
                    .map(|n| position(&columns, n))
                    .collect::<Result<Vec<_>, _>>()
                else {
                    continue;
                };
                if cols.iter().any(|i| columns[*i].nullable) {
                    continue;
                }
                if best.as_ref().is_none_or(|(_, b)| cols.len() < b.len()) {
                    best = Some((index, cols));
                }
            }
            match best {
                Some((index, cols)) => (cols, Some(index), true),
                None => (Vec::new(), None, false),
            }
        };
        if key_cols.iter().any(|i| !columns[*i].kind.keyable()) {
            return Err(SourceError::catalog("unsupported_key_type"));
        }
        if key_cols.is_empty() && iot {
            return Err(SourceError::catalog("iot_without_key"));
        }
        let integer_key = key_cols.len() == 1
            && columns[key_cols[0]].kind == ColKind::Number
            && scales[key_cols[0]].as_deref() == Some("0");
        let names: Vec<String> = key_cols.iter().map(|i| columns[*i].name.clone()).collect();
        let key = match (key_cols.len(), unique_only) {
            (0, _) => KeyShape::NoKey,
            (_, true) => KeyShape::UniqueIndex {
                index: key_index.clone().unwrap_or_default(),
                columns: names,
            },
            (1, false) if columns[key_cols[0]].kind == ColKind::Number => {
                KeyShape::IntegerPrimaryKey {
                    column: names[0].clone(),
                }
            }
            (1, false) => KeyShape::StringPrimaryKey {
                column: names[0].clone(),
                collation: columns[key_cols[0]].collation.clone(),
            },
            _ => KeyShape::CompositePrimaryKey { columns: names },
        };
        Ok(TableMeta {
            columns,
            key,
            key_cols,
            key_index,
            integer_key,
            num_rows: info[0].as_deref().and_then(|v| v.parse().ok()),
            avg_row_len: info[1].as_deref().and_then(|v| v.parse().ok()),
        })
    }

    /// Keyset bounds are compared by Oracle. That is only safe when the
    /// session comparison (`NLS_COMP`) and ordering (`NLS_SORT`) agree with
    /// the key index for every text key that has no data-bound collation.
    fn check_key_collation(&mut self, meta: &TableMeta) -> Result<(String, String), SourceError> {
        let (comp, sort) = self.ensure_nls()?;
        let session_bound = meta.keys().iter().any(|c| {
            c.kind.is_text()
                && c.collation
                    .as_deref()
                    .is_none_or(|v| v.starts_with("USING_NLS"))
        });
        if session_bound && !(comp == "BINARY" && sort == "BINARY") {
            return Err(SourceError::catalog(
                "key_collation_session_not_index_order",
            ));
        }
        Ok((comp, sort))
    }

    fn chunk_rows(meta: &TableMeta, budget: &ReadBudget) -> u64 {
        let text_bytes: u64 = meta
            .columns
            .iter()
            .map(|c| u64::from(c.kind.max_text(c.data_length)) / 2 + 1)
            .sum();
        let row_bytes = meta
            .avg_row_len
            .map(|v| v.saturating_mul(2).max(16))
            .unwrap_or(text_bytes)
            .max(1);
        (budget.max_chunk_bytes / row_bytes)
            .min(budget.max_chunk_rows)
            .max(1)
    }

    fn row_key(meta: &TableMeta, row: &[Option<String>]) -> Result<Vec<KeyValue>, SourceError> {
        meta.key_cols
            .iter()
            .map(|i| {
                let text = row[*i]
                    .as_deref()
                    .ok_or(SourceError::protocol("null_key"))?;
                keyset::key_value(meta.columns[*i].kind, text)
            })
            .collect()
    }

    /// Keyset boundaries from Oracle's own key order: each probe is one
    /// index range scan of `step` entries. No snapshot is needed because any
    /// existing key is a valid boundary.
    fn probe_boundaries(
        &mut self,
        t: &TableRef,
        meta: &TableMeta,
        lower: Option<Vec<KeyValue>>,
        upper: Option<&[KeyValue]>,
        step: u64,
    ) -> Result<Vec<Vec<KeyValue>>, SourceError> {
        let d = self.ensure_version()?;
        let keys = meta.keys();
        let mut out = Vec::new();
        let mut lo = lower;
        loop {
            if out.len() > 100_000 {
                return Err(SourceError::budget("chunk_count"));
            }
            let sql = boundary_probe_sql(
                &d,
                &t.schema,
                &t.name,
                &keys,
                lo.as_deref().map(|values| Bound {
                    values,
                    inclusive: true,
                }),
                upper.map(|values| Bound {
                    values,
                    inclusive: false,
                }),
                step,
            )?;
            let rows = self.query(&sql.text, &sql.binds, 1)?;
            let Some(row) = rows.into_iter().next() else {
                break;
            };
            let key = keys
                .iter()
                .zip(row)
                .map(|(c, v)| {
                    keyset::key_value(
                        c.kind,
                        v.as_deref().ok_or(SourceError::protocol("null_key"))?,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            out.push(key.clone());
            lo = Some(key);
        }
        Ok(out)
    }

    /// Uniform integer boundaries when `(max-min+1)/rows` is in
    /// `[0.05, 100]` (docs/product/02 §2), else `None`.
    fn integer_boundaries(
        &mut self,
        t: &TableRef,
        meta: &TableMeta,
        chunk_rows: u64,
    ) -> Result<Option<(Vec<Vec<KeyValue>>, u64)>, SourceError> {
        let col = keyset::ident(&meta.keys()[0].name)?;
        let row = self.query(
            &format!(
                "SELECT TO_CHAR(MIN({col})), TO_CHAR(MAX({col})), TO_CHAR(COUNT(*)) FROM {}",
                keyset::table_name(&t.schema, &t.name)?
            ),
            &[],
            1,
        )?;
        let row = row
            .into_iter()
            .next()
            .ok_or(SourceError::protocol("minmax"))?;
        let count: u64 = row[2].as_deref().and_then(|v| v.parse().ok()).unwrap_or(0);
        let (Some(min), Some(max)) = (
            row[0].as_deref().and_then(|v| v.parse::<i128>().ok()),
            row[1].as_deref().and_then(|v| v.parse::<i128>().ok()),
        ) else {
            return Ok(if count == 0 {
                Some((Vec::new(), 0))
            } else {
                None
            });
        };
        let Some(span) = max.checked_sub(min).and_then(|v| v.checked_add(1)) else {
            return Ok(None);
        };
        let factor = span as f64 / count.max(1) as f64;
        if !(0.05..=100.0).contains(&factor) {
            return Ok(None);
        }
        let Some(product) = span.checked_mul(i128::from(chunk_rows)) else {
            return Ok(None);
        };
        let step = (product / i128::from(count.max(1))).max(1);
        let mut out = Vec::new();
        let mut b = min + step;
        while b <= max {
            out.push(vec![KeyValue::Int(b)]);
            let Some(next) = b.checked_add(step) else {
                break;
            };
            b = next;
            if out.len() > 100_000 {
                return Err(SourceError::budget("chunk_count"));
            }
        }
        Ok(Some((out, count)))
    }

    /// Execute the middle-chunk keyset query once (one row) and read its
    /// plan from `V$SQL_PLAN`. No `EXPLAIN PLAN`: that writes `PLAN_TABLE`.
    fn verify_index(
        &mut self,
        t: &TableRef,
        meta: &TableMeta,
        boundaries: &[Vec<KeyValue>],
    ) -> Result<IndexCheck, SourceError> {
        if self.plan_view.is_none() {
            let v = self.readable("v$sql_plan")? && self.readable("v$session")?;
            self.plan_view = Some(v);
        }
        if self.plan_view != Some(true) {
            return Ok(IndexCheck::Unavailable {
                reason: "V$SQL_PLAN or V$SESSION not readable".into(),
            });
        }
        let d = self.ensure_version()?;
        let scn = self.current_scn()?;
        let keys = meta.keys();
        let (lo, hi) = match boundaries {
            [] => (None, None),
            [only] => (Some(only.as_slice()), None),
            [a, b, ..] => (Some(a.as_slice()), Some(b.as_slice())),
        };
        let mut sql = page_sql(
            &d,
            &t.schema,
            &t.name,
            &meta.columns,
            PageRange::Keyset {
                keys: &keys,
                lower: lo.map(|values| Bound {
                    values,
                    inclusive: true,
                }),
                upper: hi.map(|values| Bound {
                    values,
                    inclusive: false,
                }),
            },
            scn,
            1,
        )?;
        let mut retries = 0;
        loop {
            let sample = if meta.columns.iter().any(|c| c.kind.is_lob()) {
                self.sample_statement(&sql.text, &sql.binds)
            } else {
                self.query(&sql.text, &sql.binds, 1).map(|_| ())
            };
            match sample {
                Ok(_) => break,
                Err(error)
                    if error.fact() == Some("ora_snapshot_unavailable")
                        && retries < self.cfg.snapshot_retries =>
                {
                    retries += 1;
                    self.stats.fresh_scn_retries += 1;
                    std::thread::sleep(Duration::from_secs(1));
                    sql.binds[0] = Bind::Number(self.current_scn()?.to_string());
                }
                Err(error) => return Err(error),
            }
        }
        let cursor = self.query(
            "SELECT prev_sql_id, TO_CHAR(prev_child_number) FROM v$session WHERE sid = TO_NUMBER(SYS_CONTEXT('USERENV','SID'))",
            &[],
            1,
        )?;
        let Some([Some(sql_id), Some(child)]) = cursor
            .into_iter()
            .next()
            .map(|r| [r[0].clone(), r[1].clone()])
        else {
            return Ok(IndexCheck::Unavailable {
                reason: "previous cursor not visible".into(),
            });
        };
        let plan = self.query(
            "SELECT operation, NVL(options,'-'), NVL(object_name,'-') FROM v$sql_plan WHERE sql_id = :b1 AND child_number = TO_NUMBER(:b2) ORDER BY id",
            &[Bind::Text(sql_id), Bind::Text(child)],
            256,
        )?;
        let ops: Vec<String> = plan
            .iter()
            .map(|r| {
                format!(
                    "{} {} {}",
                    r[0].as_deref().unwrap_or("-"),
                    r[1].as_deref().unwrap_or("-"),
                    r[2].as_deref().unwrap_or("-")
                )
            })
            .collect();
        if ops.is_empty() {
            return Ok(IndexCheck::Unavailable {
                reason: "cursor plan aged out".into(),
            });
        }
        let index = meta.key_index.clone().unwrap_or_default();
        let uses_index = plan
            .iter()
            .any(|r| r[0].as_deref() == Some("INDEX") && r[2].as_deref() == Some(index.as_str()));
        let sorts = plan.iter().any(|r| {
            let (op, opt) = (r[0].as_deref().unwrap_or(""), r[1].as_deref().unwrap_or(""));
            (op == "SORT" && opt.contains("ORDER BY"))
                || (op == "WINDOW" && !opt.starts_with("NOSORT"))
        });
        Ok(if uses_index && !sorts {
            IndexCheck::Verified {
                index,
                operations: ops,
            }
        } else {
            IndexCheck::NotUsed { operations: ops }
        })
    }

    /// ROWID ranges from `DBA_EXTENTS` + `DBMS_ROWID.ROWID_CREATE`, valid
    /// only at `scn`. `ALL_EXTENTS` does not exist and `USER_EXTENTS` has
    /// no block addresses, so a readable `DBA_EXTENTS` is required.
    fn rowid_boundaries(
        &mut self,
        t: &TableRef,
        meta: &TableMeta,
        chunk_rows: u64,
    ) -> Result<Vec<String>, SourceError> {
        let rows = self.query(
            "SELECT TO_CHAR(o.data_object_id), TO_CHAR(e.relative_fno), TO_CHAR(e.block_id), TO_CHAR(e.blocks) FROM dba_extents e JOIN all_objects o ON o.owner = e.owner AND o.object_name = e.segment_name AND NVL(o.subobject_name,'-') = NVL(e.partition_name,'-') AND o.object_type IN ('TABLE','TABLE PARTITION','TABLE SUBPARTITION') WHERE e.owner = :b1 AND e.segment_name = :b2 AND e.segment_type IN ('TABLE','TABLE PARTITION','TABLE SUBPARTITION') ORDER BY o.data_object_id, e.relative_fno, e.block_id",
            &[Bind::Text(t.schema.clone()), Bind::Text(t.name.clone())],
            1_000_000,
        )?;
        let num = |v: &Option<String>| -> Result<u64, SourceError> {
            v.as_deref()
                .and_then(|s| s.parse().ok())
                .ok_or(SourceError::protocol("extent_number"))
        };
        let extents = rows
            .iter()
            .map(|r| {
                Ok(Extent {
                    data_object_id: num(&r[0])?,
                    relative_fno: num(&r[1])? as u32,
                    block_id: num(&r[2])?,
                    blocks: num(&r[3])?,
                })
            })
            .collect::<Result<Vec<_>, SourceError>>()?;
        let total_blocks: u64 = extents.iter().map(|e| e.blocks).sum();
        let rows_per_block = match (meta.num_rows, total_blocks) {
            (Some(n), b) if b > 0 && n > 0 => n.div_ceil(b).max(1),
            _ => (8192 / meta.avg_row_len.unwrap_or(100).max(1)).max(1),
        };
        // Allocated-but-empty blocks must not make populated extents too large.
        let rows_per_block = rows_per_block.max(8192 / meta.avg_row_len.unwrap_or(1).max(1));
        let blocks_per_chunk = (chunk_rows / rows_per_block.max(1)).max(1);
        let extents = keyset::split_extents(&extents, blocks_per_chunk);
        let starts = keyset::group_extents(&extents, blocks_per_chunk);
        let mut bounds = Vec::new();
        for s in starts.into_iter().skip(1) {
            let e = &extents[s];
            let (fno, block) = keyset::rowid_parts(e);
            let rid = self
                .scalar(
                    "SELECT ROWIDTOCHAR(DBMS_ROWID.ROWID_CREATE(1, TO_NUMBER(:b1), TO_NUMBER(:b2), TO_NUMBER(:b3), 0)) FROM dual",
                    &[
                        Bind::Text(e.data_object_id.to_string()),
                        Bind::Text(fno.to_string()),
                        Bind::Text(block.to_string()),
                    ],
                )?
                .ok_or(SourceError::protocol("rowid_create"))?;
            bounds.push(rid);
        }
        Ok(bounds)
    }

    /// Wait out the duty cycle and the active-session gate.
    async fn pace(&mut self) {
        if let Some(at) = self.next_read_at.take() {
            let now = Instant::now();
            if at > now {
                let wait = at - now;
                self.stats.paced_sleep_ms += wait.as_millis() as u64;
                tokio::time::sleep(wait).await;
            }
        }
    }

    async fn protect(&mut self) -> Result<(), SourceError> {
        self.pace().await;
        let Some(limit) = self.cfg.active_session_limit else {
            return Ok(());
        };
        if self.session_gate.is_none() {
            let v = self.readable("v$session")?;
            self.session_gate = Some(v);
        }
        if self.session_gate != Some(true) {
            return Ok(());
        }
        let started = Instant::now();
        loop {
            let n: u32 = self
                .scalar(
                    "SELECT TO_CHAR(COUNT(*)) FROM v$session WHERE status = 'ACTIVE' AND type = 'USER' AND sid <> TO_NUMBER(SYS_CONTEXT('USERENV','SID'))",
                    &[],
                )?
                .and_then(|v| v.parse().ok())
                .ok_or(SourceError::protocol("active_session_count"))?;
            self.stats.peak_other_active_sessions = self.stats.peak_other_active_sessions.max(n);
            if n <= limit {
                return Ok(());
            }
            if started.elapsed() >= self.cfg.pause_max {
                return Err(SourceError::budget("source_busy"));
            }
            self.stats.busy_pauses += 1;
            self.stats.busy_pause_ms += self.cfg.pause_poll.as_millis() as u64;
            tokio::time::sleep(self.cfg.pause_poll).await;
        }
    }

    fn schedule_next(&mut self, read: Duration, budget: &ReadBudget) {
        let duty = u32::from(budget.duty_cycle_percent.clamp(1, 100));
        let idle = read * (100 - duty) / duty;
        self.next_read_at = Some(Instant::now() + idle);
        self.stats.read_ms += read.as_millis() as u64;
    }

    /// Stream one page into Arrow; returns rows read and the last row key.
    #[allow(clippy::too_many_arguments)]
    fn run_page(
        &mut self,
        sql: &Sql,
        meta: &TableMeta,
        schema: &Arc<Schema>,
        sink: &mut dyn ChunkSink,
        ipc: &mut StreamWriter<&mut HashWriter>,
        deadline: Instant,
        budget_bytes: u64,
        progress: &mut Progress,
    ) -> Result<u64, PageError> {
        let values = bind_values(&sql.binds).map_err(PageError::Source)?;
        let refs: Vec<&dyn ToDbValue> = values
            .iter()
            .map(|v| match v {
                BindValue::N(n) => n as &dyn ToDbValue,
                BindValue::S(s) => s as &dyn ToDbValue,
            })
            .collect();
        let stmt = self
            .conn
            .as_ref()
            .map(|conn| {
                conn.statement(&sql.text)?
                    .fetch_array_size(if meta.columns.iter().any(|c| c.kind.is_lob()) {
                        1
                    } else {
                        512
                    })
                    .prefetch_rows(0)
                    .fetch_lobs()
                    .build()
            })
            .transpose()
            .map_err(|e| PageError::Driver(e.into()))?;
        let mut remaining = budget_bytes.saturating_sub(progress.bytes);
        let cursor: Box<dyn Iterator<Item = Result<Vec<Option<String>>, DriverError>> + '_> =
            if let Some(worker) = &self.worker {
                Box::new(
                    worker
                        .query_bounded(&sql.text, &sql.binds, 100_001, remaining.max(1))
                        .map_err(PageError::Driver)?,
                )
            } else {
                let cursor = stmt
                    .unwrap()
                    .query(&refs)
                    .map_err(|e| PageError::Driver(e.into()))?;
                let width = meta.columns.len();
                Box::new(cursor.map(move |row| {
                    let mut row = row?;
                    (0..width)
                        .map(|i| {
                            let value = if meta.columns[i].kind.is_lob() {
                                let lob: Option<oracledb::Lob> = row.take(i)?;
                                lob.map(|mut lob| {
                                    super::lob::read_locator(
                                        &mut lob,
                                        meta.columns[i].kind == ColKind::Blob,
                                        remaining,
                                        deadline,
                                    )
                                })
                                .transpose()?
                            } else {
                                row.get::<Option<String>>(i)?
                            };
                            remaining = remaining
                                .saturating_sub(value.as_ref().map_or(1, |v| v.len() as u64 + 1));
                            Ok(value)
                        })
                        .collect()
                }))
            };
        let width = meta.columns.len();
        let mut cols: Vec<Vec<Option<String>>> = vec![Vec::new(); width];
        let mut n = 0u64;
        let mut last = None;
        let mut buffered_bytes = 0;
        // Only flushed rows count as delivered; a failed page re-reads the
        // rows it buffered but did not hand to the sink.
        let flush = |cols: &mut Vec<Vec<Option<String>>>,
                     last: &mut Option<Vec<KeyValue>>,
                     buffered_bytes: &mut u64,
                     progress: &mut Progress,
                     sink: &mut dyn ChunkSink,
                     ipc: &mut StreamWriter<&mut HashWriter>|
         -> Result<(), PageError> {
            if cols[0].is_empty() {
                return Ok(());
            }
            let rows = cols[0].len() as u64;
            let arrays: Vec<ArrayRef> = cols
                .iter_mut()
                .map(|c| Arc::new(StringArray::from(std::mem::take(c))) as ArrayRef)
                .collect();
            let batch = RecordBatch::try_new(schema.clone(), arrays)
                .map_err(|_| PageError::Source(SourceError::protocol("arrow_batch")))?;
            ipc.write(&batch)
                .map_err(|_| PageError::Source(SourceError::protocol("arrow_ipc")))?;
            sink.push(batch).map_err(PageError::Source)?;
            progress.rows += rows;
            progress.bytes += std::mem::take(buffered_bytes);
            if last.is_some() {
                progress.last = last.take();
            }
            Ok(())
        };
        for row in cursor {
            let row = row.map_err(PageError::Driver)?;
            if Instant::now() > deadline {
                return Err(PageError::Source(SourceError::budget("chunk_deadline")));
            }
            let mut cells = Vec::with_capacity(width);
            if row.len() != width {
                return Err(PageError::Source(SourceError::protocol(
                    "worker_column_count",
                )));
            }
            for (v, c) in row.into_iter().zip(&meta.columns) {
                let v = v
                    .map(|v| canonical(c.kind, v))
                    .transpose()
                    .map_err(PageError::Source)?;
                buffered_bytes += v.as_ref().map_or(1, |s| s.len() as u64 + 1);
                cells.push(v);
            }
            if progress.bytes + buffered_bytes > budget_bytes {
                return Err(PageError::Source(SourceError::budget(
                    "chunk_over_budget_split",
                )));
            }
            if !meta.key_cols.is_empty() {
                last = Some(Self::row_key(meta, &cells).map_err(PageError::Source)?);
            }
            for (i, v) in cells.into_iter().enumerate() {
                cols[i].push(v);
            }
            n += 1;
            if cols[0].len() >= BATCH_ROWS {
                flush(
                    &mut cols,
                    &mut last,
                    &mut buffered_bytes,
                    progress,
                    sink,
                    ipc,
                )?;
            }
        }
        flush(
            &mut cols,
            &mut last,
            &mut buffered_bytes,
            progress,
            sink,
            ipc,
        )?;
        Ok(n)
    }

    fn read_chunk_sync(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt, SourceError> {
        let d = self.ensure_version()?;
        let state = self
            .tables
            .get(&chunk.table)
            .cloned()
            .ok_or(SourceError::catalog("table_not_planned"))?;
        let meta = &state.meta;
        let infos: Vec<ColumnInfo> = meta.columns.iter().map(ColumnInfo::from).collect();
        let schema = Arc::new(chunk_schema(&chunk.table, &infos));
        let timeout = Duration::from_millis(budget.statement_timeout_ms.max(1));
        self.set_timeout(timeout)?;
        let deadline = Instant::now() + timeout;
        let mut hw = HashWriter {
            hasher: Sha256::new(),
            bytes: 0,
        };
        let max_rows = budget.max_chunk_rows.min(100_000).max(1);
        let mut progress = Progress::default();
        let low;
        {
            let mut ipc = StreamWriter::try_new(&mut hw, &schema)
                .map_err(|_| SourceError::protocol("arrow_ipc"))?;
            match &state.strategy {
                ChunkStrategy::Keyset { .. } => {
                    let keys = meta.keys();
                    let scn = self.current_scn()?;
                    low = scn;
                    let mut page = max_rows + 1;
                    let mut retries = 0;
                    loop {
                        let after = progress.last.clone();
                        let lower = match &after {
                            Some(k) => Some(Bound {
                                values: k.as_slice(),
                                inclusive: false,
                            }),
                            None => chunk.lo.as_deref().map(|values| Bound {
                                values,
                                inclusive: true,
                            }),
                        };
                        let upper = chunk.hi.as_deref().map(|values| Bound {
                            values,
                            inclusive: false,
                        });
                        let limit = page.min(max_rows + 1 - progress.rows);
                        let sql = page_sql(
                            &d,
                            &chunk.table.schema,
                            &chunk.table.name,
                            &meta.columns,
                            PageRange::Keyset {
                                keys: &keys,
                                lower,
                                upper,
                            },
                            scn,
                            limit,
                        )?;
                        let result = self.run_page(
                            &sql,
                            meta,
                            &schema,
                            sink,
                            &mut ipc,
                            deadline,
                            budget.max_chunk_bytes,
                            &mut progress,
                        );
                        match result {
                            Ok(n) => {
                                if progress.rows > max_rows {
                                    return Err(SourceError::budget("chunk_over_budget_split"));
                                }
                                if n < limit {
                                    break;
                                }
                            }
                            Err(PageError::Driver(e)) if ora_code(&e) == Some(1555) => {
                                // Never change the snapshot after flushing:
                                // key moves across the cursor would be lost.
                                self.last_ora = Some(1555);
                                if !shrink_page(&mut page, &mut retries, self.cfg.snapshot_retries)
                                {
                                    return Err(map_driver(&e));
                                }
                                self.stats.snapshot_retries += 1;
                            }
                            Err(PageError::Driver(e)) => {
                                self.last_ora = ora_code(&e);
                                return Err(map_driver(&e));
                            }
                            Err(PageError::Source(e)) => return Err(e),
                        }
                    }
                }
                ChunkStrategy::PhysicalRange { .. } | ChunkStrategy::SingleRead => {
                    let scn = state
                        .facts
                        .snapshot_scn
                        .ok_or(SourceError::catalog("physical_plan_without_snapshot"))?;
                    if chunk.plan_revision != state.facts.plan_revision {
                        return Err(SourceError::catalog("physical_plan_revision_stale"));
                    }
                    low = scn;
                    let locator =
                        |v: &Option<Vec<KeyValue>>| -> Result<Option<String>, SourceError> {
                            match v.as_deref() {
                                None => Ok(None),
                                Some([KeyValue::Locator(l)]) => Ok(Some(l.clone())),
                                _ => Err(SourceError::catalog("rowid_bound_shape")),
                            }
                        };
                    let (lo, hi) = (locator(&chunk.lo)?, locator(&chunk.hi)?);
                    let sql = page_sql(
                        &d,
                        &chunk.table.schema,
                        &chunk.table.name,
                        &meta.columns,
                        PageRange::Rowid {
                            lo: lo.as_deref(),
                            hi: hi.as_deref(),
                        },
                        scn,
                        max_rows + 1,
                    )?;
                    match self.run_page(
                        &sql,
                        meta,
                        &schema,
                        sink,
                        &mut ipc,
                        deadline,
                        budget.max_chunk_bytes,
                        &mut progress,
                    ) {
                        Ok(_) => {}
                        Err(PageError::Driver(e)) => {
                            self.last_ora = ora_code(&e);
                            // ROWID ranges cannot move to a fresh SCN.
                            if ora_code(&e) == Some(1555) {
                                return Err(SourceError::budget("ora_01555_physical_replan"));
                            }
                            return Err(map_driver(&e));
                        }
                        Err(PageError::Source(e)) => return Err(e),
                    }
                    if progress.rows > max_rows {
                        return Err(SourceError::budget("chunk_over_budget_split"));
                    }
                }
            }
            if progress.rows == 0 {
                let batch = RecordBatch::new_empty(schema.clone());
                ipc.write(&batch)
                    .map_err(|_| SourceError::protocol("arrow_ipc"))?;
                sink.push(batch)?;
            }
            ipc.finish()
                .map_err(|_| SourceError::protocol("arrow_ipc"))?;
        }
        let high = match &state.strategy {
            ChunkStrategy::Keyset { .. } => self.current_scn()?,
            _ => low,
        };
        if hw.bytes > budget.max_chunk_bytes {
            return Err(SourceError::budget("chunk_over_budget_split"));
        }
        self.stats.chunks += 1;
        let rows = progress.rows;
        self.stats.rows += rows;
        self.stats.ipc_bytes += hw.bytes;
        Ok(ChunkReceipt {
            table: chunk.table.clone(),
            plan_revision: chunk.plan_revision,
            chunk_id: chunk.chunk_id,
            rows,
            bytes: hw.bytes,
            sha256: format!("{:x}", hw.hasher.finalize()),
            low: Some(Watermark::OracleScn { scn: low }),
            high: Some(Watermark::OracleScn { scn: high }),
        })
    }

    /// Split an over-budget keyset chunk into a new plan revision. Physical
    /// chunks cannot be split outside their snapshot: plan the table again.
    pub fn split_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
    ) -> Result<Vec<ChunkSpec>, SourceError> {
        validate_budget(budget)?;
        let state = self
            .tables
            .get(&chunk.table)
            .cloned()
            .ok_or(SourceError::catalog("table_not_planned"))?;
        if !matches!(state.strategy, ChunkStrategy::Keyset { .. }) {
            return Err(SourceError::catalog("physical_chunk_replan_required"));
        }
        let step = (budget.max_chunk_rows / 2).max(1);
        let bounds = self.probe_boundaries(
            &chunk.table,
            &state.meta,
            chunk.lo.clone(),
            chunk.hi.as_deref(),
            step,
        )?;
        let revision = state.facts.plan_revision + 1;
        let mut edges: Vec<Option<Vec<KeyValue>>> = vec![chunk.lo.clone()];
        edges.extend(bounds.into_iter().map(Some));
        edges.push(chunk.hi.clone());
        let st = self.tables.get_mut(&chunk.table).expect("planned");
        st.facts.plan_revision = revision;
        let specs = edges
            .windows(2)
            .map(|w| {
                let id = st.next_chunk_id;
                st.next_chunk_id += 1;
                ChunkSpec {
                    table: chunk.table.clone(),
                    plan_revision: revision,
                    chunk_id: id,
                    lo: w[0].clone(),
                    hi: w[1].clone(),
                    estimated_rows: Some(step),
                }
            })
            .collect();
        Ok(specs)
    }

    /// Keys whose row changed in `(from_scn, to_scn]` inside `[lo, hi)`,
    /// read with a Flashback Version Query (read-only, undo-bounded). Used
    /// to correct a chunk read at `from_scn` to `to_scn`; old and new keys of
    /// a key update are both reported.
    pub fn changed_keys(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
        from_scn: u64,
        to_scn: u64,
    ) -> Result<Vec<Vec<KeyValue>>, SourceError> {
        let state = self
            .tables
            .get(table)
            .cloned()
            .ok_or(SourceError::catalog("table_not_planned"))?;
        let meta = &state.meta;
        if meta.key_cols.is_empty() {
            return Err(SourceError::catalog("changed_keys_needs_key"));
        }
        let keys = meta.keys();
        let mut sql = Sql::default();
        let a = format!(":b{}", {
            sql.binds.push(Bind::Number(from_scn.to_string()));
            sql.binds.len()
        });
        let b = format!(":b{}", {
            sql.binds.push(Bind::Number(to_scn.to_string()));
            sql.binds.len()
        });
        let a2 = format!(":b{}", {
            sql.binds.push(Bind::Number(from_scn.to_string()));
            sql.binds.len()
        });
        let b2 = format!(":b{}", {
            sql.binds.push(Bind::Number(to_scn.to_string()));
            sql.binds.len()
        });
        let pred = keyset::range_predicate(
            &mut sql,
            &keys,
            lo.map(|values| Bound {
                values,
                inclusive: true,
            }),
            hi.map(|values| Bound {
                values,
                inclusive: false,
            }),
        )?;
        let proj = keys
            .iter()
            .map(|c| keyset::projection(c))
            .collect::<Result<Vec<_>, _>>()?
            .join(",");
        let range = if pred.is_empty() {
            String::new()
        } else {
            format!(" AND {pred}")
        };
        sql.text = format!(
            "SELECT DISTINCT {proj} FROM {} VERSIONS BETWEEN SCN {a} AND {b} WHERE ((versions_startscn > {a2}) OR (versions_endscn <= {b2})){range}",
            keyset::table_name(&table.schema, &table.name)?
        );
        let rows = self.query(&sql.text, &sql.binds, 10_000_000)?;
        rows.iter()
            .map(|r| {
                keys.iter()
                    .zip(r)
                    .map(|(c, v)| {
                        keyset::key_value(
                            c.kind,
                            v.as_deref().ok_or(SourceError::protocol("null_key"))?,
                        )
                    })
                    .collect()
            })
            .collect()
    }

    fn probe_sync(&mut self) -> Result<ProbeReport, SourceError> {
        let d = self.ensure_version()?;
        let version = self.version.clone().expect("version set");
        let user = self.session_user.clone();
        let mut settings = BTreeMap::new();
        let mut caps = BTreeMap::new();
        settings.insert("version_raw".into(), version.raw.clone());
        settings.insert("session_user".into(), user.clone());
        settings.insert(
            "driver".into(),
            if self.worker.is_some() {
                "external thick worker (Arrow IPC)"
            } else {
                "oracledb 26.0.0-beta.4 thin"
            }
            .into(),
        );
        let container = match self.query_raw(
            "SELECT SYS_CONTEXT('USERENV','CON_NAME'), SYS_CONTEXT('USERENV','CON_ID') FROM dual",
            &[],
            1,
        ) {
            Ok(r) => r.into_iter().next().and_then(|r| r[0].clone()),
            Err(_) => None,
        };
        let db = if self.readable("v$database")? {
            let cdb = if d.multitenant == Multitenant::NonCdbOnly {
                "'NO'"
            } else {
                "cdb"
            };
            self.query(
                &format!("SELECT log_mode, supplemental_log_data_min, supplemental_log_data_pk, supplemental_log_data_all, force_logging, {cdb} FROM v$database"),
                &[],
                1,
            )?
            .into_iter()
            .next()
        } else {
            None
        };
        let cdb = match d.multitenant {
            Multitenant::NonCdbOnly => false,
            Multitenant::PdbOnly => true,
            Multitenant::Optional => db
                .as_ref()
                .map(|r| r[5].as_deref() == Some("YES"))
                .unwrap_or_else(|| {
                    container
                        .as_deref()
                        .is_some_and(|c| c != "NON-CDB" && !c.is_empty())
                }),
        };
        settings.insert("cdb".into(), cdb.to_string());
        if let Some(c) = &container {
            settings.insert("container".into(), c.clone());
        }
        let (comp, sort) = self.ensure_nls()?;
        settings.insert("nls_comp".into(), comp);
        settings.insert("nls_sort".into(), sort);
        settings.insert(
            "logminer.dictionary".into(),
            rules::DICTIONARY_STRATEGY.into(),
        );
        settings.insert(
            "logminer.redo_log_catalog".into(),
            rules::REDO_LOG_CATALOG_STATE.into(),
        );
        settings.insert(
            "logminer.continuous_mine".into(),
            d.continuous_mine_label().into(),
        );
        settings.insert("logminer.log_list".into(), d.log_list().into());
        settings.insert(
            "logminer.excluded_tablespaces".into(),
            d.logminer_excluded_tablespaces.join(","),
        );
        settings.insert("digest.algorithm".into(), d.digest_algorithm().into());
        settings.insert(
            "chunk.page_limit".into(),
            if d.fetch_first {
                "fetch_first"
            } else {
                "rownum"
            }
            .into(),
        );
        settings.insert("snapshot.consistency".into(), "AS OF SCN per chunk".into());
        settings.insert(
            "c1.grade".into(),
            "experimental until T7 audit (Plan D8)".into(),
        );
        if let Some(r) = &db {
            for (i, k) in [
                "log_mode",
                "supplemental_log_data_min",
                "supplemental_log_data_pk",
                "supplemental_log_data_all",
                "force_logging",
            ]
            .iter()
            .enumerate()
            {
                settings.insert((*k).into(), r[i].clone().unwrap_or_default());
            }
        }
        let privs: Vec<String> = self
            .query("SELECT privilege FROM session_privs", &[], 1000)?
            .into_iter()
            .filter_map(|r| r[0].clone())
            .collect();
        let has = |p: &str| privs.iter().any(|v| v == p);
        let excess: Vec<&str> = ["SELECT ANY TABLE", "FLASHBACK ANY TABLE", "LOCK ANY TABLE"]
            .into_iter()
            .filter(|p| has(p))
            .collect();
        if !excess.is_empty() {
            settings.insert("excess_privileges".into(), excess.join(","));
        }

        caps.insert(Capability::Catalog, CapabilityState::Available);

        // Snapshot: SELECT and FLASHBACK on each scoped table.
        let mut snap = Vec::new();
        let mut supp_tables = Vec::new();
        for t in self.cfg.scope.clone() {
            let granted: Vec<String> = if t.schema == user {
                vec!["SELECT".into(), "FLASHBACK".into()]
            } else {
                self.query(
                    "SELECT privilege FROM all_tab_privs WHERE table_schema = :b1 AND table_name = :b2 AND grantee IN (USER,'PUBLIC') UNION SELECT privilege FROM role_tab_privs WHERE owner = :b3 AND table_name = :b4 AND role IN (SELECT role FROM session_roles)",
                    &[
                        Bind::Text(t.schema.clone()),
                        Bind::Text(t.name.clone()),
                        Bind::Text(t.schema.clone()),
                        Bind::Text(t.name.clone()),
                    ],
                    64,
                )?
                .into_iter()
                .filter_map(|r| r[0].clone())
                .collect()
            };
            if !granted.iter().any(|p| p == "SELECT") && !has("SELECT ANY TABLE") {
                snap.push(rules::select_grant(&t.schema, &t.name, &user));
            }
            if !granted.iter().any(|p| p == "FLASHBACK") && !has("FLASHBACK ANY TABLE") {
                snap.push(rules::flashback_grant(&t.schema, &t.name, &user));
            }
            let keyed = self
                .load_meta(&t)
                .map(|m| {
                    matches!(
                        m.key,
                        KeyShape::IntegerPrimaryKey { .. }
                            | KeyShape::StringPrimaryKey { .. }
                            | KeyShape::CompositePrimaryKey { .. }
                    )
                })
                .unwrap_or(false);
            let groups = self.query(
                "SELECT log_group_type FROM all_log_groups WHERE owner = :b1 AND table_name = :b2",
                &[Bind::Text(t.schema.clone()), Bind::Text(t.name.clone())],
                16,
            )?;
            let has_group = groups.iter().any(|r| {
                matches!(r[0].as_deref(), Some("ALL COLUMN LOGGING"))
                    || (keyed && r[0].as_deref() == Some("PRIMARY KEY LOGGING"))
            });
            let db_level = db.as_ref().is_some_and(|r| {
                r[3].as_deref() == Some("YES") || (keyed && r[2].as_deref() == Some("YES"))
            });
            if !has_group && !db_level {
                supp_tables.push(rules::supplemental_table(&t.schema, &t.name, keyed));
            }
        }
        caps.insert(
            Capability::ChunkedSnapshot,
            if snap.is_empty() {
                CapabilityState::Available
            } else {
                CapabilityState::NeedsSetup { requirements: snap }
            },
        );

        let scn_ok = self.current_scn().is_ok();
        caps.insert(
            Capability::Watermark,
            if scn_ok {
                CapabilityState::Available
            } else {
                CapabilityState::NeedsSetup {
                    requirements: vec![rules::scn_source(&user)],
                }
            },
        );
        if let Some(s) = self.scn_source {
            settings.insert(
                "watermark.source".into(),
                match s {
                    ScnSource::VDatabase => "V$DATABASE.CURRENT_SCN",
                    ScnSource::DbmsFlashback => "DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER",
                }
                .into(),
            );
        }

        // D11 is an opt-in requirement even when every DBA prerequisite is met.
        let mut c1: Vec<Requirement> = Vec::new();
        match &db {
            Some(r) => {
                if r[0].as_deref() != Some("ARCHIVELOG") {
                    c1.push(rules::archivelog(r[0].clone()));
                }
                if !matches!(r[1].as_deref(), Some("YES" | "IMPLICIT")) {
                    c1.push(rules::supplemental_minimal(r[1].clone()));
                }
            }
            None => {
                c1.push(rules::view_grant("V$DATABASE", &user, cdb));
                c1.push(rules::archivelog(Some(
                    "unknown: V$DATABASE not readable".into(),
                )));
            }
        }
        c1.extend(supp_tables);
        let mining = if d.logmining_privilege {
            has("LOGMINING")
        } else {
            has("SELECT ANY TRANSACTION")
        };
        if !mining {
            c1.push(rules::logmining(&d, &user, cdb));
        }
        let logmnr = has("EXECUTE ANY PROCEDURE") || self
            .scalar(
                "SELECT TO_CHAR(COUNT(*)) FROM (SELECT privilege FROM all_tab_privs WHERE table_schema = 'SYS' AND table_name = 'DBMS_LOGMNR' AND grantee IN (USER,'PUBLIC') AND privilege = 'EXECUTE' UNION ALL SELECT privilege FROM role_tab_privs WHERE owner = 'SYS' AND table_name = 'DBMS_LOGMNR' AND privilege = 'EXECUTE' AND role IN (SELECT role FROM session_roles))",
                &[],
            )?
            .is_some_and(|v| v != "0");
        if !logmnr {
            c1.push(rules::execute_logmnr(&user, cdb));
        }
        for view in [
            "V$ARCHIVED_LOG",
            "V$LOG",
            "V$LOGFILE",
            "V$LOGMNR_CONTENTS",
            "V$LOGMNR_LOGS",
            "V$TRANSACTION",
        ] {
            if !self.readable(view)? {
                c1.push(rules::view_grant(view, &user, cdb));
            }
        }
        if cdb && (container.as_deref() != Some("CDB$ROOT") || !user.starts_with("C##")) {
            c1.push(rules::cdb_root_session(
                container.as_deref().unwrap_or("unknown"),
                &user,
            ));
        }
        c1.push(logminer_opt_in());
        caps.insert(
            Capability::LogCdc,
            if c1.is_empty() {
                CapabilityState::Available
            } else {
                CapabilityState::NeedsSetup { requirements: c1 }
            },
        );
        caps.insert(
            Capability::NativeChanges,
            CapabilityState::Unsupported {
                reason: "XStream needs a GoldenGate license; not offered".into(),
            },
        );
        caps.insert(Capability::DigestDiff, CapabilityState::Available);
        caps.insert(
            Capability::ReplicaRead,
            CapabilityState::Unsupported {
                reason: "Active Data Guard reads need a separate license confirmation".into(),
            },
        );

        let gate = self.readable("v$session")?;
        self.session_gate = Some(gate);
        settings.insert(
            "protection.active_session_gate".into(),
            if gate { "V$SESSION" } else { "unavailable" }.into(),
        );
        let plan = self.readable("v$sql_plan")?;
        self.plan_view = Some(plan && gate);
        settings.insert(
            "plan_check".into(),
            if plan && gate {
                "V$SQL_PLAN of the shared cursor (no EXPLAIN PLAN)"
            } else {
                "unavailable: grant SELECT on V_$SQL_PLAN and V_$SESSION"
            }
            .into(),
        );
        let ext = self.readable("dba_extents")?;
        self.extents_view = Some(ext);
        settings.insert(
            "no_key_strategy".into(),
            if ext {
                "rowid_ranges from DBA_EXTENTS + DBMS_ROWID.ROWID_CREATE"
            } else {
                "single_read (grant SELECT on DBA_EXTENTS for ROWID ranges)"
            }
            .into(),
        );
        if let Ok(Some(v)) = self.scalar(
            "SELECT value FROM v$parameter WHERE name = 'undo_retention'",
            &[],
        ) {
            settings.insert("undo_retention".into(), v);
        }
        settings.insert(
            "protection.call_timeout".into(),
            "client call timeout per round trip; no server statement timeout without Resource Manager".into(),
        );
        Ok(ProbeReport {
            protocol: ENGINE_PROTOCOL.into(),
            family: EngineFamily::Oracle,
            version,
            capabilities: caps,
            settings,
        })
    }

    fn plan_sync(
        &mut self,
        t: &TableRef,
        budget: &ReadBudget,
    ) -> Result<TableChunkPlan, SourceError> {
        self.ensure_version()?;
        self.set_timeout(Duration::from_millis(budget.statement_timeout_ms.max(1)))?;
        let meta = self.load_meta(t)?;
        let chunk_rows = Self::chunk_rows(&meta, budget);
        let revision = self.tables.get(t).map_or(1, |s| s.facts.plan_revision + 1);
        let empty_null = meta
            .columns
            .iter()
            .filter(|c| c.kind.is_text())
            .map(|c| c.name.clone())
            .collect();
        let mut facts = PlanFacts {
            plan_revision: revision,
            key_index: meta.key_index.clone(),
            boundary_method: "",
            index_check: IndexCheck::NotApplicable,
            snapshot_scn: None,
            nls_comp: String::new(),
            nls_sort: String::new(),
            empty_string_is_null_columns: empty_null,
            chunk_rows,
        };
        let (strategy, edges, estimate) = if !meta.key_cols.is_empty() {
            let (comp, sort) = self.check_key_collation(&meta)?;
            facts.nls_comp = comp;
            facts.nls_sort = sort;
            let uniform = if meta.integer_key {
                self.integer_boundaries(t, &meta, chunk_rows)?
            } else {
                None
            };
            let (bounds, est) = match uniform {
                Some((b, count)) => {
                    facts.boundary_method = "uniform_integer_step";
                    (b, Some(count))
                }
                None => {
                    facts.boundary_method = "keyset_offset_probe";
                    (
                        self.probe_boundaries(t, &meta, None, None, chunk_rows)?,
                        meta.num_rows,
                    )
                }
            };
            facts.index_check = self.verify_index(t, &meta, &bounds)?;
            let columns = meta.keys().iter().map(|c| c.name.clone()).collect();
            (ChunkStrategy::Keyset { columns }, bounds, est)
        } else {
            let scn = self.current_scn()?;
            facts.snapshot_scn = Some(scn);
            if self.extents_view.is_none() {
                let v = self.readable("dba_extents")?;
                self.extents_view = Some(v);
            }
            if self.extents_view == Some(true) {
                facts.boundary_method = "dba_extents_rowid_create";
                let b = self.rowid_boundaries(t, &meta, chunk_rows)?;
                (
                    ChunkStrategy::PhysicalRange {
                        kind: PhysicalRangeKind::Rowid,
                    },
                    b.into_iter().map(|r| vec![KeyValue::Locator(r)]).collect(),
                    meta.num_rows,
                )
            } else {
                facts.boundary_method = "single_read";
                (ChunkStrategy::SingleRead, Vec::new(), meta.num_rows)
            }
        };
        let mut all: Vec<Option<Vec<KeyValue>>> = vec![None];
        all.extend(edges.into_iter().map(Some));
        all.push(None);
        let chunks: Vec<ChunkSpec> = all
            .windows(2)
            .enumerate()
            .map(|(i, w)| ChunkSpec {
                table: t.clone(),
                plan_revision: revision,
                chunk_id: i as u32,
                lo: w[0].clone(),
                hi: w[1].clone(),
                estimated_rows: Some(chunk_rows),
            })
            .collect();
        let plan = TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table: t.clone(),
            key: meta.key.clone(),
            strategy: strategy.clone(),
            estimated_rows: estimate,
            chunks,
        };
        let next_chunk_id = plan.chunks.len() as u32;
        self.tables.insert(
            t.clone(),
            TableState {
                meta,
                strategy,
                facts,
                next_chunk_id,
            },
        );
        Ok(plan)
    }

    fn digest_sync(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest, SourceError> {
        let d = self.ensure_version()?;
        let meta = match self.tables.get(table) {
            Some(s) => s.meta.clone(),
            None => self.load_meta(table)?,
        };
        let keys = meta.keys();
        let locator = |v: Option<&[KeyValue]>| -> Result<Option<String>, SourceError> {
            match v {
                None => Ok(None),
                Some([KeyValue::Locator(l)]) => Ok(Some(l.clone())),
                _ => Err(SourceError::catalog("rowid_bound_shape")),
            }
        };
        let is_locator = |v: Option<&[KeyValue]>| matches!(v, Some([KeyValue::Locator(_)]));
        let (rlo, rhi);
        let range = if keys.is_empty() || is_locator(lo) || is_locator(hi) {
            rlo = locator(lo)?;
            rhi = locator(hi)?;
            PageRange::Rowid {
                lo: rlo.as_deref(),
                hi: rhi.as_deref(),
            }
        } else {
            self.check_key_collation(&meta)?;
            PageRange::Keyset {
                keys: &keys,
                lower: lo.map(|values| Bound {
                    values,
                    inclusive: true,
                }),
                upper: hi.map(|values| Bound {
                    values,
                    inclusive: false,
                }),
            }
        };
        let snapshot = if is_locator(lo) || is_locator(hi) {
            Some(
                self.tables
                    .get(table)
                    .and_then(|s| s.facts.snapshot_scn)
                    .ok_or(SourceError::catalog("physical_digest_requires_snapshot"))?,
            )
        } else {
            None
        };
        let sql = digest_sql(
            &d,
            &table.schema,
            &table.name,
            &meta.columns,
            range,
            snapshot,
        )?;
        let row = self
            .query(&sql.text, &sql.binds, 1)?
            .into_iter()
            .next()
            .ok_or(SourceError::protocol("digest_row"))?;
        let rows: u64 = row[0]
            .as_deref()
            .and_then(|v| v.parse().ok())
            .ok_or(SourceError::protocol("digest_count"))?;
        let lanes: Vec<String> = row[1..]
            .iter()
            .map(|v| v.clone().unwrap_or_else(|| "0".into()))
            .collect();
        Ok(RangeDigest {
            table: table.clone(),
            lo: lo.map(<[KeyValue]>::to_vec),
            hi: hi.map(<[KeyValue]>::to_vec),
            rows,
            digest: format!("{}:{}", d.digest_algorithm(), lanes.join("-")),
        })
    }
}

/// Rows, last key and bytes handed to the sink so far in one chunk.
#[derive(Default)]
struct Progress {
    rows: u64,
    last: Option<Vec<KeyValue>>,
    bytes: u64,
}

fn logminer_opt_in() -> Requirement {
    Requirement {
        kind: crate::engine::RequirementKind::Feature,
        name: "logminer_reader".into(),
        observed: Some(
            "R1 with Oracle-internal recursive temporary writes to SYS.LOGMNR_GT_*; \
             driver ALTER SESSION time zone/NLS settings (D11)"
                .into(),
        ),
        required: "Explicit Oracle C1 opt-in and version-specific T7 acceptance; \
                   sync: auto must remain C5"
            .into(),
        dba_statement: None,
    }
}

enum PageError {
    Driver(DriverError),
    Source(SourceError),
}

impl SourceEngine for OracleEngine {
    fn family(&self) -> EngineFamily {
        EngineFamily::Oracle
    }

    async fn probe(&mut self) -> Result<ProbeReport, SourceError> {
        self.set_timeout(Duration::from_secs(15))?;
        self.probe_sync()
    }

    async fn plan_chunks(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<TableChunkPlan, SourceError> {
        validate_budget(budget)?;
        self.protect().await?;
        let started = Instant::now();
        let result = self.plan_sync(table, budget);
        self.schedule_next(started.elapsed(), budget);
        self.pace().await;
        result
    }

    async fn read_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt, SourceError> {
        validate_budget(budget)?;
        self.protect().await?;
        let started = Instant::now();
        let result = self.read_chunk_sync(chunk, budget, sink).map_err(|e| {
            // The thin driver can close the socket when timeout cancellation
            // itself times out. Keep that budget failure distinct from a
            // protocol failure, without treating arbitrary disconnects as timeouts.
            if e.fact() == Some("connection_recovery_failed")
                && started.elapsed() >= Duration::from_millis(budget.statement_timeout_ms)
            {
                SourceError::budget("timeout_connection_closed")
            } else {
                e
            }
        });
        self.schedule_next(started.elapsed(), budget);
        self.pace().await;
        result
    }

    async fn watermark(&mut self) -> Result<Option<Watermark>, SourceError> {
        Ok(Some(Watermark::OracleScn {
            scn: self.current_scn()?,
        }))
    }

    async fn digest_range(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest, SourceError> {
        let budget = ReadBudget::default();
        self.set_timeout(Duration::from_millis(budget.statement_timeout_ms))?;
        self.protect().await?;
        let started = Instant::now();
        let result = self.digest_sync(table, lo, hi);
        self.schedule_next(started.elapsed(), &budget);
        self.pace().await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logminer_disclosure_is_an_unconditional_feature_requirement() {
        let requirement = logminer_opt_in();
        assert_eq!(requirement.kind, crate::engine::RequirementKind::Feature);
        assert!(requirement.observed.unwrap().contains("SYS.LOGMNR_GT_*"));
        assert!(requirement.required.contains("Explicit Oracle C1 opt-in"));
        assert!(requirement.required.contains("sync: auto must remain C5"));
        assert!(requirement.dba_statement.is_none());
    }

    #[test]
    fn engine_futures_are_send() {
        fn send<T: Send>() {}
        send::<OracleEngine>();
    }

    #[test]
    fn budgets_reject_zero_and_overflow_before_source_calls() {
        assert!(validate_budget(&ReadBudget::default()).is_ok());
        let mut budget = ReadBudget {
            max_chunk_rows: u64::MAX,
            ..ReadBudget::default()
        };
        assert!(validate_budget(&budget).is_err());
        budget.max_chunk_rows = 1;
        budget.statement_timeout_ms = 0;
        assert!(validate_budget(&budget).is_err());
        budget.statement_timeout_ms = 100;
        budget.duty_cycle_percent = 0;
        assert!(validate_budget(&budget).is_err());
    }
    #[test]
    fn canonical_cells_are_lossless_and_era_checked() {
        assert_eq!(canonical(ColKind::Number, ".5".into()).unwrap(), "0.5");
        assert_eq!(
            canonical(ColKind::Date, " 2024-01-02T03:04:05".into()).unwrap(),
            "2024-01-02T03:04:05"
        );
        assert!(canonical(ColKind::Date, "-0044-03-15T00:00:00".into()).is_err());
        assert_eq!(canonical(ColKind::Raw, "0AFF".into()).unwrap(), "0aff");
        assert_eq!(canonical(ColKind::Char, "a  ".into()).unwrap(), "a  ");
    }

    #[test]
    fn snapshot_retry_halves_pages_and_stops_even_at_one_row() {
        let (mut page, mut retries) = (8, 0);
        for expected in [4, 2, 1, 1] {
            assert!(shrink_page(&mut page, &mut retries, 4));
            assert_eq!(page, expected);
        }
        assert!(!shrink_page(&mut page, &mut retries, 4));
        assert_eq!(retries, 4);
        assert!(!shrink_page(&mut page, &mut 0, 0));
    }

    #[test]
    fn schema_flags_empty_string_semantics() {
        let t = TableRef {
            schema: "APP".into(),
            name: "T".into(),
        };
        let s = chunk_schema(
            &t,
            &[
                ColumnInfo {
                    name: "ID".into(),
                    data_type: "NUMBER".into(),
                    canonical: "decimal",
                    text: false,
                },
                ColumnInfo {
                    name: "NAME".into(),
                    data_type: "VARCHAR2".into(),
                    canonical: "text",
                    text: true,
                },
            ],
        );
        assert!(
            s.field(0)
                .metadata()
                .get("oo.oracle.empty_string_is_null")
                .is_none()
        );
        assert_eq!(
            s.field(1).metadata()["oo.oracle.empty_string_is_null"],
            "true"
        );
        assert_eq!(s.metadata()["oo.table"], "APP.T");
        for (data_type, kind, canonical) in [
            ("CLOB", ColKind::Clob, "text"),
            ("NCLOB", ColKind::NClob, "text"),
            ("BLOB", ColKind::Blob, "hex"),
        ] {
            let s = chunk_schema(
                &t,
                &[ColumnInfo {
                    name: "LOB".into(),
                    data_type: data_type.into(),
                    canonical: kind.canonical(),
                    text: kind.is_text(),
                }],
            );
            assert_eq!(s.field(0).data_type(), &DataType::Utf8);
            assert_eq!(s.field(0).metadata()["oo.oracle.type"], data_type);
            assert_eq!(s.field(0).metadata()["oo.oracle.canonical"], canonical);
            assert!(
                !s.field(0)
                    .metadata()
                    .contains_key("oo.oracle.empty_string_is_null")
            );
        }
    }

    #[test]
    fn driver_errors_map_to_closed_facts() {
        let m = map_error_kind(&ErrorKind::CallTimeoutExceeded);
        assert_eq!(m.kind(), SourceErrorKind::Budget);
        assert_eq!(m.fact(), Some("call_timeout"));
        let v = map_error_kind(&ErrorKind::ServerVersionNotSupported);
        assert_eq!(v.kind(), SourceErrorKind::Version);
        let recovery = map_error_kind(&ErrorKind::UnableToRecover);
        assert_eq!(recovery.kind(), SourceErrorKind::Connect);
        assert_eq!(recovery.fact(), Some("connection_recovery_failed"));
    }
}
