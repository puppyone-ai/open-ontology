//! Replica read source for the chunked C1 baseline (DESIGN §3.13, FRAMES-V2 §9).
//!
//! CDC stays on the primary. Chunk reads may come from a replica of that primary.
//! Before each chunk the replica must have executed L0, the primary's GTID set at
//! `open`: `WAIT_FOR_EXECUTED_GTID_SET('<measured>:1-<L0>', t)` with t >= 1. A
//! timeout of 0 blocks forever on 5.7 and 8.4 (testing N10), so it is never sent.
//!
//! Why the result is correct (DESIGN §3.13): once the wait returns 0, the replica's
//! executed set contains L0. The replica only applies events from the primary, so
//! it never contains anything after L_end. Each chunk checks the same server identity,
//! read-only and GTID mode, lineage, delay and replication filters before the wait,
//! and filters and lineage again after it. A filter that skips a selected table could
//! hide events below L0 even though the wait passes, so it is refused.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row, Value as MyValue};
use oo_connectors::mysql::{
    Cancel, ChunkBatch, ChunkCursor, ChunkReader, MysqlConfig, MysqlError, ReadConnection,
    cancel_connection, connect,
};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::c1::gtid_contains;
use crate::c1_chunked::{ChunkSource, backoff};
use crate::config::read_secret;
use crate::error::PreviewError;
use crate::version::ServerVersion;

pub(crate) const DEFAULT_WAIT_TIMEOUT_S: u64 = 60;
pub(crate) const DEFAULT_RETRY_BUDGET_S: u64 = 1800;
const MAX_WAIT_TIMEOUT_S: u64 = 3600;
const MAX_RETRY_BUDGET_S: u64 = 86_400;
/// Client-side guard over one WAIT. The server bound is the timeout argument. This
/// only catches a connection that stopped answering.
const WAIT_CLIENT_SLACK_S: u64 = 15;
const CONNECT_TIMEOUT_MS: u64 = 30_000;

fn refuse(fact: &'static str) -> PreviewError {
    PreviewError::new("c1", fact)
}

/// Operator configuration: the `read_replica` block of the producer config file.
/// Credentials live only here, never in the bootstrap frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReplicaConfig {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) user: String,
    pub(crate) password_file: PathBuf,
    pub(crate) ca_file: PathBuf,
    pub(crate) tls_server_name: Option<String>,
    pub(crate) wait_timeout_s: u64,
    pub(crate) retry_budget_s: u64,
}

pub(crate) fn load_replica_config(
    path: &std::path::Path,
) -> Result<Option<ReplicaConfig>, PreviewError> {
    let bytes = std::fs::read(path).map_err(|_| PreviewError::new("config", "path"))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| PreviewError::new("config", "json"))?;
    match value.get("read_replica") {
        None | Some(Value::Null) => Ok(None),
        Some(block) => replica_config_from(block).map(Some),
    }
}

fn replica_config_from(block: &Value) -> Result<ReplicaConfig, PreviewError> {
    let refused = || PreviewError::new("config", "read_replica");
    let text = |key: &str| -> Result<String, PreviewError> {
        block
            .get(key)
            .and_then(Value::as_str)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .ok_or_else(refused)
    };
    let port = block
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|item| u16::try_from(item).ok())
        .filter(|item| *item > 0)
        .ok_or_else(refused)?;
    let tls_server_name = match block.get("tls_server_name") {
        None | Some(Value::Null) => None,
        Some(item) => Some(
            item.as_str()
                .filter(|name| oo_connectors::mysql::tls_server_name_ok(name))
                .ok_or_else(refused)?
                .to_owned(),
        ),
    };
    let wait_timeout_s = bounded_u64(
        block,
        "wait_timeout_s",
        DEFAULT_WAIT_TIMEOUT_S,
        MAX_WAIT_TIMEOUT_S,
    )?;
    let retry_budget_s = bounded_u64(
        block,
        "retry_budget_s",
        DEFAULT_RETRY_BUDGET_S,
        MAX_RETRY_BUDGET_S,
    )?;
    Ok(ReplicaConfig {
        host: text("host")?,
        port,
        user: text("user")?,
        password_file: PathBuf::from(text("password_file")?),
        ca_file: PathBuf::from(text("ca_file")?),
        tls_server_name,
        wait_timeout_s,
        retry_budget_s,
    })
}

/// Reads an optional integer in `1..=max`. Zero is refused, because a WAIT timeout
/// of 0 blocks forever (testing N10).
fn bounded_u64(map: &Value, key: &str, default: u64, max: u64) -> Result<u64, PreviewError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(item) => item
            .as_u64()
            .filter(|value| (1..=max).contains(value))
            .ok_or_else(|| PreviewError::new("config", "read_replica")),
    }
}

/// What the bootstrap asks for. `read_source` may carry the replica's identity to
/// pin and a wait timeout. It never carries credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReplicaRequest {
    pub(crate) identity: Option<String>,
    pub(crate) wait_timeout_s: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReadSource {
    Primary,
    Replica(ReplicaRequest),
}

pub(crate) fn parse_read_source(value: Option<&Value>) -> Result<ReadSource, PreviewError> {
    let refused = || PreviewError::new("config", "read_source");
    let Some(source) = value.filter(|item| !item.is_null()) else {
        return Ok(ReadSource::Primary);
    };
    match source.get("kind").and_then(Value::as_str) {
        None | Some("primary") => Ok(ReadSource::Primary),
        Some("replica") => {
            let identity = match source.get("replica_identity") {
                None | Some(Value::Null) => None,
                Some(item) => Some(
                    item.as_str()
                        .filter(|text| uuid_ok(text))
                        .ok_or_else(refused)?
                        .to_owned(),
                ),
            };
            let wait_timeout_s = match source.get("wait_timeout_s") {
                None | Some(Value::Null) => None,
                Some(item) => Some(
                    item.as_u64()
                        .filter(|value| (1..=MAX_WAIT_TIMEOUT_S).contains(value))
                        .ok_or_else(refused)?,
                ),
            };
            Ok(ReadSource::Replica(ReplicaRequest {
                identity,
                wait_timeout_s,
            }))
        }
        Some(_) => Err(refused()),
    }
}

fn uuid_ok(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// Column names of the replica status statement. They differ between 5.7 and 8.x.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StatusSql {
    pub(crate) statement: &'static str,
    pub(crate) source_uuid: &'static str,
    pub(crate) io_running: &'static str,
    pub(crate) sql_running: &'static str,
    pub(crate) seconds_behind: &'static str,
}

const SLAVE_STATUS: StatusSql = StatusSql {
    statement: "SHOW SLAVE STATUS",
    source_uuid: "Master_UUID",
    io_running: "Slave_IO_Running",
    sql_running: "Slave_SQL_Running",
    seconds_behind: "Seconds_Behind_Master",
};

const REPLICA_STATUS: StatusSql = StatusSql {
    statement: "SHOW REPLICA STATUS",
    source_uuid: "Source_UUID",
    io_running: "Replica_IO_Running",
    sql_running: "Replica_SQL_Running",
    seconds_behind: "Seconds_Behind_Source",
};

/// 5.7 uses `SHOW SLAVE STATUS`. 8.0.22 and later use `SHOW REPLICA STATUS` (8.4
/// removes the old statement, testing N10). The version comes from the replica.
pub(crate) fn status_sql(version: &str) -> Result<StatusSql, PreviewError> {
    let server = ServerVersion::parse(version)?;
    let replica_syntax = match server {
        ServerVersion::V57 => false,
        ServerVersion::V8(minor) => minor >= 1 || patch_of(version) >= 22,
    };
    Ok(if replica_syntax {
        REPLICA_STATUS
    } else {
        SLAVE_STATUS
    })
}

fn patch_of(version: &str) -> u32 {
    version
        .split('-')
        .next()
        .and_then(|core| core.split('.').nth(2))
        .and_then(|patch| patch.parse().ok())
        .unwrap_or(0)
}

/// One result row, keyed by column name. NULL is `None`.
pub(crate) type Columns = BTreeMap<String, Option<String>>;

pub(crate) fn columns_of(row: &Row) -> Columns {
    row.columns_ref()
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let value = match row.as_ref(index) {
                None | Some(MyValue::NULL) => None,
                Some(MyValue::Bytes(bytes)) => Some(String::from_utf8_lossy(bytes).into_owned()),
                Some(MyValue::Int(value)) => Some(value.to_string()),
                Some(MyValue::UInt(value)) => Some(value.to_string()),
                Some(other) => Some(format!("{other:?}")),
            };
            (column.name_str().into_owned(), value)
        })
        .collect()
}

fn cell(row: &Columns, name: &str) -> Result<String, PreviewError> {
    match row.get(name) {
        Some(value) => Ok(value.clone().unwrap_or_default()),
        None => Err(refuse("replica_status")),
    }
}

/// GTID sets from SHOW SLAVE STATUS can span lines. Whitespace is not part of a set.
fn gtid_text(text: &str) -> String {
    text.split_whitespace().collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Filters {
    pub(crate) do_db: String,
    pub(crate) ignore_db: String,
    pub(crate) do_table: String,
    pub(crate) ignore_table: String,
    pub(crate) wild_do_table: String,
    pub(crate) wild_ignore_table: String,
    pub(crate) rewrite_db: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReplicaStatus {
    pub(crate) source_uuid: String,
    pub(crate) io_running: bool,
    pub(crate) sql_running: bool,
    pub(crate) last_error: String,
    pub(crate) sql_delay: u64,
    pub(crate) retrieved: String,
    pub(crate) executed: String,
    pub(crate) seconds_behind: Option<u64>,
    pub(crate) filters: Filters,
}

pub(crate) fn parse_status(sql: StatusSql, row: &Columns) -> Result<ReplicaStatus, PreviewError> {
    let sql_delay = cell(row, "SQL_Delay")?
        .parse::<u64>()
        .map_err(|_| refuse("replica_status"))?;
    let last_error = [cell(row, "Last_Error")?, cell(row, "Last_IO_Error")?]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let seconds_behind = row
        .get(sql.seconds_behind)
        .and_then(|value| value.as_deref())
        .and_then(|text| text.parse::<u64>().ok());
    Ok(ReplicaStatus {
        source_uuid: cell(row, sql.source_uuid)?,
        io_running: cell(row, sql.io_running)? == "Yes",
        sql_running: cell(row, sql.sql_running)? == "Yes",
        last_error,
        sql_delay,
        retrieved: gtid_text(&cell(row, "Retrieved_Gtid_Set")?),
        executed: gtid_text(&cell(row, "Executed_Gtid_Set")?),
        seconds_behind,
        filters: Filters {
            do_db: cell(row, "Replicate_Do_DB")?,
            ignore_db: cell(row, "Replicate_Ignore_DB")?,
            do_table: cell(row, "Replicate_Do_Table")?,
            ignore_table: cell(row, "Replicate_Ignore_Table")?,
            wild_do_table: cell(row, "Replicate_Wild_Do_Table")?,
            wild_ignore_table: cell(row, "Replicate_Wild_Ignore_Table")?,
            rewrite_db: cell(row, "Replicate_Rewrite_DB")?,
        },
    })
}

/// The replica state read on every attempt: one round trip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReplicaState {
    pub(crate) uuid: String,
    pub(crate) read_only: bool,
    pub(crate) super_read_only: bool,
    pub(crate) gtid_on: bool,
}

const STATE_SQL: &str = "SELECT @@GLOBAL.server_uuid AS server_uuid, \
    @@GLOBAL.read_only AS read_only, @@GLOBAL.super_read_only AS super_read_only, \
    @@GLOBAL.gtid_mode AS gtid_mode, \
    @@GLOBAL.enforce_gtid_consistency AS enforce_gtid_consistency";

pub(crate) fn state_of(row: &Columns) -> Result<ReplicaState, PreviewError> {
    let on = |text: &str| matches!(text, "1" | "ON");
    Ok(ReplicaState {
        uuid: cell(row, "server_uuid")?,
        read_only: on(&cell(row, "read_only")?),
        super_read_only: on(&cell(row, "super_read_only")?),
        gtid_on: cell(row, "gtid_mode")? == "ON" && cell(row, "enforce_gtid_consistency")? == "ON",
    })
}

/// Whether one replication filter list can make events for `schema.table` disappear.
/// Matching is conservative. An entry that might match counts as a match.
pub(crate) fn table_filtered(filters: &Filters, schema: &str, table: &str) -> bool {
    let list = |text: &str| -> Vec<String> {
        text.split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let qualified = format!("{schema}.{table}");
    let do_db = list(&filters.do_db);
    if !do_db.is_empty() && !do_db.iter().any(|item| item == schema) {
        return true;
    }
    if list(&filters.ignore_db).iter().any(|item| item == schema) {
        return true;
    }
    if !filters.rewrite_db.trim().is_empty() && filters.rewrite_db.contains(schema) {
        return true;
    }
    let do_table = list(&filters.do_table);
    let wild_do = list(&filters.wild_do_table);
    if !do_table.is_empty() || !wild_do.is_empty() {
        let included = do_table.iter().any(|item| *item == qualified)
            || wild_do.iter().any(|pattern| like(pattern, &qualified));
        if !included {
            return true;
        }
    }
    if list(&filters.ignore_table)
        .iter()
        .any(|item| *item == qualified)
    {
        return true;
    }
    list(&filters.wild_ignore_table)
        .iter()
        .any(|pattern| like(pattern, &qualified))
}

/// MySQL pattern syntax used by the replication filters: `%`, `_` and `\` escapes.
fn like(pattern: &str, text: &str) -> bool {
    fn go(pattern: &[u8], text: &[u8]) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some((b'%', rest)) => (0..=text.len()).any(|skip| go(rest, &text[skip..])),
            Some((b'_', rest)) => !text.is_empty() && go(rest, &text[1..]),
            Some((b'\\', rest)) => match rest.split_first() {
                Some((literal, after)) => {
                    !text.is_empty() && text[0] == *literal && go(after, &text[1..])
                }
                None => false,
            },
            Some((literal, rest)) => {
                !text.is_empty() && text[0] == *literal && go(rest, &text[1..])
            }
        }
    }
    go(pattern.as_bytes(), text.as_bytes())
}

/// Where a check runs. At open, a failed check refuses the run. Mid-run, a lineage
/// change is reported as the CDC lineage refusal (DESIGN R6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Open,
    Midrun,
}

/// Checks that never wait: lineage, delay and filters.
pub(crate) fn decide_fatal(
    status: &ReplicaStatus,
    primary_uuid: &str,
    tables: &[(String, String)],
    phase: Phase,
) -> Result<(), PreviewError> {
    if status.source_uuid != primary_uuid {
        return Err(match phase {
            Phase::Open => refuse("replica_source"),
            Phase::Midrun => PreviewError::new("c1", "lineage"),
        });
    }
    if status.sql_delay != 0 {
        return Err(refuse("replica_delayed"));
    }
    if tables
        .iter()
        .any(|(schema, table)| table_filtered(&status.filters, schema, table))
    {
        return Err(refuse("replica_filter"));
    }
    Ok(())
}

/// Liveness: a replication error first, then a stopped thread. `None` means running.
pub(crate) fn liveness(status: &ReplicaStatus) -> Option<Behind> {
    if !status.last_error.is_empty() {
        Some(Behind::Error)
    } else if !(status.io_running && status.sql_running) {
        Some(Behind::Stopped)
    } else {
        None
    }
}

/// Why a replica is not ready for the next chunk. Every reason is retried within the
/// budget. The last reason decides the final error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Behind {
    Lag,
    Stopped,
    Error,
    Unreachable,
    ReadOnly,
}

impl Behind {
    pub(crate) fn fact(self) -> &'static str {
        match self {
            Self::Lag => "replica_lag",
            Self::Stopped => "replica_stopped",
            Self::Error => "replica_error",
            Self::Unreachable => "replica_unreachable",
            Self::ReadOnly => "replica_readonly",
        }
    }
}

/// Final error when the budget is spent. A lag that never moves and has not
/// delivered L0 to the replica is a stall: no retry can fix it (DESIGN R7).
pub(crate) fn exhausted(reason: Behind, stalled: bool) -> PreviewError {
    if reason == Behind::Lag && stalled {
        refuse("replica_stalled")
    } else {
        refuse(reason.fact())
    }
}

/// What the replica's retrieved set says about the measured primary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Retrieved {
    pub(crate) end: u64,
    pub(crate) has_l0: bool,
}

/// Largest GTID number of `uuid` in a set, or 0.
pub(crate) fn uuid_end(set: &str, uuid: &str) -> u64 {
    set.split(',')
        .filter_map(|entry| {
            let ranges = entry.strip_prefix(uuid)?.strip_prefix(':')?;
            ranges
                .split(':')
                .filter_map(|range| range.rsplit('-').next()?.parse::<u64>().ok())
                .max()
        })
        .max()
        .unwrap_or(0)
}

/// Decides whether a lag is a stall. It is a stall when the retrieved end did not move
/// for the whole budget and L0 is not in the retrieved set.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StallTracker {
    first: Option<u64>,
    moved: bool,
    last: Option<Retrieved>,
}

impl StallTracker {
    pub(crate) fn observe(&mut self, seen: Retrieved) {
        match self.first {
            None => self.first = Some(seen.end),
            Some(first) => self.moved |= seen.end != first,
        }
        self.last = Some(seen);
    }

    pub(crate) fn stalled(&self) -> bool {
        !self.moved && self.last.is_some_and(|seen| !seen.has_l0)
    }
}

/// Retry budget for one chunk's wait. Attempts run until `budget` is spent. Each
/// back-off is `backoff()` (1 s, doubling, capped at 30 s), cut to what is left.
#[derive(Debug)]
pub(crate) struct RetryBudget {
    budget: Duration,
    spent: Duration,
    failures: u32,
}

impl RetryBudget {
    pub(crate) fn new(budget_s: u64) -> Self {
        Self {
            budget: Duration::from_secs(budget_s),
            spent: Duration::ZERO,
            failures: 0,
        }
    }

    /// WAIT timeout for the next attempt: the configured value, cut to the budget left.
    /// Never 0, because 0 waits forever (testing N10).
    pub(crate) fn wait_timeout_s(&self, configured: u64) -> u64 {
        let left = self.budget.saturating_sub(self.spent).as_secs_f64().ceil() as u64;
        configured.min(left).max(1)
    }

    /// Records one failed attempt that took `took`. Returns the back-off before the
    /// next attempt, or `None` when the budget is spent.
    pub(crate) fn after_failure(&mut self, took: Duration) -> Option<Duration> {
        self.failures += 1;
        self.spent += took;
        if self.spent >= self.budget {
            return None;
        }
        let sleep = backoff(self.failures).min(self.budget - self.spent);
        self.spent += sleep;
        Some(sleep)
    }
}

/// One-time facts gathered at open, from the replica and the primary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Facts {
    pub(crate) replica_uuid: String,
    pub(crate) primary_uuid: String,
    pub(crate) pinned: Option<String>,
    pub(crate) read_only: bool,
    pub(crate) super_read_only: bool,
    pub(crate) gtid_on: bool,
    pub(crate) schema_utf8mb4: bool,
    pub(crate) tables_present: bool,
    pub(crate) schema_match: bool,
}

pub(crate) fn decide_facts(facts: &Facts) -> Result<(), PreviewError> {
    if facts.replica_uuid == facts.primary_uuid {
        return Err(refuse("replica_identity"));
    }
    if facts
        .pinned
        .as_deref()
        .is_some_and(|pin| pin != facts.replica_uuid)
    {
        return Err(refuse("replica_identity"));
    }
    if !facts.gtid_on {
        return Err(refuse("replica_gtid"));
    }
    if !(facts.read_only && facts.super_read_only) {
        return Err(refuse("replica_readonly"));
    }
    if !facts.schema_utf8mb4 {
        return Err(refuse("replica_schema"));
    }
    if !facts.tables_present || !facts.schema_match {
        return Err(refuse("replica_schema"));
    }
    Ok(())
}

/// One line of the schema fingerprint for a column. NULL is `\N`.
pub(crate) fn column_line(row: &Columns) -> String {
    const FIELDS: [&str; 8] = [
        "COLUMN_NAME",
        "ORDINAL_POSITION",
        "COLUMN_TYPE",
        "IS_NULLABLE",
        "COLUMN_KEY",
        "DATA_TYPE",
        "CHARACTER_SET_NAME",
        "COLLATION_NAME",
    ];
    let parts: Vec<String> = FIELDS
        .iter()
        .map(|name| {
            row.get(*name)
                .and_then(|v| v.clone())
                .unwrap_or_else(|| "\\N".to_owned())
        })
        .collect();
    format!("{}\n", parts.join("\t"))
}

const SCHEMA_SQL: &str = "SELECT COLUMN_NAME, ORDINAL_POSITION, COLUMN_TYPE, IS_NULLABLE, \
    COLUMN_KEY, DATA_TYPE, CHARACTER_SET_NAME, COLLATION_NAME FROM information_schema.COLUMNS \
    WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION";

/// Fingerprint of the selected tables' columns, and whether every table exists.
pub(crate) async fn schema_digest(
    conn: &mut Conn,
    tables: &[(String, String)],
) -> Result<(String, bool), PreviewError> {
    let mut text = String::from("oo.c1.replica-schema/v1\n");
    let mut present = true;
    for (schema, table) in tables {
        let rows: Vec<Row> = conn
            .exec(SCHEMA_SQL, (schema.as_str(), table.as_str()))
            .await
            .map_err(|_| PreviewError::new("protocol", "query"))?;
        present &= !rows.is_empty();
        text.push_str(&format!("T:{schema}.{table}\n"));
        for row in &rows {
            text.push_str(&column_line(&columns_of(row)));
        }
    }
    Ok((sha256_hex(text.as_bytes()), present))
}

/// What one attempt found.
#[derive(Debug)]
enum Attempt {
    Ready,
    Behind {
        reason: Behind,
        retrieved: Option<Retrieved>,
    },
    /// The connection failed. It is discarded and a new one is opened next time.
    Broken,
}

impl Attempt {
    fn behind(reason: Behind) -> Self {
        Self::Behind {
            reason,
            retrieved: None,
        }
    }
}

/// `Full` runs liveness and the wait. `Fatal` runs only the non-waiting checks, and
/// is used once more after the last chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Full,
    Fatal,
}

#[derive(Debug, Default)]
struct WaitStats {
    waits: u64,
    wait_total_ms: u64,
    by_reason: BTreeMap<&'static str, u64>,
}

/// Replica chunk source. The connection is always the replica's; the chunk reader
/// uses it through `ReadConnection`, so control queries and reads share one session.
pub(crate) struct ReplicaSource {
    config: MysqlConfig,
    password: Zeroizing<String>,
    conn: Option<Conn>,
    cancel: Cancel,
    primary_uuid: String,
    identity: String,
    status_sql: StatusSql,
    tables: Vec<(String, String)>,
    wait_timeout_s: u64,
    retry_budget_s: u64,
    seconds_behind_at_open: Option<u64>,
    stats: WaitStats,
}

impl ReplicaSource {
    /// Connects, gathers the one-time facts and refuses what cannot be fixed by
    /// waiting. Liveness is left to `prepare`, which waits within the budget.
    pub(crate) async fn open(
        replica: ReplicaConfig,
        request: ReplicaRequest,
        base: &MysqlConfig,
        primary: &mut Conn,
        primary_uuid: &str,
        tables: &[String],
    ) -> Result<Self, PreviewError> {
        let password = read_secret(&replica.password_file)?;
        let config = MysqlConfig {
            mysql_host: replica.host.clone(),
            mysql_port: replica.port,
            mysql_user: replica.user.clone(),
            mysql_ca_file: replica.ca_file.clone(),
            mysql_tls_server_name: replica.tls_server_name.clone(),
            connect_timeout_ms: CONNECT_TIMEOUT_MS,
            ..base.clone()
        };
        let mut conn = match connect(&config, password.as_str()).await {
            Ok(conn) => conn,
            Err(err) if err.code == "connect" => return Err(refuse("replica_unreachable")),
            Err(err) => return Err(err.into()),
        };
        let tables: Vec<(String, String)> = tables
            .iter()
            .map(|name| {
                name.split_once('.')
                    .map(|(s, t)| (s.to_owned(), t.to_owned()))
                    .ok_or_else(|| PreviewError::new("catalog", "tables"))
            })
            .collect::<Result<_, _>>()?;
        let version = first_text(&mut conn, "SELECT @@GLOBAL.version AS version").await?;
        let status_sql = status_sql(&version)?;
        let state = state_of(&unreachable_on_error(
            query_columns(&mut conn, STATE_SQL).await,
        )?)?;
        let status_row = query_status(&mut conn, status_sql).await?;
        let status = parse_status(status_sql, &status_row)?;
        decide_fatal(&status, primary_uuid, &tables, Phase::Open)?;
        let schema_charset = schema_charset(&mut conn, &base.mysql_database).await?;
        let (replica_digest, replica_present) = schema_digest(&mut conn, &tables).await?;
        let (primary_digest, primary_present) = schema_digest(primary, &tables).await?;
        let facts = Facts {
            replica_uuid: state.uuid.clone(),
            primary_uuid: primary_uuid.to_owned(),
            pinned: request.identity.clone(),
            read_only: state.read_only,
            super_read_only: state.super_read_only,
            gtid_on: state.gtid_on,
            schema_utf8mb4: schema_charset == "utf8mb4",
            tables_present: replica_present && primary_present,
            schema_match: replica_digest == primary_digest,
        };
        decide_facts(&facts)?;
        Ok(Self {
            config,
            password,
            conn: Some(conn),
            cancel: Cancel::default(),
            primary_uuid: primary_uuid.to_owned(),
            identity: state.uuid,
            status_sql,
            tables,
            wait_timeout_s: request.wait_timeout_s.unwrap_or(replica.wait_timeout_s),
            retry_budget_s: replica.retry_budget_s,
            seconds_behind_at_open: status.seconds_behind,
            stats: WaitStats::default(),
        })
    }

    pub(crate) fn identity(&self) -> &str {
        &self.identity
    }

    /// Wire and receipt form of the replica facts and the waits that happened.
    pub(crate) fn receipt(&self) -> Value {
        json!({
            "identity": self.identity,
            "wait_timeout_s": self.wait_timeout_s,
            "retry_budget_s": self.retry_budget_s,
            "seconds_behind_at_open": self.seconds_behind_at_open,
            "wait_count": self.stats.waits,
            "wait_total_ms": self.stats.wait_total_ms,
            "waits_by_reason": self.stats.by_reason,
        })
    }

    /// Non-waiting checks, run once more after the last chunk (DESIGN §3.13).
    pub(crate) async fn verify(&mut self, l0_pos: u64) -> Result<(), PreviewError> {
        match self
            .run_attempt(l0_pos, self.wait_timeout_s, Scope::Fatal)
            .await?
        {
            Attempt::Ready => Ok(()),
            Attempt::Behind { reason, .. } => Err(exhausted(reason, false)),
            Attempt::Broken => Err(refuse("replica_unreachable")),
        }
    }

    async fn run_attempt(
        &mut self,
        l0_pos: u64,
        timeout_s: u64,
        scope: Scope,
    ) -> Result<Attempt, PreviewError> {
        let mut conn = match self.conn.take() {
            Some(conn) => conn,
            None => match connect(&self.config, self.password.as_str()).await {
                Ok(conn) => conn,
                Err(err) if err.code == "connect" => return Ok(Attempt::Broken),
                Err(err) => return Err(err.into()),
            },
        };
        match self.check(&mut conn, l0_pos, timeout_s, scope).await {
            Ok(Attempt::Broken) => {
                self.discard(conn).await;
                Ok(Attempt::Broken)
            }
            Ok(attempt) => {
                self.conn = Some(conn);
                Ok(attempt)
            }
            Err(err) => {
                self.discard(conn).await;
                Err(err)
            }
        }
    }

    async fn discard(&self, conn: Conn) {
        let _ = cancel_connection(conn, &self.config, self.password.as_str()).await;
    }

    /// After a successful WAIT, filters and lineage are checked again. A filter set while
    /// L0 was applying can skip events at or below L0, and the wait cannot see that.
    async fn recheck_after_wait(&self, conn: &mut Conn) -> Result<Attempt, PreviewError> {
        match query_status_row(conn, self.status_sql).await {
            Ok(Some(row)) => {
                let status = parse_status(self.status_sql, &columns_of(&row))?;
                decide_fatal(&status, &self.primary_uuid, &self.tables, Phase::Midrun)?;
                Ok(Attempt::Ready)
            }
            Ok(None) => Err(refuse("replica_source")),
            Err(_) => Ok(Attempt::Broken),
        }
    }

    async fn check(
        &self,
        conn: &mut Conn,
        l0_pos: u64,
        timeout_s: u64,
        scope: Scope,
    ) -> Result<Attempt, PreviewError> {
        let Ok(columns) = query_columns(conn, STATE_SQL).await else {
            return Ok(Attempt::Broken);
        };
        let state = state_of(&columns)?;
        if state.uuid != self.identity {
            return Err(refuse("replica_identity"));
        }
        if !state.gtid_on {
            return Err(refuse("replica_gtid"));
        }
        if !(state.read_only && state.super_read_only) {
            return Ok(Attempt::behind(Behind::ReadOnly));
        }
        let row = match query_status_row(conn, self.status_sql).await {
            Ok(row) => row,
            Err(err) if is_server_code(&err, 1227) => {
                return Err(PreviewError::new("privilege", "replication"));
            }
            Err(_) => return Ok(Attempt::Broken),
        };
        let row = row.ok_or_else(|| refuse("replica_source"))?;
        let status = parse_status(self.status_sql, &columns_of(&row))?;
        decide_fatal(&status, &self.primary_uuid, &self.tables, Phase::Midrun)?;
        if scope == Scope::Fatal {
            return Ok(Attempt::Ready);
        }
        let seen = Retrieved {
            end: uuid_end(&status.retrieved, &self.primary_uuid),
            has_l0: gtid_contains(&status.retrieved, &self.primary_uuid, l0_pos),
        };
        if let Some(reason) = liveness(&status) {
            return Ok(Attempt::Behind {
                reason,
                retrieved: Some(seen),
            });
        }
        let set = format!("{}:1-{l0_pos}", self.primary_uuid);
        let waited = tokio::time::timeout(
            Duration::from_secs(timeout_s + WAIT_CLIENT_SLACK_S),
            conn.exec_first::<i64, _, _>(
                "SELECT WAIT_FOR_EXECUTED_GTID_SET(?, ?)",
                (set, timeout_s),
            ),
        )
        .await;
        match waited {
            Err(_) => Ok(Attempt::Broken),
            Ok(Err(err)) if is_server_code(&err, 1772) || is_server_code(&err, 1210) => {
                Err(refuse("replica_wait"))
            }
            Ok(Err(_)) => Ok(Attempt::Broken),
            Ok(Ok(Some(0))) => self.recheck_after_wait(conn).await,
            Ok(Ok(Some(1))) => Ok(Attempt::Behind {
                reason: Behind::Lag,
                retrieved: Some(seen),
            }),
            Ok(Ok(_)) => Err(refuse("replica_wait")),
        }
    }
}

fn is_server_code(err: &mysql_async::Error, code: u16) -> bool {
    matches!(err, mysql_async::Error::Server(server) if server.code == code)
}

fn unreachable_on_error<T>(result: Result<T, mysql_async::Error>) -> Result<T, PreviewError> {
    result.map_err(|_| refuse("replica_unreachable"))
}

async fn first_text(conn: &mut Conn, sql: &str) -> Result<String, PreviewError> {
    let row: Option<Row> = conn
        .query_first(sql)
        .await
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    let row = row.ok_or_else(|| PreviewError::new("protocol", "query"))?;
    cell(&columns_of(&row), "version")
}

async fn query_columns(conn: &mut Conn, sql: &str) -> Result<Columns, mysql_async::Error> {
    let row: Option<Row> = conn.query_first(sql).await?;
    Ok(row.map(|row| columns_of(&row)).unwrap_or_default())
}

async fn query_status_row(
    conn: &mut Conn,
    sql: StatusSql,
) -> Result<Option<Row>, mysql_async::Error> {
    conn.query_first(sql.statement).await
}

async fn query_status(conn: &mut Conn, sql: StatusSql) -> Result<Columns, PreviewError> {
    let row = query_status_row(conn, sql).await.map_err(|err| {
        if is_server_code(&err, 1227) {
            PreviewError::new("privilege", "replication")
        } else {
            PreviewError::new("protocol", "query")
        }
    })?;
    row.map(|row| columns_of(&row))
        .ok_or_else(|| refuse("replica_source"))
}

async fn schema_charset(conn: &mut Conn, schema: &str) -> Result<String, PreviewError> {
    let row: Option<Row> = conn
        .exec_first(
            "SELECT DEFAULT_CHARACTER_SET_NAME AS charset FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?",
            (schema,),
        )
        .await
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    row.map(|row| cell(&columns_of(&row), "charset"))
        .unwrap_or_else(|| Ok(String::new()))
}

fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

impl ChunkSource for ReplicaSource {
    fn kind(&self) -> &'static str {
        "replica"
    }

    /// Waits until the replica has executed L0. Each retry sleeps in this future, so
    /// the caller keeps consuming CDC while it waits (`pump`).
    async fn prepare(&mut self, l0_pos: u64) -> Result<u64, PreviewError> {
        let started = Instant::now();
        let mut budget = RetryBudget::new(self.retry_budget_s);
        let mut stall = StallTracker::default();
        let mut attempts = 0u32;
        loop {
            let began = Instant::now();
            let timeout_s = budget.wait_timeout_s(self.wait_timeout_s);
            match self.run_attempt(l0_pos, timeout_s, Scope::Full).await? {
                Attempt::Ready => {
                    if attempts > 0 {
                        self.stats.wait_total_ms += elapsed_ms(started);
                    }
                    return Ok(elapsed_ms(started));
                }
                Attempt::Broken => {
                    self.stats.waits += 1;
                    *self
                        .stats
                        .by_reason
                        .entry(Behind::Unreachable.fact())
                        .or_default() += 1;
                    attempts += 1;
                    match budget.after_failure(began.elapsed()) {
                        Some(sleep) => self.pause(Behind::Unreachable, attempts, sleep).await,
                        None => return Err(exhausted(Behind::Unreachable, false)),
                    }
                }
                Attempt::Behind { reason, retrieved } => {
                    self.stats.waits += 1;
                    *self.stats.by_reason.entry(reason.fact()).or_default() += 1;
                    attempts += 1;
                    if let (Behind::Lag, Some(seen)) = (reason, retrieved) {
                        stall.observe(seen);
                    }
                    match budget.after_failure(began.elapsed()) {
                        Some(sleep) => self.pause(reason, attempts, sleep).await,
                        None => {
                            return Err(exhausted(
                                reason,
                                reason == Behind::Lag && stall.stalled(),
                            ));
                        }
                    }
                }
            }
        }
    }

    async fn read(
        &mut self,
        reader: &ChunkReader,
        cursor: &mut ChunkCursor,
    ) -> Result<ChunkBatch, MysqlError> {
        let conn = self
            .conn
            .take()
            .ok_or_else(|| MysqlError::new("connect", "replica_closed"))?;
        let mut read = ReadConnection::new(conn, self.config.clone(), self.password.clone());
        let result = reader.read_chunk(&mut read, cursor, &self.cancel).await;
        if let Ok(conn) = read.into_connection() {
            self.conn = Some(conn);
        }
        result
    }

    /// Drops the connection. The next attempt reconnects and checks the identity again.
    async fn reconnect(&mut self) -> Result<(), PreviewError> {
        self.conn = None;
        Ok(())
    }
}

impl ReplicaSource {
    async fn pause(&self, reason: Behind, attempt: u32, sleep: Duration) {
        eprintln!(
            "mysql-preview: replica retry reason={} attempt={attempt} sleep_ms={}",
            reason.fact(),
            sleep.as_millis()
        );
        tokio::time::sleep(sleep).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: &str = "aaaaaaaa-0000-0000-0000-000000000001";
    const REPLICA: &str = "bbbbbbbb-0000-0000-0000-000000000002";
    const OTHER: &str = "cccccccc-0000-0000-0000-000000000003";

    fn columns(pairs: &[(&str, &str)]) -> Columns {
        pairs
            .iter()
            .map(|(name, value)| {
                (
                    (*name).to_owned(),
                    (!value.is_empty() || *name == "Last_Error").then(|| (*value).to_owned()),
                )
            })
            .collect()
    }

    /// A healthy replica row for the given statement dialect.
    fn healthy(sql: StatusSql, source: &str) -> Columns {
        let mut row = columns(&[
            ("Last_Error", ""),
            ("Last_IO_Error", ""),
            ("SQL_Delay", "0"),
            (
                "Retrieved_Gtid_Set",
                &format!("{source}:1-120,\n{OTHER}:1-7"),
            ),
            ("Executed_Gtid_Set", &format!("{source}:1-118")),
            ("Replicate_Do_DB", ""),
            ("Replicate_Ignore_DB", ""),
            ("Replicate_Do_Table", ""),
            ("Replicate_Ignore_Table", ""),
            ("Replicate_Wild_Do_Table", ""),
            ("Replicate_Wild_Ignore_Table", ""),
            ("Replicate_Rewrite_DB", ""),
        ]);
        row.insert(sql.source_uuid.to_owned(), Some(source.to_owned()));
        row.insert(sql.io_running.to_owned(), Some("Yes".to_owned()));
        row.insert(sql.sql_running.to_owned(), Some("Yes".to_owned()));
        row.insert(sql.seconds_behind.to_owned(), Some("2".to_owned()));
        row
    }

    fn fatal_status(sql: StatusSql, source: &str) -> ReplicaStatus {
        parse_status(sql, &healthy(sql, source)).unwrap()
    }

    fn tables() -> Vec<(String, String)> {
        vec![
            ("oo_c1".to_owned(), "cb_hdr".to_owned()),
            ("oo_c1".to_owned(), "cb_line".to_owned()),
        ]
    }

    // ---- version and statement selection ----

    #[test]
    fn statement_follows_the_server_version() {
        assert_eq!(
            status_sql("5.7.44-log").unwrap().statement,
            "SHOW SLAVE STATUS"
        );
        assert_eq!(status_sql("8.0.21").unwrap().statement, "SHOW SLAVE STATUS");
        assert_eq!(
            status_sql("8.0.22").unwrap().statement,
            "SHOW REPLICA STATUS"
        );
        assert_eq!(
            status_sql("8.0.41").unwrap().statement,
            "SHOW REPLICA STATUS"
        );
        assert_eq!(
            status_sql("8.4.10").unwrap().statement,
            "SHOW REPLICA STATUS"
        );
        for wrong in ["5.6.51", "9.0.1", "5.7.44-MariaDB"] {
            assert!(status_sql(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn column_names_differ_between_5_7_and_8_4() {
        let slave = status_sql("5.7.44").unwrap();
        assert_eq!(
            (slave.source_uuid, slave.io_running, slave.sql_running),
            ("Master_UUID", "Slave_IO_Running", "Slave_SQL_Running")
        );
        let replica = status_sql("8.4.10").unwrap();
        assert_eq!(
            (replica.source_uuid, replica.io_running, replica.sql_running),
            ("Source_UUID", "Replica_IO_Running", "Replica_SQL_Running")
        );
    }

    // ---- status parsing ----

    #[test]
    fn parses_the_5_7_status_columns() {
        let sql = status_sql("5.7.44").unwrap();
        let status = parse_status(sql, &healthy(sql, PRIMARY)).unwrap();
        assert_eq!(status.source_uuid, PRIMARY);
        assert!(status.io_running && status.sql_running);
        assert!(status.last_error.is_empty());
        assert_eq!(status.sql_delay, 0);
        assert_eq!(status.seconds_behind, Some(2));
        assert_eq!(
            status.retrieved,
            format!("{PRIMARY}:1-120,{OTHER}:1-7"),
            "newlines are not part of a GTID set"
        );
    }

    #[test]
    fn parses_the_8_4_status_columns_and_ignores_extra_columns() {
        let sql = status_sql("8.4.10").unwrap();
        let mut row = healthy(sql, PRIMARY);
        row.insert(
            "Replica_SQL_Running_State".to_owned(),
            Some("Waiting for dependent".to_owned()),
        );
        let status = parse_status(sql, &row).unwrap();
        assert_eq!(status.source_uuid, PRIMARY);
        assert!(status.io_running && status.sql_running);
        assert_eq!(status.executed, format!("{PRIMARY}:1-118"));
    }

    #[test]
    fn a_status_row_without_the_dialect_columns_is_refused() {
        let slave = status_sql("5.7.44").unwrap();
        let replica = status_sql("8.4.10").unwrap();
        // A 5.7 row read with the 8.4 names: the uuid column is missing.
        let row = healthy(slave, PRIMARY);
        assert_eq!(
            parse_status(replica, &row).unwrap_err(),
            refuse("replica_status")
        );
        let mut missing_filter = healthy(replica, PRIMARY);
        missing_filter.remove("Replicate_Ignore_Table");
        assert_eq!(
            parse_status(replica, &missing_filter).unwrap_err(),
            refuse("replica_status")
        );
    }

    #[test]
    fn replication_errors_and_stopped_threads_are_reported_in_liveness() {
        let sql = status_sql("8.4.10").unwrap();
        let running = fatal_status(sql, PRIMARY);
        assert_eq!(liveness(&running), None);

        let mut stopped = healthy(sql, PRIMARY);
        stopped.insert("Replica_SQL_Running".to_owned(), Some("No".to_owned()));
        assert_eq!(
            liveness(&parse_status(sql, &stopped).unwrap()),
            Some(Behind::Stopped)
        );

        let mut broken = healthy(sql, PRIMARY);
        broken.insert(
            "Last_Error".to_owned(),
            Some("Lock wait timeout exceeded".to_owned()),
        );
        assert_eq!(
            liveness(&parse_status(sql, &broken).unwrap()),
            Some(Behind::Error)
        );
    }

    // ---- filters ----

    fn filters(pairs: &[(&str, &str)]) -> Filters {
        let mut out = Filters::default();
        for (name, value) in pairs {
            let slot = match *name {
                "do_db" => &mut out.do_db,
                "ignore_db" => &mut out.ignore_db,
                "do_table" => &mut out.do_table,
                "ignore_table" => &mut out.ignore_table,
                "wild_do" => &mut out.wild_do_table,
                "wild_ignore" => &mut out.wild_ignore_table,
                "rewrite" => &mut out.rewrite_db,
                other => panic!("{other}"),
            };
            *slot = (*value).to_owned();
        }
        out
    }

    #[test]
    fn no_filter_affects_nothing() {
        assert!(!table_filtered(&Filters::default(), "oo_c1", "cb_hdr"));
    }

    #[test]
    fn ignore_table_refuses_only_the_named_table() {
        let f = filters(&[("ignore_table", "oo_c1.cb_hdr")]);
        assert!(table_filtered(&f, "oo_c1", "cb_hdr"));
        assert!(!table_filtered(&f, "oo_c1", "cb_line"));
    }

    #[test]
    fn do_table_refuses_every_selected_table_it_does_not_name() {
        let f = filters(&[("do_table", "oo_c1.cb_line")]);
        assert!(table_filtered(&f, "oo_c1", "cb_hdr"), "cb_hdr is skipped");
        assert!(!table_filtered(&f, "oo_c1", "cb_line"));
    }

    #[test]
    fn wildcard_filters_follow_mysql_pattern_rules() {
        assert!(table_filtered(
            &filters(&[("wild_ignore", "oo_c1.cb_%")]),
            "oo_c1",
            "cb_hdr"
        ));
        assert!(!table_filtered(
            &filters(&[("wild_ignore", "other.%")]),
            "oo_c1",
            "cb_hdr"
        ));
        assert!(!table_filtered(
            &filters(&[("wild_do", "oo_c1.cb_h%")]),
            "oo_c1",
            "cb_hdr"
        ));
        assert!(table_filtered(
            &filters(&[("wild_do", "oo_c1.cb_h%")]),
            "oo_c1",
            "cb_line"
        ));
        assert!(like("a_c", "abc") && like("%", "anything") && like("a\\_c", "a_c"));
        assert!(!like("a\\_c", "abc") && !like("ab", "abc"));
    }

    #[test]
    fn database_filters_and_rewrites_refuse_the_selected_schema() {
        assert!(table_filtered(
            &filters(&[("ignore_db", "oo_c1")]),
            "oo_c1",
            "cb_hdr"
        ));
        assert!(!table_filtered(
            &filters(&[("ignore_db", "other")]),
            "oo_c1",
            "cb_hdr"
        ));
        assert!(table_filtered(
            &filters(&[("do_db", "other")]),
            "oo_c1",
            "cb_hdr"
        ));
        assert!(!table_filtered(
            &filters(&[("do_db", "oo_c1, other")]),
            "oo_c1",
            "cb_hdr"
        ));
        assert!(table_filtered(
            &filters(&[("rewrite", "(oo_c1, copy)")]),
            "oo_c1",
            "cb_hdr"
        ));
        assert!(!table_filtered(
            &filters(&[("rewrite", "(other, copy)")]),
            "oo_c1",
            "cb_hdr"
        ));
    }

    #[test]
    fn decide_fatal_names_lineage_delay_and_filters_by_phase() {
        let sql = status_sql("5.7.44").unwrap();
        let ok = fatal_status(sql, PRIMARY);
        assert!(decide_fatal(&ok, PRIMARY, &tables(), Phase::Open).is_ok());
        let moved = fatal_status(sql, OTHER);
        assert_eq!(
            decide_fatal(&moved, PRIMARY, &tables(), Phase::Open).unwrap_err(),
            refuse("replica_source")
        );
        assert_eq!(
            decide_fatal(&moved, PRIMARY, &tables(), Phase::Midrun).unwrap_err(),
            PreviewError::new("c1", "lineage")
        );
        let mut delayed = healthy(sql, PRIMARY);
        delayed.insert("SQL_Delay".to_owned(), Some("60".to_owned()));
        let delayed = parse_status(sql, &delayed).unwrap();
        assert_eq!(
            decide_fatal(&delayed, PRIMARY, &tables(), Phase::Open).unwrap_err(),
            refuse("replica_delayed")
        );
        let mut filtered = healthy(sql, PRIMARY);
        filtered.insert(
            "Replicate_Ignore_Table".to_owned(),
            Some("oo_c1.cb_hdr".to_owned()),
        );
        let filtered = parse_status(sql, &filtered).unwrap();
        assert_eq!(
            decide_fatal(&filtered, PRIMARY, &tables(), Phase::Midrun).unwrap_err(),
            refuse("replica_filter")
        );
    }

    // ---- open-time facts ----

    fn good_facts() -> Facts {
        Facts {
            replica_uuid: REPLICA.to_owned(),
            primary_uuid: PRIMARY.to_owned(),
            pinned: None,
            read_only: true,
            super_read_only: true,
            gtid_on: true,
            schema_utf8mb4: true,
            tables_present: true,
            schema_match: true,
        }
    }

    #[test]
    fn open_facts_refuse_each_unfixable_condition() {
        assert!(decide_facts(&good_facts()).is_ok());
        let cases: Vec<(Box<dyn Fn(&mut Facts)>, &str)> = vec![
            (
                Box::new(|f| f.replica_uuid = PRIMARY.to_owned()),
                "replica_identity",
            ),
            (
                Box::new(|f| f.pinned = Some(OTHER.to_owned())),
                "replica_identity",
            ),
            (Box::new(|f| f.gtid_on = false), "replica_gtid"),
            (Box::new(|f| f.read_only = false), "replica_readonly"),
            (Box::new(|f| f.super_read_only = false), "replica_readonly"),
            (Box::new(|f| f.schema_utf8mb4 = false), "replica_schema"),
            (Box::new(|f| f.tables_present = false), "replica_schema"),
            (Box::new(|f| f.schema_match = false), "replica_schema"),
        ];
        for (mutate, fact) in cases {
            let mut facts = good_facts();
            mutate(&mut facts);
            assert_eq!(decide_facts(&facts).unwrap_err(), refuse(fact));
        }
        let mut pinned = good_facts();
        pinned.pinned = Some(REPLICA.to_owned());
        assert!(decide_facts(&pinned).is_ok(), "the pinned identity matches");
    }

    #[test]
    fn replica_state_reads_mysql_flag_spellings() {
        let row = columns(&[
            ("server_uuid", REPLICA),
            ("read_only", "1"),
            ("super_read_only", "1"),
            ("gtid_mode", "ON"),
            ("enforce_gtid_consistency", "ON"),
        ]);
        let state = state_of(&row).unwrap();
        assert!(state.read_only && state.super_read_only && state.gtid_on);
        let mut off = row.clone();
        off.insert("super_read_only".to_owned(), Some("0".to_owned()));
        assert!(!state_of(&off).unwrap().super_read_only);
    }

    #[test]
    fn schema_fingerprint_changes_with_any_column_difference() {
        let base = columns(&[
            ("COLUMN_NAME", "note"),
            ("ORDINAL_POSITION", "2"),
            ("COLUMN_TYPE", "varchar(64)"),
            ("IS_NULLABLE", "NO"),
            ("COLUMN_KEY", ""),
            ("DATA_TYPE", "varchar"),
            ("CHARACTER_SET_NAME", "utf8mb4"),
            ("COLLATION_NAME", "utf8mb4_bin"),
        ]);
        let mut wider = base.clone();
        wider.insert("COLUMN_TYPE".to_owned(), Some("varchar(128)".to_owned()));
        assert_ne!(column_line(&base), column_line(&wider));
        let mut null_charset = base.clone();
        null_charset.insert("CHARACTER_SET_NAME".to_owned(), None);
        assert!(column_line(&null_charset).contains("\\N"));
    }

    // ---- budget, backoff and stall ----

    #[test]
    fn wait_timeout_is_never_zero_and_is_cut_to_the_budget_left() {
        let mut budget = RetryBudget::new(10);
        assert_eq!(budget.wait_timeout_s(60), 10, "cut to the budget");
        assert_eq!(
            budget.wait_timeout_s(4),
            4,
            "configured value wins when it fits"
        );
        assert_eq!(
            budget.after_failure(Duration::from_secs(4)),
            Some(Duration::from_secs(1))
        );
        assert_eq!(budget.wait_timeout_s(60), 5);
        assert_eq!(
            budget.after_failure(Duration::from_secs(4)),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            budget.wait_timeout_s(60),
            1,
            "the last attempt still waits at least one second"
        );
        assert_eq!(
            budget.after_failure(Duration::from_secs(1)),
            None,
            "the budget is spent"
        );
    }

    #[test]
    fn backoff_follows_the_chunk_schedule_and_stops_at_the_budget() {
        let mut budget = RetryBudget::new(3600);
        let sleeps: Vec<u64> = (0..7)
            .map(|_| budget.after_failure(Duration::ZERO).unwrap().as_secs())
            .collect();
        assert_eq!(sleeps, vec![1, 2, 4, 8, 16, 30, 30]);
    }

    #[test]
    fn uuid_end_reads_the_largest_number_of_the_measured_uuid() {
        let set = format!("{PRIMARY}:1-5:7-9,\n{OTHER}:1-3");
        assert_eq!(uuid_end(&gtid_text(&set), PRIMARY), 9);
        assert_eq!(uuid_end(&gtid_text(&set), OTHER), 3);
        assert_eq!(uuid_end("", PRIMARY), 0);
        assert_eq!(uuid_end(&format!("{REPLICA}:1-40"), PRIMARY), 0);
    }

    #[test]
    fn a_lag_that_never_moves_and_lacks_l0_is_a_stall() {
        let mut stall = StallTracker::default();
        stall.observe(Retrieved {
            end: 10,
            has_l0: false,
        });
        stall.observe(Retrieved {
            end: 10,
            has_l0: false,
        });
        assert!(stall.stalled());
        let mut moving = StallTracker::default();
        moving.observe(Retrieved {
            end: 10,
            has_l0: false,
        });
        moving.observe(Retrieved {
            end: 11,
            has_l0: false,
        });
        assert!(!moving.stalled());
        let mut delivered = StallTracker::default();
        delivered.observe(Retrieved {
            end: 50,
            has_l0: true,
        });
        delivered.observe(Retrieved {
            end: 50,
            has_l0: true,
        });
        assert!(
            !delivered.stalled(),
            "a lock-blocked replica has L0 and is only lagging"
        );
    }

    #[test]
    fn the_final_error_names_the_last_reason_and_stalls_only_a_lag() {
        assert_eq!(exhausted(Behind::Lag, false), refuse("replica_lag"));
        assert_eq!(exhausted(Behind::Lag, true), refuse("replica_stalled"));
        assert_eq!(exhausted(Behind::Stopped, true), refuse("replica_stopped"));
        assert_eq!(exhausted(Behind::Error, false), refuse("replica_error"));
        assert_eq!(
            exhausted(Behind::Unreachable, false),
            refuse("replica_unreachable")
        );
        assert_eq!(
            exhausted(Behind::ReadOnly, false),
            refuse("replica_readonly")
        );
    }

    // ---- configuration and bootstrap ----

    #[test]
    fn read_replica_block_applies_defaults_and_refuses_zero_waits() {
        let block = json!({"host": "10.0.0.12", "port": 3306, "user": "oo_reader",
            "password_file": "/p/reader.pw", "ca_file": "/p/ca.pem"});
        let config = replica_config_from(&block).unwrap();
        assert_eq!((config.wait_timeout_s, config.retry_budget_s), (60, 1800));
        assert_eq!(config.tls_server_name, None);
        let named = json!({"host": "h", "port": 3306, "user": "u", "password_file": "p",
            "ca_file": "c", "tls_server_name": "localhost", "wait_timeout_s": 5, "retry_budget_s": 600});
        let config = replica_config_from(&named).unwrap();
        assert_eq!(
            (
                config.tls_server_name.as_deref(),
                config.wait_timeout_s,
                config.retry_budget_s
            ),
            (Some("localhost"), 5, 600)
        );
        for (key, value) in [
            ("wait_timeout_s", json!(0)),
            ("retry_budget_s", json!(0)),
            ("tls_server_name", json!("a b")),
        ] {
            let mut bad = named.clone();
            bad[key] = value;
            assert_eq!(
                replica_config_from(&bad).unwrap_err(),
                PreviewError::new("config", "read_replica"),
                "{key}"
            );
        }
        let mut no_host = block.clone();
        no_host.as_object_mut().unwrap().remove("host");
        assert!(replica_config_from(&no_host).is_err());
        let mut port_zero = block.clone();
        port_zero["port"] = json!(0);
        assert!(replica_config_from(&port_zero).is_err());
    }

    #[test]
    fn read_source_is_primary_unless_the_bootstrap_asks_for_a_replica() {
        assert_eq!(parse_read_source(None).unwrap(), ReadSource::Primary);
        assert_eq!(
            parse_read_source(Some(&json!({"kind": "primary"}))).unwrap(),
            ReadSource::Primary
        );
        assert_eq!(
            parse_read_source(Some(
                &json!({"kind": "replica", "replica_identity": REPLICA, "wait_timeout_s": 90})
            ))
            .unwrap(),
            ReadSource::Replica(ReplicaRequest {
                identity: Some(REPLICA.to_owned()),
                wait_timeout_s: Some(90)
            })
        );
        assert_eq!(
            parse_read_source(Some(&json!({"kind": "replica"}))).unwrap(),
            ReadSource::Replica(ReplicaRequest {
                identity: None,
                wait_timeout_s: None
            })
        );
        for bad in [
            json!({"kind": "replica", "replica_identity": "not-a-uuid"}),
            json!({"kind": "replica", "wait_timeout_s": 0}),
            json!({"kind": "replica", "wait_timeout_s": 999_999}),
            json!({"kind": "elsewhere"}),
        ] {
            assert_eq!(
                parse_read_source(Some(&bad)).unwrap_err(),
                PreviewError::new("config", "read_source"),
                "{bad}"
            );
        }
    }

    #[test]
    fn statement_version_patch_is_read_from_the_core_version() {
        assert_eq!(patch_of("8.0.22-log"), 22);
        assert_eq!(patch_of("5.7.44"), 44);
        assert_eq!(patch_of("8.4"), 0);
    }
}
