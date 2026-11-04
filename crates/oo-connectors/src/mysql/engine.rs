//! [`SourceEngine`] for MySQL 5.7, 8.0, 8.4 and 9.x (S0-C-01, S1-C-01, S1-C-02).
//!
//! One engine owns at most one connection. Every statement passes
//! [`keyset::guard`]: SELECT, SHOW, EXPLAIN, session settings and a
//! read-only consistent-snapshot transaction, never LOCK, FLUSH or DDL.
//! Each chunk is one `START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY`
//! read in key order, paged with [`AdaptiveRows`] so every statement stays
//! short, bracketed by a low watermark before the snapshot and a high one
//! right after it. String bounds are always parameters compared by the
//! server under the key column's collation.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use arrow_array::RecordBatch;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Params, Row, Value};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::capability::{self, Dialect, Grants, Observed};
use super::chunk::AdaptiveRows;
use super::gtid::GtidSet;
use super::keyset::{self, ChosenKey, IndexColumn, KeyCol, KeyKind, Lower, PageSql, Split};
use super::map::{MappedColumn, capture_expression, map_capture_column};
use super::protect::{DutyCycle, Protection, ProtectionStats, load_verdict};
use super::{MysqlConfig, MysqlError, SelectedTable, mysql_ident};
use crate::engine::{
    ChunkReceipt, ChunkSink, ChunkSpec, ChunkStrategy, ENGINE_PROTOCOL, EngineFamily,
    EngineVersion, KeyShape, KeyValue, ProbeReport, RangeDigest, ReadBudget, SourceEngine,
    TableChunkPlan, TableRef, Watermark,
};
use crate::types::RunBinding;
use crate::{SourceError, SourceErrorKind};

#[cfg(feature = "mysql-binlog")]
#[path = "change.rs"]
pub mod change;
#[cfg(feature = "mysql-binlog")]
#[path = "repair.rs"]
pub mod repair;

type Result<T> = std::result::Result<T, SourceError>;

/// Server error: maximum statement execution time exceeded.
const ER_QUERY_TIMEOUT: u16 = 3024;
/// Raw bytes per Arrow batch inside one chunk; deterministic grouping keeps
/// the chunk's IPC digest independent of page sizes.
const BATCH_RAW_BYTES: u64 = 8 * 1024 * 1024;
const BATCH_ROWS: usize = 50_000;
/// Keys per point-read statement in [`MysqlEngine::read_keys`].
const POINT_KEYS: usize = 100;

/// Engine knobs. Budgets are internal resource limits, never business limits.
#[derive(Clone, Debug)]
pub struct EngineOptions {
    pub budget: ReadBudget,
    pub protection: Protection,
    /// Estimated chunks above which boundaries come from a sample.
    pub sample_threshold_chunks: u64,
    /// One sampled key in `sample_rate`.
    pub sample_rate: u64,
    /// Rows in the first page of a chunk read.
    pub initial_page_rows: u64,
    pub source_id: String,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            budget: ReadBudget::default(),
            protection: Protection::default(),
            sample_threshold_chunks: 1000,
            sample_rate: 1000,
            initial_page_rows: 5000,
            source_id: "mysql".into(),
        }
    }
}

/// Key and canonical row hash of one row read, for chunk comparison.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RowFingerprint {
    pub key: Vec<KeyValue>,
    pub canonical_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WatermarkMode {
    GtidSet,
    FilePos,
    None,
}

fn file_position_mode(mode: WatermarkMode) -> Result<WatermarkMode> {
    match mode {
        WatermarkMode::None => Err(SourceError::config("binlog_unavailable")),
        WatermarkMode::GtidSet | WatermarkMode::FilePos => Ok(WatermarkMode::FilePos),
    }
}

#[derive(Clone)]
struct TableMeta {
    key: ChosenKey,
    columns: Vec<MappedColumn>,
    names: Vec<String>,
    select_list: String,
    binding: RunBinding,
    /// Positions of key columns in the select list.
    key_positions: Vec<usize>,
}

struct Basics {
    version: EngineVersion,
    dialect: Dialect,
    watermark: WatermarkMode,
    max_connections: Option<u64>,
}

enum ExecError {
    Timeout,
    ServerTimeout,
    Server(u16),
    Io,
}

pub struct MysqlEngine {
    config: MysqlConfig,
    password: Zeroizing<String>,
    options: EngineOptions,
    conn: Option<Conn>,
    basics: Option<Basics>,
    tables: HashMap<TableRef, TableMeta>,
    duty: DutyCycle,
    stats: ProtectionStats,
    first_op: Option<Instant>,
    last_op: Option<Instant>,
    idle_since: Option<Instant>,
    next_split_id: u32,
}

fn mysql_err(e: MysqlError) -> SourceError {
    let f = e.fact;
    match e.code {
        "config" => SourceError::config(f),
        "connect" => SourceError::connect(f),
        "tls" => SourceError::tls(f),
        "budget" => SourceError::budget(f),
        "catalog" => SourceError::catalog(f),
        "ident" => SourceError::identifier(f),
        "privilege" => SourceError::privilege(f),
        "version" => SourceError::version(f),
        _ => SourceError::protocol(f),
    }
}

fn exec_err(e: &mysql_async::Error) -> ExecError {
    match e {
        mysql_async::Error::Server(s) if s.code == ER_QUERY_TIMEOUT => ExecError::ServerTimeout,
        mysql_async::Error::Server(s) => ExecError::Server(s.code),
        _ => ExecError::Io,
    }
}

fn params(values: Vec<Value>) -> Params {
    if values.is_empty() {
        Params::Empty
    } else {
        Params::Positional(values)
    }
}

/// Engine key value to a driver parameter. The server compares it.
pub fn key_param(v: &KeyValue) -> Result<Value> {
    Ok(match v {
        KeyValue::Int(n) if *n < 0 => {
            Value::Int(i64::try_from(*n).map_err(|_| SourceError::config("key_integer"))?)
        }
        KeyValue::Int(n) => {
            Value::UInt(u64::try_from(*n).map_err(|_| SourceError::config("key_integer"))?)
        }
        KeyValue::Decimal(s) | KeyValue::Text(s) => Value::Bytes(s.as_bytes().to_vec()),
        KeyValue::Bytes(hex) => Value::Bytes(unhex(hex)?),
        KeyValue::Null | KeyValue::Locator(_) => return Err(SourceError::config("key_value")),
    })
}

fn unhex(hex: &str) -> Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) || !hex.is_ascii() {
        return Err(SourceError::config("key_bytes"));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| SourceError::config("key_bytes"))
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Driver value of a key column to an engine key value, losslessly. Text
/// that is not UTF-8 is kept as bytes; both bind back to the same bytes.
pub fn key_value(kind: KeyKind, v: &Value) -> Result<KeyValue> {
    Ok(match (kind, v) {
        (_, Value::NULL) => return Err(SourceError::protocol("key_null")),
        (KeyKind::Int, Value::Int(n)) => KeyValue::Int(i128::from(*n)),
        (KeyKind::Int, Value::UInt(n)) => KeyValue::Int(i128::from(*n)),
        (KeyKind::Int, Value::Bytes(b)) => KeyValue::Int(
            std::str::from_utf8(b)
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| SourceError::protocol("key_integer"))?,
        ),
        (KeyKind::Decimal, Value::Bytes(b)) => KeyValue::Decimal(
            String::from_utf8(b.clone()).map_err(|_| SourceError::protocol("key_decimal"))?,
        ),
        (KeyKind::Decimal, Value::Int(n)) => KeyValue::Decimal(n.to_string()),
        (KeyKind::Decimal, Value::UInt(n)) => KeyValue::Decimal(n.to_string()),
        (KeyKind::Text, Value::Bytes(b)) => match String::from_utf8(b.clone()) {
            Ok(s) => KeyValue::Text(s),
            Err(_) => KeyValue::Bytes(hex(b)),
        },
        (KeyKind::Bytes, Value::Bytes(b)) => KeyValue::Bytes(hex(b)),
        _ => return Err(SourceError::protocol("key_value_type")),
    })
}

fn row_value(row: &Row, i: usize) -> Result<Value> {
    row.as_ref(i)
        .cloned()
        .ok_or_else(|| SourceError::protocol("row_width"))
}

fn row_text(row: &Row, i: usize) -> Option<String> {
    match row.as_ref(i)? {
        Value::Bytes(b) => String::from_utf8(b.clone()).ok(),
        Value::Int(n) => Some(n.to_string()),
        Value::UInt(n) => Some(n.to_string()),
        _ => None,
    }
}

fn row_u64(row: &Row, i: usize) -> Option<u64> {
    match row.as_ref(i)? {
        Value::Int(n) => u64::try_from(*n).ok(),
        Value::UInt(n) => Some(*n),
        Value::Bytes(b) => std::str::from_utf8(b).ok()?.trim().parse().ok(),
        _ => None,
    }
}

fn named(row: &Row, name: &str) -> Option<usize> {
    row.columns_ref()
        .iter()
        .position(|c| c.name_str().eq_ignore_ascii_case(name))
}

fn on_off(text: Option<String>) -> bool {
    matches!(text.as_deref(), Some("1" | "ON" | "on"))
}

fn require_replica_commit_order(is_replica: bool, preserve: Option<bool>) -> Result<()> {
    if is_replica && preserve != Some(true) {
        return Err(SourceError::config(
            "replica_preserve_commit_order_required",
        ));
    }
    Ok(())
}

fn raw_row_bytes(row: &Row) -> u64 {
    (0..row.len())
        .map(|i| match row.as_ref(i) {
            Some(Value::Bytes(b)) => b.len() as u64,
            _ => 8,
        })
        .sum()
}

/// Canonical IPC stream of `batches` and its sha256.
fn ipc_digest(batches: &[RecordBatch]) -> Result<(u64, String)> {
    let Some(first) = batches.first() else {
        return Err(SourceError::protocol("arrow_empty"));
    };
    let mut writer = arrow_ipc::writer::StreamWriter::try_new(Vec::new(), &first.schema())
        .map_err(|_| SourceError::protocol("arrow_ipc"))?;
    for batch in batches {
        writer
            .write(batch)
            .map_err(|_| SourceError::protocol("arrow_ipc"))?;
    }
    writer
        .finish()
        .map_err(|_| SourceError::protocol("arrow_ipc"))?;
    let bytes = writer
        .into_inner()
        .map_err(|_| SourceError::protocol("arrow_ipc"))?;
    Ok((bytes.len() as u64, format!("{:x}", Sha256::digest(&bytes))))
}

impl MysqlEngine {
    /// Select file cursors before a repair when the durable consumer uses that
    /// position family. This does not change server GTID settings.
    pub async fn use_file_positions(&mut self) -> Result<()> {
        self.ensure_basics().await?;
        let basics = self.basics.as_mut().expect("basics");
        basics.watermark = file_position_mode(basics.watermark)?;
        Ok(())
    }

    /// Read the live instance identity, including on C5-only sources.
    pub async fn source_epoch(&mut self) -> Result<String> {
        let rows = self.exec("SELECT @@GLOBAL.server_uuid", vec![]).await?;
        rows.first()
            .and_then(|row| row_text(row, 0))
            .filter(|epoch| !epoch.is_empty())
            .ok_or_else(|| SourceError::protocol("server_uuid"))
    }

    /// No connection is opened until the first operation.
    pub fn new(config: MysqlConfig, password: Zeroizing<String>, options: EngineOptions) -> Self {
        let duty = DutyCycle::new(options.budget.duty_cycle_percent);
        Self {
            config,
            password,
            options,
            conn: None,
            basics: None,
            tables: HashMap::new(),
            duty,
            stats: ProtectionStats::default(),
            first_op: None,
            last_op: None,
            idle_since: None,
            next_split_id: 0,
        }
    }

    pub fn options(&self) -> &EngineOptions {
        &self.options
    }

    /// Resource receipt (T8). `peak_connections` counts this engine's own
    /// connections; it never holds two.
    pub fn stats(&self) -> ProtectionStats {
        self.stats.clone()
    }

    /// Wall time from the first to the last source operation.
    pub fn wall(&self) -> Duration {
        match (self.first_op, self.last_op) {
            (Some(a), Some(b)) => b.saturating_duration_since(a),
            _ => Duration::ZERO,
        }
    }

    pub fn watermark_mode(&self) -> Option<WatermarkMode> {
        self.basics.as_ref().map(|b| b.watermark)
    }

    /// Server connection id, for audits.
    pub fn connection_id(&self) -> Option<u32> {
        self.conn.as_ref().map(|c| c.id())
    }

    pub async fn close(&mut self) {
        self.rest().await;
        self.last_op = Some(Instant::now());
        if let Some(conn) = self.conn.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), conn.disconnect()).await;
        }
    }

    fn statement_timeout(&self) -> Duration {
        Duration::from_millis(self.options.budget.statement_timeout_ms.clamp(1, 600_000))
    }

    fn hint_ms(&self) -> Option<u64> {
        self.basics
            .as_ref()
            .filter(|b| b.dialect.server_select_timeout)
            .map(|_| self.options.budget.statement_timeout_ms.clamp(1, 600_000))
    }

    async fn ensure_conn(&mut self) -> Result<()> {
        if self.conn.is_some() {
            return Ok(());
        }
        if !mysql_ident(&self.config.mysql_database) {
            return Err(SourceError::config("database_ident"));
        }
        // A missing SELECT grant must reach probe(), not fail at handshake
        // with an implicit USE of the business schema. Reads are qualified.
        let mut login = self.config.clone();
        login.mysql_database = "information_schema".into();
        let mut conn = super::connect(&login, &self.password)
            .await
            .map_err(mysql_err)?;
        self.stats.connects += 1;
        self.stats.peak_connections = self.stats.peak_connections.max(1);
        let net = self.options.protection.net_timeout_s.clamp(30, 86_400);
        let mut setup = vec![
            format!("SET SESSION net_read_timeout = {net}, net_write_timeout = {net}"),
            "SET SESSION time_zone = '+00:00'".to_owned(),
            "SET SESSION TRANSACTION ISOLATION LEVEL REPEATABLE READ".to_owned(),
            "SET SESSION TRANSACTION READ ONLY".to_owned(),
        ];
        // max_execution_time exists from 5.7.8; the version is unknown on the
        // very first connect, so probe the variable before setting it.
        let has_met: Option<Row> = tokio::time::timeout(
            self.statement_timeout(),
            conn.query_first("SELECT @@SESSION.max_execution_time"),
        )
        .await
        .map_err(|_| SourceError::budget("session_setup_timeout"))?
        .map_err(|_| SourceError::protocol("session_timeout_variable"))?;
        self.stats.statements += 1;
        if has_met.is_some() {
            setup.push(format!(
                "SET SESSION max_execution_time = {}",
                self.options.budget.statement_timeout_ms.clamp(1, 600_000)
            ));
        }
        for sql in setup {
            keyset::guard(&sql)?;
            self.stats.statements += 1;
            tokio::time::timeout(Duration::from_secs(30), conn.query_drop(&sql))
                .await
                .map_err(|_| SourceError::budget("session_setup_timeout"))?
                .map_err(|_| SourceError::protocol("session_setup"))?;
        }
        self.conn = Some(conn);
        Ok(())
    }

    async fn drop_conn(&mut self) {
        if let Some(conn) = self.conn.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), conn.disconnect()).await;
        }
    }

    fn mark_op(&mut self) {
        let now = Instant::now();
        if self.first_op.is_none() {
            self.first_op = Some(now);
            self.stats.first_op_unix_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
        }
        self.last_op = Some(now);
    }

    /// Guarded statement with server and client timeouts. A client timeout
    /// or I/O failure discards the connection; it is never reused mid-read.
    async fn exec(&mut self, sql: &str, values: Vec<Value>) -> Result<Vec<Row>> {
        keyset::guard(sql)?;
        self.ensure_conn().await?;
        self.stats.statements += 1;
        let limit = self.statement_timeout() + self.options.protection.client_grace;
        let conn = self.conn.as_mut().expect("connected");
        let out = if values.is_empty() && !sql.starts_with("SELECT") && !sql.starts_with("EXPLAIN")
        {
            tokio::time::timeout(limit, conn.query::<Row, _>(sql)).await
        } else {
            tokio::time::timeout(limit, conn.exec::<Row, _, _>(sql, params(values))).await
        };
        match out {
            Ok(Ok(rows)) => Ok(rows),
            Ok(Err(e)) => self.fail(exec_err(&e)).await,
            Err(_) => self.fail(ExecError::Timeout).await,
        }
    }

    async fn fail<T>(&mut self, e: ExecError) -> Result<T> {
        Err(match e {
            ExecError::Timeout => {
                self.stats.client_timeouts += 1;
                self.drop_conn().await;
                SourceError::budget("client_statement_timeout")
            }
            ExecError::ServerTimeout => SourceError::budget("server_statement_timeout"),
            ExecError::Server(1142 | 1227 | 1044 | 1045 | 1143 | 1370) => {
                SourceError::privilege("statement_denied")
            }
            ExecError::Server(_) => SourceError::protocol("statement_failed"),
            ExecError::Io => {
                self.drop_conn().await;
                SourceError::connect("connection_lost")
            }
        })
    }

    /// Stream a page, stopping early once raw bytes exceed `max_bytes`.
    async fn exec_page(
        &mut self,
        sql: &str,
        values: Vec<Value>,
        max_bytes: u64,
    ) -> Result<(Vec<Row>, u64, bool)> {
        keyset::guard(sql)?;
        self.ensure_conn().await?;
        self.stats.statements += 1;
        let limit = self.statement_timeout() + self.options.protection.client_grace;
        let conn = self.conn.as_mut().expect("connected");
        let work = async {
            let mut result = conn.exec_iter(sql, params(values)).await?;
            let mut rows = Vec::new();
            let mut bytes = 0u64;
            let mut over = false;
            while let Some(row) = result.next().await? {
                bytes = bytes.saturating_add(raw_row_bytes(&row));
                rows.push(row);
                if bytes > max_bytes {
                    over = true;
                    break;
                }
            }
            result.drop_result().await?;
            Ok::<_, mysql_async::Error>((rows, bytes, over))
        };
        match tokio::time::timeout(limit, work).await {
            Ok(Ok(out)) => Ok(out),
            Ok(Err(e)) => self.fail(exec_err(&e)).await,
            Err(_) => self.fail(ExecError::Timeout).await,
        }
    }

    async fn ensure_basics(&mut self) -> Result<()> {
        if self.basics.is_some() {
            return Ok(());
        }
        let rows = self
            .exec(
                "SELECT VERSION(), @@GLOBAL.log_bin, @@GLOBAL.gtid_mode, @@GLOBAL.max_connections",
                vec![],
            )
            .await?;
        let row = rows
            .first()
            .ok_or_else(|| SourceError::protocol("version"))?;
        let raw = row_text(row, 0).ok_or_else(|| SourceError::version("version_text"))?;
        let version = EngineVersion::parse(EngineFamily::Mysql, &raw)?;
        let dialect = Dialect::for_version(&version)?;
        let log_bin = on_off(row_text(row, 1));
        let gtid_on = row_text(row, 2).is_some_and(|m| m.eq_ignore_ascii_case("ON"));
        let watermark = if !log_bin {
            WatermarkMode::None
        } else if gtid_on {
            WatermarkMode::GtidSet
        } else {
            WatermarkMode::FilePos
        };
        self.basics = Some(Basics {
            version,
            dialect,
            watermark,
            max_connections: row_u64(row, 3),
        });
        Ok(())
    }

    /// Duty-cycle rest, then pause while the source is loaded.
    async fn gate(&mut self) -> Result<()> {
        self.mark_op();
        self.rest().await;
        let mut paused = Duration::ZERO;
        loop {
            let rows = self
                .exec(
                    "SHOW GLOBAL STATUS WHERE Variable_name IN ('Threads_running', 'Threads_connected')",
                    vec![],
                )
                .await?;
            let mut running = None;
            let mut connected = None;
            for row in &rows {
                match row_text(row, 0).as_deref() {
                    Some("Threads_running") => running = row_u64(row, 1),
                    Some("Threads_connected") => connected = row_u64(row, 1),
                    _ => {}
                }
            }
            if let Some(n) = running {
                self.stats.max_threads_running = self.stats.max_threads_running.max(n);
            }
            let max = self.basics.as_ref().and_then(|b| b.max_connections);
            if load_verdict(&self.options.protection, running, connected, max).is_none() {
                return Ok(());
            }
            if paused >= self.options.protection.max_pause {
                return Err(SourceError::budget("source_busy"));
            }
            let step = self.options.protection.pause_step;
            tokio::time::sleep(step).await;
            paused += step;
            self.stats.pauses += 1;
            self.stats.pause_ms += step.as_millis() as u64;
        }
    }

    async fn rest(&mut self) {
        let wait = self.duty.wait(Instant::now());
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
            self.stats.sleep_ms += wait.as_millis() as u64;
        }
        if let Some(since) = self.idle_since.take() {
            self.stats.rest_ms += since.elapsed().as_millis() as u64;
        }
    }

    fn finish_op(&mut self, started: Instant) {
        let now = Instant::now();
        let busy = now.saturating_duration_since(started);
        self.stats.busy_ms += busy.as_millis() as u64;
        self.duty
            .set_percent(self.options.budget.duty_cycle_percent);
        self.duty.record(busy, now);
        self.last_op = Some(now);
        self.idle_since = Some(now);
    }

    async fn read_watermark(&mut self) -> Result<Option<Watermark>> {
        self.ensure_basics().await?;
        if self.basics.as_ref().expect("basics").watermark != WatermarkMode::None {
            if let Err(error) = self.check_replica_commit_order().await {
                if error.kind() == SourceErrorKind::Privilege {
                    // Snapshot/C5 remains usable, but unknown topology cannot
                    // authorize a log-corrected read.
                    return Ok(None);
                }
                return Err(error);
            }
        }
        let basics = self.basics.as_ref().expect("basics");
        match basics.watermark {
            WatermarkMode::None => Ok(None),
            WatermarkMode::GtidSet => {
                let rows = self.exec("SELECT @@GLOBAL.gtid_executed", vec![]).await?;
                let text = rows
                    .first()
                    .and_then(|r| row_text(r, 0))
                    .unwrap_or_default();
                let set = GtidSet::parse(&text)?;
                Ok(Some(Watermark::MysqlGtidSet {
                    gtid_set: set.to_string(),
                }))
            }
            WatermarkMode::FilePos => {
                let sql = basics.dialect.status_statement;
                match self.exec(sql, vec![]).await {
                    Ok(rows) => {
                        let Some(row) = rows.first() else {
                            return Ok(None);
                        };
                        let file = named(row, "File").and_then(|i| row_text(row, i));
                        let pos = named(row, "Position").and_then(|i| row_u64(row, i));
                        Ok(file
                            .zip(pos)
                            .map(|(file, position)| Watermark::MysqlFilePos { file, position }))
                    }
                    Err(e) if e.kind() == SourceErrorKind::Privilege => {
                        // No REPLICATION CLIENT: reads go on without watermarks;
                        // probe() reports the missing grant.
                        if let Some(b) = self.basics.as_mut() {
                            b.watermark = WatermarkMode::None;
                        }
                        Ok(None)
                    }
                    Err(e) => Err(e),
                }
            }
        }
    }

    async fn check_replica_commit_order(&mut self) -> Result<()> {
        let (replica, preserve) = self.replica_commit_order().await?;
        require_replica_commit_order(replica, preserve)
    }

    async fn replica_commit_order(&mut self) -> Result<(bool, Option<bool>)> {
        let sql = self.basics.as_ref().expect("basics").dialect.replica_status;
        // Inspect channels on every boundary: a cached primary classification
        // would miss a topology change during a long full read.
        if self.exec(sql, vec![]).await?.is_empty() {
            return Ok((false, None));
        }
        let rows = self
            .exec(
                "SHOW GLOBAL VARIABLES WHERE Variable_name IN ('replica_preserve_commit_order', 'slave_preserve_commit_order')",
                vec![],
            )
            .await?;
        let preserve = (!rows.is_empty()).then(|| rows.iter().all(|row| on_off(row_text(row, 1))));
        Ok((true, preserve))
    }

    /// Actual first file in the server's binlog index, not a retention setting.
    ///
    /// This is a file cursor even on GTID servers. It does not measure retained
    /// history in seconds; that requires reading the log's event timestamps.
    /// A transport must still detect a purge racing its subsequent subscription.
    pub async fn earliest_binlog(&mut self) -> Result<Watermark> {
        self.ensure_basics().await?;
        self.gate().await?;
        let started = Instant::now();
        let rows = self.exec("SHOW BINARY LOGS", vec![]).await;
        self.finish_op(started);
        let rows = rows?;
        let row = rows
            .first()
            .ok_or_else(|| SourceError::protocol("binlog_unavailable_rebaseline"))?;
        let file = row_text(row, 0)
            .filter(|s| !s.is_empty() && !s.contains('\0'))
            .ok_or_else(|| SourceError::protocol("binlog_inventory"))?;
        if row_u64(row, 1).is_none_or(|size| size < 4) {
            return Err(SourceError::protocol("binlog_inventory"));
        }
        Ok(Watermark::MysqlFilePos { file, position: 4 })
    }

    async fn meta(&mut self, table: &TableRef) -> Result<()> {
        if self.tables.contains_key(table) {
            return Ok(());
        }
        if !mysql_ident(&table.schema) || !mysql_ident(&table.name) {
            return Err(SourceError::identifier("table_ident"));
        }
        let engine = self
            .exec(
                "SELECT ENGINE FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND TABLE_TYPE = 'BASE TABLE'",
                vec![Value::from(table.schema.as_str()), Value::from(table.name.as_str())],
            )
            .await?;
        if engine.first().and_then(|r| row_text(r, 0)).as_deref() != Some("InnoDB") {
            return Err(SourceError::catalog("snapshot_requires_innodb"));
        }
        let rows = self
            .exec(
                keyset::KEY_SHAPE_SQL,
                vec![
                    Value::from(table.schema.as_str()),
                    Value::from(table.name.as_str()),
                ],
            )
            .await?;
        let index_rows: Vec<IndexColumn> = rows
            .iter()
            .map(|r| IndexColumn {
                index: row_text(r, 0).unwrap_or_default(),
                seq: row_u64(r, 1).unwrap_or(0) as u32,
                column: row_text(r, 2).unwrap_or_default(),
                non_unique: row_u64(r, 3).unwrap_or(1) != 0,
                sub_part: row_u64(r, 4).unwrap_or(1) != 0,
                nullable: row_u64(r, 5).unwrap_or(1) != 0,
                data_type: row_text(r, 6).unwrap_or_default(),
                column_type: row_text(r, 7).unwrap_or_default(),
                collation: row_text(r, 8),
            })
            .collect();
        let key = keyset::choose_key(&index_rows);
        self.ensure_conn().await?;
        self.stats.statements += 1;
        let mut notes = Vec::new();
        let selected = SelectedTable {
            schema: table.schema.clone(),
            name: table.name.clone(),
            columns: Vec::new(),
        };
        let conn = self.conn.as_mut().expect("connected");
        let catalog = super::catalog_columns(conn, &mut notes, &selected)
            .await
            .map_err(mysql_err)?;
        let key_names: BTreeSet<&str> = key.columns.iter().map(|c| c.name.as_str()).collect();
        let mut columns = Vec::with_capacity(catalog.len());
        for column in &catalog {
            let mut mapped =
                map_capture_column(column, false, true, &Default::default()).map_err(mysql_err)?;
            let is_key = key_names.contains(column.name.as_str());
            mapped.key = is_key;
            mapped.plan.key = is_key;
            columns.push(mapped);
        }
        let names: Vec<String> = columns.iter().map(|c| c.plan.name.clone()).collect();
        let key_positions = key
            .columns
            .iter()
            .map(|k| {
                names
                    .iter()
                    .position(|n| *n == k.name)
                    .ok_or_else(|| SourceError::catalog("key_column_missing"))
            })
            .collect::<Result<Vec<_>>>()?;
        let select_list = columns
            .iter()
            .map(capture_expression)
            .collect::<Vec<_>>()
            .join(", ");
        let binding = RunBinding {
            source_id: self.options.source_id.clone(),
            schema: table.schema.clone(),
            table: table.name.clone(),
            run_generation: 0,
            input_digest: Sha256::digest(format!("{table}").as_bytes()).into(),
        };
        self.tables.insert(
            table.clone(),
            TableMeta {
                key,
                columns,
                names,
                select_list,
                binding,
                key_positions,
            },
        );
        Ok(())
    }

    fn meta_ref(&self, table: &TableRef) -> Result<&TableMeta> {
        self.tables
            .get(table)
            .ok_or_else(|| SourceError::catalog("table_not_planned"))
    }

    fn key_values(meta: &TableMeta, row: &Row) -> Result<Vec<Value>> {
        meta.key_positions
            .iter()
            .map(|&i| row_value(row, i))
            .collect()
    }

    fn key_of(cols: &[KeyCol], values: &[Value]) -> Result<Vec<KeyValue>> {
        cols.iter()
            .zip(values)
            .map(|(c, v)| key_value(c.kind, v))
            .collect()
    }

    fn bound(values: Option<&[KeyValue]>, keys: &[KeyCol]) -> Result<Option<Vec<Value>>> {
        values
            .map(|vs| {
                if vs.len() != keys.len() {
                    return Err(SourceError::config("chunk_bound_width"));
                }
                if vs.iter().zip(keys).any(|(v, k)| {
                    !matches!(
                        (k.kind, v),
                        (KeyKind::Int, KeyValue::Int(_))
                            | (KeyKind::Decimal, KeyValue::Decimal(_))
                            | (KeyKind::Text, KeyValue::Text(_) | KeyValue::Bytes(_))
                            | (KeyKind::Bytes, KeyValue::Bytes(_))
                    )
                }) {
                    return Err(SourceError::config("chunk_bound_type"));
                }
                vs.iter().map(key_param).collect()
            })
            .transpose()
    }

    /// Optimizer row estimate.
    async fn estimate_rows(&mut self, table: &TableRef) -> Result<u64> {
        let sql = keyset::explain_rows_sql(&table.schema, &table.name)?;
        let rows = self.exec(&sql, vec![]).await?;
        Ok(rows
            .iter()
            .filter_map(|r| named(r, "rows").and_then(|i| row_u64(r, i)))
            .max()
            .unwrap_or(0))
    }

    /// `EXPLAIN` the statement and require the key index, not a full scan.
    async fn verify_index(&mut self, sql: &str, values: Vec<Value>, index: &str) -> Result<()> {
        let rows = self
            .exec(&format!("EXPLAIN FORMAT=TRADITIONAL {sql}"), values)
            .await?;
        self.stats.explain_checks += 1;
        let ok = rows.iter().any(|r| {
            let key = named(r, "key").and_then(|i| row_text(r, i));
            let kind = named(r, "type").and_then(|i| row_text(r, i));
            key.as_deref() == Some(index)
                && matches!(
                    kind.as_deref(),
                    Some("range" | "index" | "ref" | "eq_ref" | "const")
                )
        });
        if ok {
            Ok(())
        } else {
            Err(SourceError::catalog("chunk_index_unused"))
        }
    }

    /// Next boundary at `offset` rows from an inclusive `lo`, inside `hi`.
    async fn probe_boundary(
        &mut self,
        table: &TableRef,
        lo: Option<&[Value]>,
        hi: Option<&[Value]>,
        offset: u64,
    ) -> Result<Option<Vec<Value>>> {
        let meta = self.meta_ref(table)?;
        let keys = meta.key.columns.clone();
        let index = meta.key.index.clone();
        let (pred, values) = keyset::range_predicate(&keys, lo.map(|v| (v, Lower::Inclusive)), hi)?;
        let sql = keyset::boundary_sql(
            &table.schema,
            &table.name,
            &index,
            &keys,
            &pred,
            offset,
            self.hint_ms(),
        )?;
        let rows = self.exec(&sql, values).await?;
        rows.first()
            .map(|r| (0..keys.len()).map(|i| row_value(r, i)).collect())
            .transpose()
    }

    /// Boundaries every `chunk_rows` keys inside `[lo, hi)` by probing.
    async fn probe_boundaries(
        &mut self,
        table: &TableRef,
        lo: Option<Vec<Value>>,
        hi: Option<Vec<Value>>,
        chunk_rows: u64,
    ) -> Result<Vec<Vec<Value>>> {
        let mut out = Vec::new();
        let mut cursor = lo;
        loop {
            self.gate().await?;
            let started = Instant::now();
            let next = self
                .probe_boundary(table, cursor.as_deref(), hi.as_deref(), chunk_rows)
                .await;
            self.finish_op(started);
            match next? {
                Some(key) => {
                    out.push(key.clone());
                    cursor = Some(key);
                }
                None => return Ok(out),
            }
            if out.len() > 10_000_000 {
                return Err(SourceError::budget("chunk_count"));
            }
        }
    }

    /// Boundaries from an ordered key stream sampled at one in `rate`.
    async fn sample_boundaries(
        &mut self,
        table: &TableRef,
        chunk_rows: u64,
    ) -> Result<Vec<Vec<Value>>> {
        let rate = self.options.sample_rate.max(1);
        let page = 1000;
        let mut after: Option<Vec<Value>> = None;
        let mut samples = Vec::new();
        loop {
            self.gate().await?;
            let started = Instant::now();
            let meta = self.meta_ref(table)?;
            let keys = meta.key.columns.clone();
            let index = meta.key.index.clone();
            let (mut pred, values) = keyset::range_predicate(
                &keys,
                after.as_deref().map(|v| (v, Lower::Exclusive)),
                None,
            )?;
            let sample = keyset::sample_predicate(&keys, rate)?;
            pred.push_str(if pred.is_empty() { " WHERE " } else { " AND " });
            pred.push_str(&sample);
            let sql = keyset::key_page_sql(
                &table.schema,
                &table.name,
                &index,
                &keys,
                &pred,
                page,
                self.hint_ms(),
            )?;
            let rows = self.exec(&sql, values).await;
            self.finish_op(started);
            let rows = rows?;
            let n = rows.len() as u64;
            for row in &rows {
                samples.push(
                    (0..keys.len())
                        .map(|i| row_value(row, i))
                        .collect::<Result<Vec<_>>>()?,
                );
            }
            if n < page {
                break;
            }
            after = rows
                .last()
                .map(|r| (0..keys.len()).map(|i| row_value(r, i)).collect())
                .transpose()?;
        }
        Ok(keyset::sample_boundaries(&samples, rate, chunk_rows))
    }

    fn chunks_from(
        table: &TableRef,
        revision: u32,
        first_id: u32,
        lo: Option<Vec<KeyValue>>,
        hi: Option<Vec<KeyValue>>,
        boundaries: Vec<Vec<KeyValue>>,
        rows_each: Option<u64>,
    ) -> Vec<ChunkSpec> {
        let mut out = Vec::with_capacity(boundaries.len() + 1);
        let mut cursor = lo;
        for (i, b) in boundaries.into_iter().enumerate() {
            out.push(ChunkSpec {
                table: table.clone(),
                plan_revision: revision,
                chunk_id: first_id + i as u32,
                lo: cursor.take(),
                hi: Some(b.clone()),
                estimated_rows: rows_each,
            });
            cursor = Some(b);
        }
        out.push(ChunkSpec {
            table: table.clone(),
            plan_revision: revision,
            chunk_id: first_id + out.len() as u32,
            lo: cursor,
            hi,
            estimated_rows: None,
        });
        out
    }

    /// The page statement of a chunk with its parameters.
    fn page_statement(
        &self,
        table: &TableRef,
        lo: Option<(&[Value], Lower)>,
        hi: Option<&[Value]>,
        limit: u64,
    ) -> Result<(String, Vec<Value>)> {
        let meta = self.meta_ref(table)?;
        let (pred, values) = keyset::range_predicate(&meta.key.columns, lo, hi)?;
        let sql = keyset::page_sql(&PageSql {
            schema: &table.schema,
            table: &table.name,
            index: &meta.key.index,
            select_list: &meta.select_list,
            keys: &meta.key.columns,
            predicate: &pred,
            limit,
            timeout_ms: self.hint_ms(),
        })?;
        Ok((sql, values))
    }

    async fn plan_inner(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<TableChunkPlan> {
        self.ensure_basics().await?;
        self.gate().await?;
        let started = Instant::now();
        self.meta(table).await?;
        let rows_est = self.estimate_rows(table).await?;
        let names = self.meta_ref(table)?.names.clone();
        let sql = keyset::row_bytes_sql(&table.schema, &table.name, &names, self.hint_ms())?;
        let sized = self.exec(&sql, vec![]).await?;
        let avg = sized.first().and_then(|r| {
            let n = row_u64(r, 0)?;
            let sum = row_u64(r, 1)?;
            (n > 0).then(|| sum.div_ceil(n))
        });
        let chunk_rows = keyset::chunk_rows_for(budget, avg);
        let meta = self.meta_ref(table)?;
        let shape = meta.key.shape.clone();
        let keys = meta.key.columns.clone();
        let index = meta.key.index.clone();
        let strategy = if shape == KeyShape::NoKey {
            ChunkStrategy::SingleRead
        } else {
            ChunkStrategy::Keyset {
                columns: keys.iter().map(|k| k.name.clone()).collect(),
            }
        };
        let mut plan = TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table: table.clone(),
            key: shape.clone(),
            strategy,
            estimated_rows: Some(rows_est),
            chunks: Vec::new(),
        };
        if shape == KeyShape::NoKey {
            self.finish_op(started);
            plan.chunks = Self::chunks_from(table, 0, 0, None, None, Vec::new(), None);
            plan.chunks[0].estimated_rows = Some(rows_est);
            return Ok(plan);
        }
        let min_max = if matches!(shape, KeyShape::IntegerPrimaryKey { .. })
            || (keys.len() == 1 && keys[0].kind == KeyKind::Int)
        {
            let sql = keyset::min_max_sql(&table.schema, &table.name, &keys[0], self.hint_ms())?;
            let rows = self.exec(&sql, vec![]).await?;
            rows.first().and_then(|r| {
                let lo = key_value(KeyKind::Int, &row_value(r, 0).ok()?).ok()?;
                let hi = key_value(KeyKind::Int, &row_value(r, 1).ok()?).ok()?;
                match (lo, hi) {
                    (KeyValue::Int(a), KeyValue::Int(b)) => Some((a, b)),
                    _ => None,
                }
            })
        } else {
            None
        };
        self.finish_op(started);
        let split = keyset::choose_split(
            rows_est,
            chunk_rows,
            min_max,
            self.options.sample_threshold_chunks,
        );
        plan.chunks = match split {
            Split::Empty => Self::chunks_from(table, 0, 0, None, None, Vec::new(), None),
            Split::Step { step } => {
                let (min, max) = min_max.expect("step needs min/max");
                keyset::step_bounds(min, max, step)
                    .into_iter()
                    .enumerate()
                    .map(|(i, (lo, hi))| ChunkSpec {
                        table: table.clone(),
                        plan_revision: 0,
                        chunk_id: i as u32,
                        lo: lo.map(|v| vec![KeyValue::Int(v)]),
                        hi: hi.map(|v| vec![KeyValue::Int(v)]),
                        estimated_rows: Some(chunk_rows),
                    })
                    .collect()
            }
            Split::Probe | Split::Sample => {
                let raw = if split == Split::Probe {
                    self.probe_boundaries(table, None, None, chunk_rows).await?
                } else {
                    self.sample_boundaries(table, chunk_rows).await?
                };
                let boundaries = raw
                    .iter()
                    .map(|b| Self::key_of(&keys, b))
                    .collect::<Result<Vec<_>>>()?;
                Self::chunks_from(table, 0, 0, None, None, boundaries, Some(chunk_rows))
            }
        };
        // EXPLAIN-verify the shape every chunk read uses: bounded on both sides.
        if let Some(c) = plan
            .chunks
            .iter()
            .find(|c| c.lo.is_some() && c.hi.is_some())
            .or(plan.chunks.first())
        {
            let lo = Self::bound(c.lo.as_deref(), &keys)?;
            let hi = Self::bound(c.hi.as_deref(), &keys)?;
            let (sql, values) = self.page_statement(
                table,
                lo.as_deref().map(|v| (v, Lower::Inclusive)),
                hi.as_deref(),
                1,
            )?;
            self.verify_index(&sql, values, &index).await?;
        }
        Ok(plan)
    }

    /// Read one chunk; returns the rows and the receipt. On budget overflow
    /// the chunk fails with `chunk_over_budget` and nothing reaches the sink.
    async fn read_inner(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
    ) -> Result<(ChunkReceipt, Vec<RecordBatch>, Vec<RowFingerprint>)> {
        self.apply_budget(budget).await?;
        self.ensure_basics().await?;
        self.meta(&chunk.table).await?;
        self.gate().await?;
        let started = Instant::now();
        let result = self.read_snapshot(chunk, budget).await;
        self.finish_op(started);
        let (low, high, rows, bytes) = result?;
        self.stats.chunks_read += 1;
        let meta = self.meta_ref(&chunk.table)?;
        let mut batches = Vec::new();
        let mut prints = Vec::with_capacity(rows.len());
        let keys = meta.key.columns.clone();
        let mut group: Vec<Row> = Vec::new();
        let mut group_bytes = 0u64;
        let mut flush = |group: &mut Vec<Row>, group_bytes: &mut u64| -> Result<()> {
            let taken = std::mem::take(group);
            let key_vals: Vec<Option<Vec<KeyValue>>> = taken
                .iter()
                .map(|r| {
                    if keys.is_empty() {
                        Ok(None)
                    } else {
                        Self::key_of(&keys, &Self::key_values(meta, r)?).map(Some)
                    }
                })
                .collect::<Result<_>>()?;
            let batch = super::reader::encode_chunk_batch(
                &meta.columns,
                &meta.binding,
                taken,
                *group_bytes,
                "chunk",
            )
            .map_err(mysql_err)?;
            if !batch.rejects.is_empty() {
                return Err(SourceError::protocol("chunk_conversion_reject"));
            }
            for (typed, key) in batch.typed.rows.iter().zip(key_vals) {
                prints.push(RowFingerprint {
                    key: key.unwrap_or_default(),
                    canonical_hash: typed.canonical_hash,
                });
            }
            batches.push(batch.typed.record_batch()?);
            *group_bytes = 0;
            Ok(())
        };
        let mut flushed = 0usize;
        for row in rows {
            let b = raw_row_bytes(&row);
            if !group.is_empty() && (group_bytes + b > BATCH_RAW_BYTES || group.len() >= BATCH_ROWS)
            {
                flush(&mut group, &mut group_bytes)?;
                flushed += 1;
            }
            group_bytes += b;
            group.push(row);
        }
        if !group.is_empty() || flushed == 0 {
            flush(&mut group, &mut group_bytes)?;
        }
        let _ = bytes;
        let (ipc_bytes, sha256) = ipc_digest(&batches)?;
        if ipc_bytes > budget.max_chunk_bytes {
            self.stats.chunks_over_budget += 1;
            return Err(SourceError::budget("chunk_ipc_over_budget"));
        }
        let receipt = ChunkReceipt {
            table: chunk.table.clone(),
            plan_revision: chunk.plan_revision,
            chunk_id: chunk.chunk_id,
            rows: prints.len() as u64,
            bytes: ipc_bytes,
            sha256,
            low,
            high,
        };
        Ok((receipt, batches, prints))
    }

    async fn read_snapshot(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
    ) -> Result<(Option<Watermark>, Option<Watermark>, Vec<Row>, u64)> {
        let low = self.read_watermark().await?;
        self.exec(
            "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
            vec![],
        )
        .await?;
        let body = async {
            let (rows, bytes) = self.read_pages(chunk, budget).await?;
            Ok::<_, SourceError>((rows, bytes))
        };
        let result = body.await;
        let end = if result.is_ok() { "COMMIT" } else { "ROLLBACK" };
        if self.conn.is_some() {
            let closed = self.exec(end, vec![]).await;
            if result.is_ok() {
                closed?;
            }
        }
        let (rows, bytes) = result?;
        let high = self.read_watermark().await?;
        Ok((low, high, rows, bytes))
    }

    async fn read_pages(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
    ) -> Result<(Vec<Row>, u64)> {
        let max_rows = budget.max_chunk_rows.clamp(1, 100_000);
        let max_bytes = budget.max_chunk_bytes.clamp(1, 32 * 1024 * 1024);
        let meta = self.meta_ref(&chunk.table)?;
        let width = meta.key.columns.len();
        let index = meta.key.index.clone();
        if width == 0 {
            if chunk.lo.is_some() || chunk.hi.is_some() {
                return Err(SourceError::config("single_read_bounds"));
            }
            let sql = keyset::single_read_sql(
                &chunk.table.schema,
                &chunk.table.name,
                &meta.select_list,
                max_rows + 1,
                self.hint_ms(),
            )?;
            let (rows, bytes, over) = self.exec_page(&sql, vec![], max_bytes).await?;
            if over || rows.len() as u64 > max_rows {
                self.stats.chunks_over_budget += 1;
                return Err(SourceError::budget("single_read_over_budget"));
            }
            return Ok((rows, bytes));
        }
        let lo = Self::bound(chunk.lo.as_deref(), &meta.key.columns)?;
        let hi = Self::bound(chunk.hi.as_deref(), &meta.key.columns)?;
        let mut adaptive = AdaptiveRows::new(
            self.options.initial_page_rows.clamp(1, max_rows),
            max_rows,
            max_bytes,
        )
        .map_err(mysql_err)?;
        let mut out: Vec<Row> = Vec::new();
        let mut bytes = 0u64;
        let mut after: Option<Vec<Value>> = None;
        let mut first = true;
        loop {
            let remaining = max_rows - out.len() as u64;
            let limit = adaptive.rows().min(remaining + 1).max(1);
            let lower = match &after {
                Some(a) => Some((a.as_slice(), Lower::Exclusive)),
                None => lo.as_deref().map(|v| (v, Lower::Inclusive)),
            };
            let (sql, values) = self.page_statement(&chunk.table, lower, hi.as_deref(), limit)?;
            if first {
                self.verify_index(&sql, values.clone(), &index).await?;
                first = false;
            }
            let t = Instant::now();
            let (rows, page_bytes, over) = self
                .exec_page(&sql, values, max_bytes.saturating_sub(bytes))
                .await?;
            let n = rows.len() as u64;
            bytes += page_bytes;
            if over || out.len() as u64 + n > max_rows {
                self.stats.chunks_over_budget += 1;
                return Err(SourceError::budget("chunk_over_budget"));
            }
            adaptive.observe(n, page_bytes, t.elapsed());
            let meta = self.meta_ref(&chunk.table)?;
            after = match rows.last() {
                Some(r) => Some(Self::key_values(meta, r)?),
                None => None,
            };
            out.extend(rows);
            if n < limit {
                return Ok((out, bytes));
            }
        }
    }

    /// Chunked read that also returns each row's key and canonical hash.
    pub async fn read_chunk_with_rows(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<(ChunkReceipt, Vec<RowFingerprint>)> {
        let (receipt, batches, prints) = self.read_inner(chunk, budget).await?;
        for batch in batches {
            sink.push(batch)?;
        }
        Ok((receipt, prints))
    }

    /// Split a chunk that outgrew the budget into a new plan revision. The
    /// children cover exactly `[lo, hi)` of the parent and get consecutive
    /// ids from `first_id`.
    pub async fn split_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        first_id: u32,
    ) -> Result<Vec<ChunkSpec>> {
        self.apply_budget(budget).await?;
        self.ensure_basics().await?;
        self.meta(&chunk.table).await?;
        let meta = self.meta_ref(&chunk.table)?;
        let keys = meta.key.columns.clone();
        if keys.is_empty() {
            return Err(SourceError::budget("single_read_cannot_split"));
        }
        let names = meta.names.clone();
        self.gate().await?;
        let started = Instant::now();
        let sql = keyset::row_bytes_sql(
            &chunk.table.schema,
            &chunk.table.name,
            &names,
            self.hint_ms(),
        )?;
        let sized = self.exec(&sql, vec![]).await;
        self.finish_op(started);
        let avg = sized?.first().and_then(|r| {
            let n = row_u64(r, 0)?;
            let sum = row_u64(r, 1)?;
            (n > 0).then(|| sum.div_ceil(n))
        });
        let mut chunk_rows = keyset::chunk_rows_for(budget, avg).min(
            chunk
                .estimated_rows
                .unwrap_or(budget.max_chunk_rows)
                .div_ceil(2)
                .max(1),
        );
        let lo = Self::bound(chunk.lo.as_deref(), &keys)?;
        let hi = Self::bound(chunk.hi.as_deref(), &keys)?;
        let raw = loop {
            let raw = self
                .probe_boundaries(&chunk.table, lo.clone(), hi.clone(), chunk_rows)
                .await?;
            if !raw.is_empty() {
                break raw;
            }
            if chunk_rows == 1 {
                return Err(SourceError::budget("single_row_cannot_split"));
            }
            chunk_rows = (chunk_rows / 2).max(1);
        };
        let boundaries = raw
            .iter()
            .map(|b| Self::key_of(&keys, b))
            .collect::<Result<Vec<_>>>()?;
        if first_id.checked_add(boundaries.len() as u32).is_none() {
            return Err(SourceError::budget("chunk_id"));
        }
        Ok(Self::chunks_from(
            &chunk.table,
            chunk
                .plan_revision
                .checked_add(1)
                .ok_or_else(|| SourceError::budget("plan_revision"))?,
            first_id,
            chunk.lo.clone(),
            chunk.hi.clone(),
            boundaries,
            Some(chunk_rows),
        ))
    }

    /// Current rows for exact keys (row images for watermark correction).
    /// Missing keys are absent from the result.
    pub async fn read_keys(
        &mut self,
        table: &TableRef,
        keys: &[Vec<KeyValue>],
    ) -> Result<Vec<RowFingerprint>> {
        self.ensure_basics().await?;
        self.meta(table).await?;
        let mut out = Vec::new();
        for group in keys.chunks(POINT_KEYS) {
            self.gate().await?;
            let started = Instant::now();
            let meta = self.meta_ref(table)?;
            let cols = meta.key.columns.clone();
            if cols.is_empty() {
                return Err(SourceError::config("point_read_needs_key"));
            }
            let mut terms = Vec::new();
            let mut values = Vec::new();
            for key in group {
                if key.len() != cols.len() {
                    return Err(SourceError::config("chunk_bound_width"));
                }
                terms.push(format!(
                    "({})",
                    cols.iter()
                        .map(|c| format!("`{}` = ?", c.name))
                        .collect::<Vec<_>>()
                        .join(" AND ")
                ));
                values.extend(Self::bound(Some(key), &cols)?.unwrap_or_default());
            }
            let sql = format!(
                "SELECT {}{} FROM `{}`.`{}` FORCE INDEX (`{}`) WHERE {}",
                self.hint_ms()
                    .map(|ms| format!("/*+ MAX_EXECUTION_TIME({ms}) */ "))
                    .unwrap_or_default(),
                meta.select_list,
                table.schema,
                table.name,
                meta.key.index,
                terms.join(" OR ")
            );
            let rows = self.exec(&sql, values).await;
            self.finish_op(started);
            let rows = rows?;
            let meta = self.meta_ref(table)?;
            let bytes = rows.iter().map(raw_row_bytes).sum();
            let key_vals = rows
                .iter()
                .map(|r| Self::key_of(&cols, &Self::key_values(meta, r)?))
                .collect::<Result<Vec<_>>>()?;
            if rows.is_empty() {
                continue;
            }
            let batch = super::reader::encode_chunk_batch(
                &meta.columns,
                &meta.binding,
                rows,
                bytes,
                "point",
            )
            .map_err(mysql_err)?;
            if !batch.rejects.is_empty() {
                return Err(SourceError::protocol("chunk_conversion_reject"));
            }
            for (typed, key) in batch.typed.rows.iter().zip(key_vals) {
                out.push(RowFingerprint {
                    key,
                    canonical_hash: typed.canonical_hash,
                });
            }
        }
        Ok(out)
    }

    async fn apply_budget(&mut self, budget: &ReadBudget) -> Result<()> {
        if budget.max_chunk_rows == 0
            || budget.max_chunk_bytes == 0
            || !(1..=100).contains(&budget.duty_cycle_percent)
            || !(1..=600_000).contains(&budget.statement_timeout_ms)
        {
            return Err(SourceError::config("read_budget"));
        }
        self.options.budget = budget.clone();
        self.duty.set_percent(budget.duty_cycle_percent);
        if self.conn.is_some() {
            self.exec(
                &format!(
                    "SET SESSION max_execution_time = {}",
                    budget.statement_timeout_ms
                ),
                vec![],
            )
            .await?;
        }
        Ok(())
    }

    async fn probe_inner(&mut self) -> Result<ProbeReport> {
        self.ensure_basics().await?;
        self.gate().await?;
        let started = Instant::now();
        let result = self.observe().await;
        self.finish_op(started);
        let observed = result?;
        let basics = self.basics.as_ref().expect("basics");
        let mut report = capability::grade(&basics.version, &basics.dialect, &observed);
        report
            .settings
            .insert("server_version".into(), basics.version.raw.clone());
        Ok(report)
    }

    async fn observe(&mut self) -> Result<Observed> {
        let (status, retention_var, unit, auth_var, timeout_var) = {
            let d = &self.basics.as_ref().expect("basics").dialect;
            (
                d.status_statement,
                d.retention_variable,
                d.retention_unit_seconds,
                d.auth_variable,
                d.server_select_timeout,
            )
        };
        let _ = status;
        let rows = self
            .exec(
                "SELECT @@GLOBAL.log_bin, @@GLOBAL.binlog_format, @@GLOBAL.binlog_row_image, @@GLOBAL.gtid_mode, @@GLOBAL.server_id, @@GLOBAL.max_connections, CURRENT_USER()",
                vec![],
            )
            .await?;
        let row = rows
            .first()
            .ok_or_else(|| SourceError::protocol("settings"))?;
        let current_user = row_text(row, 6).unwrap_or_default();
        let mut o = Observed {
            database: self.config.mysql_database.clone(),
            account: capability::quote_account(&current_user),
            log_bin: on_off(row_text(row, 0)),
            binlog_format: row_text(row, 1),
            binlog_row_image: row_text(row, 2),
            gtid_mode: row_text(row, 3),
            server_id: row_u64(row, 4),
            max_connections: row_u64(row, 5),
            ..Observed::default()
        };
        let one = format!("SELECT @@GLOBAL.{retention_var}");
        o.retention_seconds = self
            .exec(&one, vec![])
            .await?
            .first()
            .and_then(|r| row_u64(r, 0))
            .map(|v| v.saturating_mul(unit));
        let auth = format!("SELECT @@GLOBAL.{auth_var}");
        o.default_auth_setting = self
            .exec(&auth, vec![])
            .await?
            .first()
            .and_then(|r| row_text(r, 0));
        if timeout_var {
            o.max_execution_time_ms = self
                .exec("SELECT @@SESSION.max_execution_time", vec![])
                .await?
                .first()
                .and_then(|r| row_u64(r, 0));
        }
        o.native_plugin_active = Some(
            self.exec(
                "SELECT PLUGIN_STATUS FROM information_schema.PLUGINS WHERE PLUGIN_NAME = 'mysql_native_password'",
                vec![],
            )
            .await?
            .first()
            .and_then(|r| row_text(r, 0))
            .is_some_and(|s| s.eq_ignore_ascii_case("ACTIVE")),
        );
        // Only the plugin name is kept; the statement also prints the
        // account's hash, which is wiped and never stored or logged.
        if let Ok(rows) = self.exec("SHOW CREATE USER CURRENT_USER()", vec![]).await
            && let Some(Value::Bytes(b)) = rows.first().and_then(|r| r.as_ref(0))
        {
            let text = Zeroizing::new(b.clone());
            let s = String::from_utf8_lossy(&text);
            o.account_plugin = s
                .split_once("IDENTIFIED WITH '")
                .and_then(|(_, rest)| rest.split_once('\''))
                .map(|(plugin, _)| plugin.to_owned())
                .filter(|p| p.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'));
        }
        let grants = self.exec("SHOW GRANTS FOR CURRENT_USER()", vec![]).await?;
        let lines: Vec<String> = grants.iter().filter_map(|r| row_text(r, 0)).collect();
        o.grants = Grants::parse(&lines, &self.config.mysql_database);
        if o.log_bin && o.grants.replication_client {
            (o.is_replica, o.replica_preserve_commit_order) = self.replica_commit_order().await?;
        }
        let status = self
            .exec(
                "SHOW GLOBAL STATUS WHERE Variable_name = 'Threads_connected'",
                vec![],
            )
            .await?;
        o.threads_connected = status.first().and_then(|r| row_u64(r, 1));
        Ok(o)
    }

    async fn digest_inner(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest> {
        self.ensure_basics().await?;
        self.meta(table).await?;
        let meta = self.meta_ref(table)?;
        let keys = meta.key.columns.clone();
        let index = meta.key.index.clone();
        let names = meta.names.clone();
        let piece_rows = self.options.budget.max_chunk_rows.clamp(1, 100_000);
        let mut rows_total = 0u64;
        let (mut x1, mut x2) = (0u64, 0u64);
        if keys.is_empty() {
            if lo.is_some() || hi.is_some() {
                return Err(SourceError::config("single_read_bounds"));
            }
            self.gate().await?;
            let started = Instant::now();
            let sql =
                keyset::digest_sql(&table.schema, &table.name, None, &names, "", self.hint_ms())?;
            let rows = self.exec(&sql, vec![]).await;
            self.finish_op(started);
            let rows = rows?;
            let r = rows
                .first()
                .ok_or_else(|| SourceError::protocol("digest"))?;
            rows_total = row_u64(r, 0).ok_or_else(|| SourceError::protocol("digest_count"))?;
            x1 = row_u64(r, 1).ok_or_else(|| SourceError::protocol("digest_sum"))?;
            x2 = row_u64(r, 2).ok_or_else(|| SourceError::protocol("digest_sum"))?;
        } else {
            let lo_v = Self::bound(lo, &keys)?;
            let hi_v = Self::bound(hi, &keys)?;
            let mut cursor = lo_v;
            loop {
                self.gate().await?;
                let started = Instant::now();
                let next = self
                    .probe_boundary(table, cursor.as_deref(), hi_v.as_deref(), piece_rows)
                    .await;
                let next = match next {
                    Ok(n) => n,
                    Err(e) => {
                        self.finish_op(started);
                        return Err(e);
                    }
                };
                let piece_hi = next.clone().or_else(|| hi_v.clone());
                let (pred, values) = keyset::range_predicate(
                    &keys,
                    cursor.as_deref().map(|v| (v, Lower::Inclusive)),
                    piece_hi.as_deref(),
                )?;
                let sql = keyset::digest_sql(
                    &table.schema,
                    &table.name,
                    Some(&index),
                    &names,
                    &pred,
                    self.hint_ms(),
                )?;
                let rows = self.exec(&sql, values).await;
                self.finish_op(started);
                let rows = rows?;
                let r = rows
                    .first()
                    .ok_or_else(|| SourceError::protocol("digest"))?;
                rows_total = rows_total
                    .checked_add(
                        row_u64(r, 0).ok_or_else(|| SourceError::protocol("digest_count"))?,
                    )
                    .ok_or_else(|| SourceError::budget("digest_count"))?;
                x1 = x1.wrapping_add(
                    row_u64(r, 1).ok_or_else(|| SourceError::protocol("digest_sum"))?,
                );
                x2 = x2.wrapping_add(
                    row_u64(r, 2).ok_or_else(|| SourceError::protocol("digest_sum"))?,
                );
                match next {
                    Some(n) => cursor = Some(n),
                    None => break,
                }
            }
        }
        Ok(RangeDigest {
            table: table.clone(),
            lo: lo.map(<[KeyValue]>::to_vec),
            hi: hi.map(<[KeyValue]>::to_vec),
            rows: rows_total,
            digest: format!("md5sum2:{x1:016x}{x2:016x}"),
        })
    }
}

impl SourceEngine for MysqlEngine {
    fn family(&self) -> EngineFamily {
        EngineFamily::Mysql
    }

    async fn probe(&mut self) -> Result<ProbeReport> {
        self.probe_inner().await
    }

    async fn plan_chunks(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<TableChunkPlan> {
        self.apply_budget(budget).await?;
        let plan = self.plan_inner(table, budget).await?;
        self.next_split_id = self.next_split_id.max(
            plan.chunks
                .iter()
                .map(|c| c.chunk_id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| SourceError::budget("chunk_id"))?,
        );
        Ok(plan)
    }

    async fn split_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
    ) -> Result<Option<Vec<ChunkSpec>>> {
        let children = MysqlEngine::split_chunk(self, chunk, budget, self.next_split_id).await?;
        self.next_split_id = children
            .last()
            .ok_or_else(|| SourceError::protocol("empty_split"))?
            .chunk_id
            .checked_add(1)
            .ok_or_else(|| SourceError::budget("chunk_id"))?;
        Ok(Some(children))
    }

    async fn read_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt> {
        let (receipt, batches, _) = self.read_inner(chunk, budget).await?;
        for batch in batches {
            sink.push(batch)?;
        }
        Ok(receipt)
    }

    async fn watermark(&mut self) -> Result<Option<Watermark>> {
        self.ensure_basics().await?;
        self.gate().await?;
        let started = Instant::now();
        let result = self.read_watermark().await;
        self.finish_op(started);
        result
    }

    async fn digest_range(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest> {
        self.digest_inner(table, lo, hi).await
    }
}

/// Version dispatch rows for `oo doctor`, keyed by note.
pub fn dispatch_table() -> BTreeMap<String, String> {
    capability::version_rules()
        .into_iter()
        .map(|r| {
            (
                r.note,
                format!(
                    "{:?} [{}.{}.{}, {})",
                    r.capability,
                    r.versions.from.0,
                    r.versions.from.1,
                    r.versions.from.2,
                    r.versions
                        .until
                        .map(|u| format!("{}.{}.{}", u.0, u.1, u.2))
                        .unwrap_or_else(|| "∞".into())
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replica_watermarks_require_known_commit_order() {
        assert!(require_replica_commit_order(false, None).is_ok());
        assert!(require_replica_commit_order(false, Some(false)).is_ok());
        assert!(require_replica_commit_order(true, Some(true)).is_ok());
        for observed in [None, Some(false)] {
            assert_eq!(
                require_replica_commit_order(true, observed)
                    .unwrap_err()
                    .fact(),
                Some("replica_preserve_commit_order_required")
            );
        }
    }

    #[tokio::test]
    async fn duty_receipt_counts_consumer_idle_without_claiming_sleep() {
        let config = MysqlConfig {
            mysql_host: "localhost".into(),
            mysql_port: 3306,
            mysql_database: "fixture".into(),
            mysql_user: "reader".into(),
            mysql_ca_file: Default::default(),
            mysql_tls_server_name: None,
            secret_ref: crate::SecretRef::new("secret://fixture").unwrap(),
            connect_timeout_ms: 1000,
        };
        let mut e = MysqlEngine::new(config, Zeroizing::new(String::new()), Default::default());
        let end = Instant::now() - Duration::from_millis(20);
        e.idle_since = Some(end);
        e.duty.record(Duration::from_millis(1), end);
        e.rest().await;
        assert!(e.stats.rest_ms >= 20);
        assert_eq!(e.stats.sleep_ms, 0);
        let before = e.stats.rest_ms;
        e.rest().await;
        assert_eq!(e.stats.rest_ms, before, "idle may only be counted once");
    }

    #[test]
    fn key_values_round_trip_through_driver_parameters() {
        for (kind, value) in [
            (KeyKind::Int, Value::Int(-5)),
            (KeyKind::Int, Value::UInt(u64::MAX)),
            (KeyKind::Text, Value::Bytes("Ä b ".as_bytes().to_vec())),
            (KeyKind::Text, Value::Bytes(vec![0xff, 0x00])),
            (KeyKind::Bytes, Value::Bytes(vec![0, 1, 254])),
            (
                KeyKind::Decimal,
                Value::Bytes(b"12345678901234567890".to_vec()),
            ),
        ] {
            let key = key_value(kind, &value).unwrap();
            let back = key_param(&key).unwrap();
            match (&value, &back) {
                (Value::Int(a), Value::Int(b)) => assert_eq!(a, b),
                (Value::UInt(a), Value::UInt(b)) => assert_eq!(a, b),
                (Value::Bytes(a), Value::Bytes(b)) => assert_eq!(a, b),
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(
            key_value(KeyKind::Int, &Value::Bytes(b"42".to_vec())).unwrap(),
            KeyValue::Int(42)
        );
        assert!(key_value(KeyKind::Int, &Value::NULL).is_err());
        assert!(key_param(&KeyValue::Null).is_err());
        assert!(key_param(&KeyValue::Bytes("abc".into())).is_err());
        assert!(key_param(&KeyValue::Bytes("a\u{00e9}b".into())).is_err());
        let keys = [KeyCol {
            name: "id".into(),
            kind: KeyKind::Int,
            collation: None,
        }];
        assert!(MysqlEngine::bound(Some(&[KeyValue::Text("oops".into())]), &keys).is_err());
    }

    #[test]
    fn dispatch_table_lists_every_rule() {
        let t = dispatch_table();
        assert_eq!(t.len(), capability::RULES.len());
        assert!(t.values().any(|v| v.contains("[8.2.0, ∞)")));
    }

    #[test]
    fn file_cursor_selection_preserves_disabled_binlog_refusal() {
        assert_eq!(
            file_position_mode(WatermarkMode::GtidSet).unwrap(),
            WatermarkMode::FilePos
        );
        assert_eq!(
            file_position_mode(WatermarkMode::FilePos).unwrap(),
            WatermarkMode::FilePos
        );
        assert!(file_position_mode(WatermarkMode::None).is_err());
    }
}
