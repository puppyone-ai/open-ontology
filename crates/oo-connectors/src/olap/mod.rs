//! ClickHouse, Doris and StarRocks as [`SourceEngine`]s (S4-F-01), converged
//! from the `tools/{clickhouse,doris,starrocks}-preview` prototypes.
//!
//! All three are read over the MySQL wire protocol with one connection per
//! engine, so no driver beyond the existing `mysql` feature is linked.
//! Snapshot chunks are half-open keyset ranges compared by the source and
//! read in source order under the [`ReadBudget`]. Where a table has
//! partitions the ranges are aligned to them (ClickHouse `_partition_id`,
//! Doris/StarRocks single-column RANGE lower bounds); a partition larger than
//! the budget is cut further by sampled key boundaries. Partition refresh is
//! the incremental mode: [`SourceEngine::digest_range`] returns a content
//! digest per range, and a range whose digest changed is reread as a whole.
//! There is no log CDC. Consistency is `B`: each chunk is one statement and
//! there is no snapshot across chunks.

pub mod clickhouse;
pub mod mpp;
pub(crate) mod sql;
pub(crate) mod types;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use arrow_array::RecordBatch;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, SslOpts, Value};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::SourceError;
use crate::engine::{
    ChunkReceipt, ChunkSink, ChunkSpec, ChunkStrategy, ENGINE_PROTOCOL, EngineFamily,
    EngineVersion, KeyShape, KeyValue, ProbeReport, RangeDigest, ReadBudget, SourceEngine,
    TableChunkPlan, TableRef, Watermark,
};
use sql::{KeyKind, OrderCol, ident, table_sql, text_literal};
use types::{Col, Mapped};

type Result<T> = std::result::Result<T, SourceError>;

/// Rows of one metadata statement; catalogs above this fail closed.
const META_ROWS: usize = 100_000;
/// Plan revision of a fresh plan; deterministic so unchanged partitions keep
/// identical ranges across processes (C5 baselines compare plans).
const PLAN_REVISION: u32 = 1;
/// Partitions counted per statement.
const COUNT_BATCH: usize = 64;
/// Hash buckets of the key. A partition (or unpartitioned table) above the
/// row budget is cut at bucket boundaries, which depend only on its size
/// band, not on which rows changed, so refresh plans stay identical.
const BUCKETS: u64 = 4096;
const BUCKET_COLUMN: &str = "__oo_bucket";
const PARTITION_COLUMN: &str = "__oo_partition";
/// Above this many RANGE partitions the table is chunked by key only.
const MAX_ALIGNED_PARTITIONS: usize = 4096;

/// Caller trust anchor for TLS. The chain is always verified.
#[derive(Clone, Debug)]
pub struct OlapTls {
    pub ca_file: PathBuf,
    pub server_name: Option<String>,
}

/// Endpoint and account. The password is passed separately and zeroized.
#[derive(Clone, Debug)]
pub struct OlapConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// `None` is plaintext; callers allow that only on loopback fixtures.
    pub tls: Option<OlapTls>,
    pub connect_timeout_ms: u64,
}

/// Source-side work counters for resource receipts (T8).
#[derive(Clone, Debug, Default, Serialize)]
pub struct OlapStats {
    pub statements: u64,
    pub connects: u64,
    pub chunks_read: u64,
    pub rows_read: u64,
    pub digests: u64,
    pub busy_ms: u64,
    pub rest_ms: u64,
}

#[derive(Clone, Debug)]
enum Partitioning {
    None,
    /// ClickHouse `_partition_id`, compared bytewise.
    ClickhouseId,
    /// Doris/StarRocks single-column RANGE partitions on `column`, with
    /// lower bounds in source order and partition names. Order column 0
    /// maps each row to its partition's lower bound, so it is constant
    /// within a partition and bucket cuts apply inside one partition.
    Range {
        lowers: Vec<(KeyValue, String)>,
        prune: OrderCol,
    },
}

#[derive(Clone, Debug)]
struct TableMeta {
    /// `FROM` target including `FINAL` where needed.
    from: String,
    cols: Vec<Col>,
    order: Vec<OrderCol>,
    partitioning: Partitioning,
    key: KeyShape,
    excluded: Vec<String>,
}

/// Read-only OLAP source engine for one family.
pub struct OlapEngine {
    family: EngineFamily,
    config: OlapConfig,
    password: Zeroizing<String>,
    conn: Option<Conn>,
    budget: ReadBudget,
    version: Option<EngineVersion>,
    metas: BTreeMap<TableRef, TableMeta>,
    chunks: BTreeMap<(TableRef, u32), ChunkSpec>,
    next_id: BTreeMap<TableRef, u32>,
    stats: OlapStats,
}

fn cell(row: &Row, i: usize) -> Option<Vec<u8>> {
    match row.as_ref(i)? {
        Value::NULL => None,
        Value::Bytes(b) => Some(b.clone()),
        Value::Int(n) => Some(n.to_string().into_bytes()),
        Value::UInt(n) => Some(n.to_string().into_bytes()),
        _ => None,
    }
}

fn text(row: &Row, i: usize) -> Option<String> {
    cell(row, i).and_then(|b| String::from_utf8(b).ok())
}

fn named(row: &Row, name: &str) -> Option<String> {
    let i = row
        .columns_ref()
        .iter()
        .position(|c| c.name_str().eq_ignore_ascii_case(name))?;
    text(row, i)
}

fn driver_error(error: &mysql_async::Error) -> SourceError {
    match error {
        mysql_async::Error::Server(e) => match e.code {
            // ClickHouse ACCESS_DENIED; MySQL-protocol access denials.
            497 | 1044 | 1045 | 1142 | 1143 | 1227 => SourceError::privilege("olap_access_denied"),
            // ClickHouse TIMEOUT_EXCEEDED; MySQL max_execution_time.
            159 | 3024 => SourceError::budget("statement_timeout"),
            _ => SourceError::protocol("olap_query"),
        },
        mysql_async::Error::Io(_) => SourceError::connect("olap_io"),
        _ => SourceError::protocol("olap_driver"),
    }
}

fn ipc_digest(batch: &RecordBatch) -> Result<(u64, String)> {
    let mut writer = arrow_ipc::writer::StreamWriter::try_new(Vec::new(), &batch.schema())
        .map_err(|_| SourceError::protocol("arrow_ipc"))?;
    writer
        .write(batch)
        .map_err(|_| SourceError::protocol("arrow_ipc"))?;
    writer
        .finish()
        .map_err(|_| SourceError::protocol("arrow_ipc"))?;
    let bytes = writer
        .into_inner()
        .map_err(|_| SourceError::protocol("arrow_ipc"))?;
    Ok((bytes.len() as u64, format!("{:x}", Sha256::digest(&bytes))))
}

/// Chunks from sorted, distinct interior boundaries. Every chunk is
/// `[bounds[i-1], bounds[i])`; the first is open below, the last above.
fn chunks_from(
    table: &TableRef,
    first_id: u32,
    revision: u32,
    bounds: &[Vec<KeyValue>],
    estimates: &[Option<u64>],
) -> Vec<ChunkSpec> {
    (0..=bounds.len())
        .map(|i| ChunkSpec {
            table: table.clone(),
            plan_revision: revision,
            chunk_id: first_id + i as u32,
            lo: i.checked_sub(1).map(|j| bounds[j].clone()),
            hi: bounds.get(i).cloned(),
            estimated_rows: estimates.get(i).copied().flatten(),
        })
        .collect()
}

/// Source-computed bucket of the key columns, `0..BUCKETS`.
fn bucket_col(family: EngineFamily, keys: &[String]) -> Result<Option<OrderCol>> {
    if keys.is_empty() {
        return Ok(None);
    }
    let quoted = keys.iter().map(|k| ident(k)).collect::<Result<Vec<_>>>()?;
    let expr = if family == EngineFamily::Clickhouse {
        format!("(cityHash64({}) % {BUCKETS})", quoted.join(", "))
    } else {
        let args = quoted
            .iter()
            .map(|q| format!("IFNULL(CAST({q} AS STRING), '')"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("bitand(murmur_hash3_32({args}), {})", BUCKETS - 1)
    };
    Ok(Some(OrderCol {
        name: BUCKET_COLUMN.into(),
        sample: expr.clone(),
        expr,
        kind: KeyKind::Int,
        cast: String::new(),
    }))
}

/// Order column 0 of a RANGE-partitioned table: each row's partition lower
/// bound, newest partition first, so it is constant within a partition.
fn partition_col(lowers: &[(KeyValue, String)], prune: &OrderCol) -> Result<OrderCol> {
    let (first, _) = lowers
        .first()
        .ok_or_else(|| SourceError::catalog("partition_range"))?;
    let mut arms = Vec::new();
    for (value, _) in lowers.iter().skip(1).rev() {
        let lit = sql::literal(prune, value)?;
        arms.push(format!("WHEN {} >= {lit} THEN {lit}", prune.expr));
    }
    let first = sql::literal(prune, first)?;
    let expr = if arms.is_empty() {
        first
    } else {
        format!("(CASE {} ELSE {first} END)", arms.join(" "))
    };
    Ok(OrderCol {
        name: PARTITION_COLUMN.into(),
        sample: expr.clone(),
        expr,
        kind: prune.kind,
        cast: prune.cast.clone(),
    })
}

/// Partition pruning on the raw column, implied by the mapped bounds. NULL
/// partition values map to the first partition, which `hi` must keep.
fn pruning(
    prune: &OrderCol,
    lo: Option<&[KeyValue]>,
    hi: Option<&[KeyValue]>,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if let Some(v) = lo.and_then(|b| b.first()) {
        out.push(format!("{} >= {}", prune.expr, sql::literal(prune, v)?));
    }
    if let Some([v]) = hi {
        let lit = sql::literal(prune, v)?;
        out.push(format!("({e} < {lit} OR {e} IS NULL)", e = prune.expr));
    }
    Ok(out)
}

/// Power-of-two piece count for `rows` under `max`, with headroom for skew.
fn pieces(rows: u64, max: u64) -> u64 {
    if rows <= max {
        return 1;
    }
    (rows
        .saturating_mul(3)
        .div_ceil(max.max(1).saturating_mul(2)))
    .next_power_of_two()
    .min(BUCKETS)
}

fn bucket_bounds(prefix: &[KeyValue], pieces: u64) -> Vec<Vec<KeyValue>> {
    (1..pieces)
        .map(|k| {
            let mut b = prefix.to_vec();
            b.push(KeyValue::Int(i128::from(k * BUCKETS / pieces)));
            b
        })
        .collect()
}

fn key_shape(names: &[(String, KeyKind)]) -> KeyShape {
    match names {
        [] => KeyShape::NoKey,
        [(column, KeyKind::Int)] => KeyShape::IntegerPrimaryKey {
            column: column.clone(),
        },
        [(column, KeyKind::Text | KeyKind::Bytes)] => KeyShape::StringPrimaryKey {
            column: column.clone(),
            collation: None,
        },
        many => KeyShape::CompositePrimaryKey {
            columns: many.iter().map(|(n, _)| n.clone()).collect(),
        },
    }
}

impl OlapEngine {
    /// Construct without connecting. Only the three OLAP families are accepted.
    pub fn new(
        family: EngineFamily,
        config: OlapConfig,
        password: Zeroizing<String>,
        budget: ReadBudget,
    ) -> Result<Self> {
        if !matches!(
            family,
            EngineFamily::Clickhouse | EngineFamily::Doris | EngineFamily::Starrocks
        ) {
            return Err(SourceError::config("olap_family"));
        }
        if config.host.is_empty()
            || config.host.len() > 253
            || config
                .host
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
            || config.port == 0
            || config.user.is_empty()
            || ident(&config.database).is_err()
            || !(1..=60_000).contains(&config.connect_timeout_ms)
        {
            return Err(SourceError::config("olap_endpoint"));
        }
        if let Some(name) = config.tls.as_ref().and_then(|t| t.server_name.as_deref())
            && !crate::mysql::tls_server_name_ok(name)
        {
            return Err(SourceError::config("olap_tls_server_name"));
        }
        Ok(Self {
            family,
            config,
            password,
            conn: None,
            budget,
            version: None,
            metas: BTreeMap::new(),
            chunks: BTreeMap::new(),
            next_id: BTreeMap::new(),
            stats: OlapStats::default(),
        })
    }

    pub fn stats(&self) -> &OlapStats {
        &self.stats
    }

    /// Columns excluded from loads (aggregate states), per planned table.
    pub fn excluded_columns(&self, table: &TableRef) -> Vec<String> {
        self.metas
            .get(table)
            .map(|m| m.excluded.clone())
            .unwrap_or_default()
    }

    /// Graceful disconnect; the engine reconnects on next use.
    pub async fn close(&mut self) {
        if let Some(conn) = self.conn.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), conn.disconnect()).await;
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(self.budget.statement_timeout_ms.clamp(1_000, 600_000) + 5_000)
    }

    async fn ensure_conn(&mut self) -> Result<()> {
        if self.conn.is_some() {
            return Ok(());
        }
        let mut opts = OptsBuilder::default()
            .ip_or_hostname(self.config.host.clone())
            .tcp_port(self.config.port)
            .user(Some(self.config.user.clone()))
            .pass(Some(self.password.to_string()))
            // No default schema: every statement is fully qualified, and an
            // account without access to it must still reach the probe.
            .prefer_socket(false)
            .max_allowed_packet(Some(64 * 1024 * 1024))
            .wait_timeout(Some(28_800))
            .stmt_cache_size(0);
        if let Some(tls) = &self.config.tls {
            let meta = std::fs::metadata(&tls.ca_file).map_err(|_| SourceError::tls("ca_file"))?;
            if !meta.is_file() || meta.len() == 0 || meta.len() > 1_048_576 {
                return Err(SourceError::tls("ca_file"));
            }
            opts = opts.ssl_opts(
                SslOpts::default()
                    .with_root_certs(vec![tls.ca_file.clone().into()])
                    .with_danger_accept_invalid_certs(false)
                    .with_danger_skip_domain_validation(false)
                    .with_danger_tls_hostname_override(tls.server_name.clone())
                    .with_disable_built_in_roots(true),
            );
        }
        let conn = tokio::time::timeout(
            Duration::from_millis(self.config.connect_timeout_ms),
            Conn::new(opts),
        )
        .await
        .map_err(|_| SourceError::connect("connect_timeout"))?
        .map_err(|e| match driver_error(&e) {
            e if e.kind() == crate::SourceErrorKind::Privilege => {
                SourceError::new(crate::SourceErrorKind::Auth, Some("olap_auth"), None)
            }
            _ => SourceError::connect("olap_connect"),
        })?;
        let mut conn = conn;
        if let Some(set) = self.timeout_statement() {
            sql::guard(&set)?;
            self.stats.statements += 1;
            conn.query_drop(set).await.map_err(|e| driver_error(&e))?;
        }
        self.conn = Some(conn);
        self.stats.connects += 1;
        Ok(())
    }

    fn timeout_statement(&self) -> Option<String> {
        (self.family != EngineFamily::Clickhouse).then(|| {
            let secs = self
                .budget
                .statement_timeout_ms
                .div_ceil(1000)
                .clamp(1, 3_600);
            format!("SET query_timeout = {secs}")
        })
    }

    /// Doris/StarRocks take the budget's statement timeout per session.
    /// ClickHouse readonly profiles forbid SET; the profile's
    /// `max_execution_time` applies and a client deadline bounds the wait.
    async fn apply_timeout(&mut self) -> Result<()> {
        if let Some(set) = self.timeout_statement() {
            self.exec(&set).await?;
        }
        Ok(())
    }

    async fn drop_conn(&mut self) {
        // A half-read result cannot be reused; dropping closes the socket.
        self.conn = None;
    }

    /// One bounded statement. Rows above `max_rows`, raw bytes above
    /// `max_bytes` or the deadline drop the connection and fail closed.
    async fn query(&mut self, sql: &str, max_rows: usize, max_bytes: u64) -> Result<Vec<Row>> {
        sql::guard(sql)?;
        self.ensure_conn().await?;
        self.stats.statements += 1;
        let deadline = self.timeout();
        let conn = self.conn.as_mut().expect("connected");
        let work = async {
            let mut result = conn.query_iter(sql).await.map_err(|e| driver_error(&e))?;
            let mut rows = Vec::new();
            let mut bytes = 0u64;
            while let Some(row) = result.next().await.map_err(|e| driver_error(&e))? {
                bytes += (0..row.len())
                    .map(|i| match row.as_ref(i) {
                        Some(Value::Bytes(b)) => b.len() as u64 + 1,
                        _ => 1,
                    })
                    .sum::<u64>();
                rows.push(row);
                if rows.len() > max_rows || bytes > max_bytes {
                    return Ok((rows, true));
                }
            }
            Ok::<_, SourceError>((rows, false))
        };
        match tokio::time::timeout(deadline, work).await {
            Ok(Ok((rows, false))) => Ok(rows),
            Ok(Ok((_, true))) => {
                self.drop_conn().await;
                Err(SourceError::budget("chunk_over_budget"))
            }
            Ok(Err(e)) => {
                if e.kind() != crate::SourceErrorKind::Privilege {
                    self.drop_conn().await;
                }
                Err(e)
            }
            Err(_) => {
                self.drop_conn().await;
                Err(SourceError::budget("statement_timeout"))
            }
        }
    }

    async fn exec(&mut self, sql: &str) -> Result<Vec<Row>> {
        self.query(sql, META_ROWS, 64 * 1024 * 1024)
            .await
            .map_err(|e| {
                if e.fact() == Some("chunk_over_budget") {
                    SourceError::catalog("olap_metadata_too_large")
                } else {
                    e
                }
            })
    }

    /// Duty cycle: rest at least `(100 - duty) / duty` of the busy time.
    async fn rest(&mut self, started: Instant) {
        let busy = started.elapsed();
        let duty = u32::from(self.budget.duty_cycle_percent.clamp(1, 100));
        let pause = busy * (100 - duty) / duty;
        self.stats.busy_ms += busy.as_millis() as u64;
        self.stats.rest_ms += pause.as_millis() as u64;
        tokio::time::sleep(pause).await;
    }

    fn apply_budget(&mut self, budget: &ReadBudget) -> Result<bool> {
        if budget.max_chunk_rows == 0
            || budget.max_chunk_bytes == 0
            || !(1..=100).contains(&budget.duty_cycle_percent)
            || budget.statement_timeout_ms == 0
        {
            return Err(SourceError::config("olap_budget"));
        }
        let changed = budget.statement_timeout_ms != self.budget.statement_timeout_ms;
        self.budget = budget.clone();
        Ok(changed)
    }

    async fn use_budget(&mut self, budget: &ReadBudget) -> Result<()> {
        if self.apply_budget(budget)? && self.conn.is_some() {
            self.apply_timeout().await?;
        }
        Ok(())
    }

    async fn ensure_version(&mut self) -> Result<EngineVersion> {
        if let Some(v) = &self.version {
            return Ok(v.clone());
        }
        let version = match self.family {
            EngineFamily::Clickhouse => {
                let rows = self.exec("SELECT version()").await?;
                let raw = rows
                    .first()
                    .and_then(|r| text(r, 0))
                    .ok_or_else(|| SourceError::version("version_text"))?;
                clickhouse::parse_version(&raw)?
            }
            EngineFamily::Doris => {
                let rows = self.exec("SELECT @@version_comment").await?;
                let raw = rows
                    .first()
                    .and_then(|r| text(r, 0))
                    .ok_or_else(|| SourceError::version("version_text"))?;
                mpp::parse_doris(&raw)?
            }
            _ => {
                let rows = self
                    .exec("SELECT current_version(), @@version_comment")
                    .await?;
                let row = rows
                    .first()
                    .ok_or_else(|| SourceError::version("version_text"))?;
                mpp::parse_starrocks(
                    &text(row, 0).ok_or_else(|| SourceError::version("version_text"))?,
                    &text(row, 1).unwrap_or_default(),
                )?
            }
        };
        self.version = Some(version.clone());
        Ok(version)
    }

    async fn select_granted(&mut self) -> Result<bool> {
        let rows = self.exec("SHOW GRANTS").await?;
        let db = self.config.database.to_ascii_lowercase();
        Ok(rows.iter().any(|row| {
            (0..row.len()).any(|i| {
                let line = text(row, i).unwrap_or_default().to_ascii_lowercase();
                line.contains("select")
                    && (line.contains(&format!("{db}.*"))
                        || line.contains(&format!("`{db}`.*"))
                        || line.contains(&format!("internal.{db}:"))
                        || line.contains(&format!("in database {db} "))
                        || line.contains(&format!("in database `{db}` "))
                        || line.contains("on *.*")
                        || line.contains("in all databases"))
            })
        }))
    }

    async fn observe(&mut self) -> Result<ProbeReport> {
        let version = self.ensure_version().await?;
        let select_granted = self.select_granted().await?;
        let database = self.config.database.clone();
        if self.family == EngineFamily::Clickhouse {
            let settings = self
                .exec("SELECT name, value FROM system.settings WHERE name IN ('readonly', 'max_execution_time')")
                .await?;
            let get = |name: &str| {
                settings
                    .iter()
                    .find(|r| text(r, 0).as_deref() == Some(name))
                    .and_then(|r| text(r, 1))
            };
            let (readonly, max_execution_time) = (get("readonly"), get("max_execution_time"));
            let mutations_visible = match self
                .exec("SELECT count() FROM system.mutations WHERE 0")
                .await
            {
                Ok(_) => true,
                Err(e) if e.kind() == crate::SourceErrorKind::Privilege => false,
                Err(e) => return Err(e),
            };
            let account = self
                .exec("SELECT currentUser()")
                .await?
                .first()
                .and_then(|r| text(r, 0))
                .unwrap_or_default();
            return Ok(clickhouse::report(clickhouse::Observed {
                version,
                select_granted,
                readonly,
                max_execution_time,
                mutations_visible,
                account,
                database,
            }));
        }
        let row = self.exec("SELECT CURRENT_USER(), @@query_timeout").await?;
        let (account, query_timeout) = row
            .first()
            .map(|r| (text(r, 0).unwrap_or_default(), text(r, 1)))
            .unwrap_or_default();
        let account = account
            .split_once('@')
            .map(|(u, h)| {
                format!(
                    "'{}'@'{}'",
                    u.trim_matches(['\'', '`']),
                    h.trim_matches(['\'', '`'])
                )
            })
            .unwrap_or_else(|| format!("'{}'@'%'", account.trim_matches(['\'', '`'])));
        Ok(mpp::report(mpp::Observed {
            family: self.family,
            version,
            select_granted,
            account,
            database,
            query_timeout,
        }))
    }

    async fn meta(&mut self, table: &TableRef) -> Result<()> {
        if self.metas.contains_key(table) {
            return Ok(());
        }
        self.ensure_version().await?;
        let target = table_sql(&table.schema, &table.name)?;
        let meta = if self.family == EngineFamily::Clickhouse {
            self.clickhouse_meta(table, &target).await?
        } else {
            self.mpp_meta(table, &target).await?
        };
        self.metas.insert(table.clone(), meta);
        Ok(())
    }

    async fn clickhouse_meta(&mut self, table: &TableRef, target: &str) -> Result<TableMeta> {
        let (db, name) = (text_literal(&table.schema)?, text_literal(&table.name)?);
        let rows = self
            .exec(&format!(
                "SELECT engine, partition_key, sorting_key FROM system.tables WHERE database = {db} AND name = {name}"
            ))
            .await?;
        let row = rows
            .first()
            .ok_or_else(|| SourceError::catalog("table_not_found"))?;
        let engine = text(row, 0).unwrap_or_default();
        if !clickhouse::is_merge_tree(&engine) {
            return Err(SourceError::catalog("clickhouse_engine_not_merge_tree"));
        }
        let partitioned = !text(row, 1).unwrap_or_default().trim().is_empty();
        let sorting = clickhouse::sorting_columns(&text(row, 2).unwrap_or_default());
        let final_clause = clickhouse::needs_final(&engine);
        let rows = self
            .exec(&format!(
                "SELECT name, type FROM system.columns WHERE database = {db} AND table = {name} ORDER BY position"
            ))
            .await?;
        let (mut cols, mut excluded) = (Vec::new(), Vec::new());
        for row in &rows {
            let (n, t) = (
                text(row, 0).ok_or_else(|| SourceError::catalog("column_name"))?,
                text(row, 1).ok_or_else(|| SourceError::catalog("column_type"))?,
            );
            match types::clickhouse(&n, &t)? {
                Mapped::Col(c) => cols.push(c),
                Mapped::Excluded => excluded.push(n),
            }
        }
        if cols.is_empty() {
            return Err(SourceError::catalog("no_loadable_columns"));
        }
        let mut order = Vec::new();
        if partitioned {
            order.push(OrderCol {
                name: "_partition_id".into(),
                expr: "_partition_id".into(),
                sample: "_partition_id".into(),
                kind: KeyKind::Text,
                cast: String::new(),
            });
        }
        let mut keys = Vec::new();
        // ORDER BY tuple(): order by the comparable columns instead, so the
        // table still splits into source-ordered ranges.
        let fallback: Vec<String> = cols
            .iter()
            .filter(|c| c.key.is_some())
            .take(8)
            .map(|c| c.name.clone())
            .collect();
        let unique_sorting = !sorting.is_empty();
        for s in if unique_sorting { &sorting } else { &fallback } {
            let Some(c) = cols.iter().find(|c| &c.name == s) else {
                break;
            };
            let Some((kind, cast)) = c.key.clone() else {
                break;
            };
            let q = ident(&c.name)?;
            let sample = match kind {
                KeyKind::Int | KeyKind::Bytes => q.clone(),
                _ => format!("toString({q})"),
            };
            order.push(OrderCol {
                name: c.name.clone(),
                expr: q,
                sample,
                kind,
                cast,
            });
            keys.push((c.name.clone(), kind));
        }
        let hashed: Vec<String> = keys.iter().map(|(n, _)| n.clone()).collect();
        if let Some(bucket) = bucket_col(EngineFamily::Clickhouse, &hashed)? {
            order.insert(usize::from(partitioned), bucket);
        }
        // Keys are unique only after FINAL on replacing/collapsing engines.
        let key = if unique_sorting && final_clause && engine.contains("Replacing") {
            key_shape(&keys)
        } else {
            KeyShape::NoKey
        };
        Ok(TableMeta {
            from: if final_clause {
                format!("{target} FINAL")
            } else {
                target.to_string()
            },
            cols,
            order,
            partitioning: if partitioned {
                Partitioning::ClickhouseId
            } else {
                Partitioning::None
            },
            key,
            excluded,
        })
    }

    async fn mpp_meta(&mut self, table: &TableRef, target: &str) -> Result<TableMeta> {
        let desc = self.exec(&format!("DESC {target}")).await?;
        let (mut cols, mut excluded, mut key_names) = (Vec::new(), Vec::new(), Vec::new());
        for row in &desc {
            let name = named(row, "Field").ok_or_else(|| SourceError::catalog("column_name"))?;
            let ty = named(row, "Type").ok_or_else(|| SourceError::catalog("column_type"))?;
            let nullable = named(row, "Null").is_some_and(|n| n.eq_ignore_ascii_case("yes"));
            let is_key = named(row, "Key").is_some_and(|k| k.eq_ignore_ascii_case("true"));
            match types::mpp(&name, &ty, nullable, self.family == EngineFamily::Doris)? {
                Mapped::Col(c) => {
                    if is_key {
                        key_names.push(c.name.clone());
                    }
                    cols.push(c);
                }
                Mapped::Excluded => excluded.push(name),
            }
        }
        if cols.is_empty() {
            return Err(SourceError::catalog("no_loadable_columns"));
        }
        let (db, name) = (text_literal(&table.schema)?, text_literal(&table.name)?);
        let model = self
            .exec(&format!(
                "SELECT COLUMN_KEY FROM information_schema.columns WHERE table_schema = {db} AND table_name = {name} AND COLUMN_KEY <> '' LIMIT 1"
            ))
            .await?
            .first()
            .and_then(|r| text(r, 0))
            .unwrap_or_default()
            .to_ascii_uppercase();
        let parts = self.exec(&format!("SHOW PARTITIONS FROM {target}")).await?;
        let mut partitioning = Partitioning::None;
        let keys: Vec<String> = parts
            .iter()
            .filter_map(|r| named(r, "PartitionKey"))
            .collect();
        if !parts.is_empty()
            && parts.len() <= MAX_ALIGNED_PARTITIONS
            && keys.len() == parts.len()
            && keys.iter().all(|k| k == &keys[0])
        {
            let column = keys[0].trim().trim_matches('`').to_string();
            let mut lowers = Vec::new();
            let mut names = BTreeMap::new();
            for row in &parts {
                let (Some(lower), Some(name)) = (
                    named(row, "Range").as_deref().and_then(mpp::range_lower),
                    named(row, "PartitionName"),
                ) else {
                    lowers.clear();
                    break;
                };
                if ident(&name).is_err() || names.insert(lower.clone(), name).is_some() {
                    lowers.clear();
                    break;
                }
                lowers.push((lower, KeyValue::Null));
            }
            if let Some(c) = cols.iter().find(|c| c.name == column)
                && let Some((kind, cast)) = &c.key
                && (*kind == KeyKind::Int
                    || matches!(c.kind, types::Kind::Date | types::Kind::TsNaive))
                && !lowers.is_empty()
                && mpp::sort_lowers(*kind == KeyKind::Int, &mut lowers)
            {
                let prune = OrderCol {
                    name: column.clone(),
                    expr: ident(&column)?,
                    sample: ident(&column)?,
                    kind: *kind,
                    cast: cast.clone(),
                };
                partitioning = Partitioning::Range {
                    lowers: lowers
                        .into_iter()
                        .map(|(text, value)| (value, names[&text].clone()))
                        .collect(),
                    prune,
                };
            }
        }
        let mut order = Vec::new();
        let push = |c: &Col, order: &mut Vec<OrderCol>| -> Result<bool> {
            let Some((kind, cast)) = c.key.clone() else {
                return Ok(false);
            };
            let q = ident(&c.name)?;
            order.push(OrderCol {
                name: c.name.clone(),
                expr: q.clone(),
                sample: q,
                kind,
                cast,
            });
            Ok(true)
        };
        if let Partitioning::Range { lowers, prune } = &partitioning {
            order.push(partition_col(lowers, prune)?);
        }
        let mut shape = Vec::new();
        for k in &key_names {
            let c = cols.iter().find(|c| &c.name == k).expect("key column");
            if let Some((kind, _)) = &c.key {
                shape.push((c.name.clone(), *kind));
            }
            if order.iter().any(|o| &o.name == k) {
                continue;
            }
            if !push(c, &mut order)? {
                break;
            }
        }
        let at = usize::from(matches!(partitioning, Partitioning::Range { .. }));
        if let Some(bucket) = bucket_col(self.family, &key_names)? {
            order.insert(at, bucket);
        }
        let key = match model.as_str() {
            "UNI" | "PRI" if shape.len() == key_names.len() => key_shape(&shape),
            "AGG" => KeyShape::UniqueIndex {
                index: "aggregate_key".into(),
                columns: key_names.clone(),
            },
            _ => KeyShape::NoKey,
        };
        Ok(TableMeta {
            from: target.to_string(),
            cols,
            order,
            partitioning,
            key,
            excluded,
        })
    }

    fn meta_ref(&self, table: &TableRef) -> Result<&TableMeta> {
        self.metas
            .get(table)
            .ok_or_else(|| SourceError::catalog("table_not_planned"))
    }

    fn predicate(
        &self,
        meta: &TableMeta,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<String> {
        let mut parts: Vec<String> = sql::range(self.family, &meta.order, lo, hi)?
            .into_iter()
            .collect();
        if let Partitioning::Range { prune, .. } = &meta.partitioning {
            parts.extend(pruning(prune, lo, hi)?);
        }
        Ok(if parts.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", parts.join(" AND "))
        })
    }

    async fn count(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<u64> {
        let meta = self.meta_ref(table)?.clone();
        let sql = format!(
            "SELECT count(*) FROM {}{}",
            meta.from,
            self.predicate(&meta, lo, hi)?
        );
        let rows = self.exec(&sql).await?;
        rows.first()
            .and_then(|r| text(r, 0))
            .and_then(|t| t.parse().ok())
            .ok_or_else(|| SourceError::protocol("olap_count"))
    }

    fn key_from_row(meta: &TableMeta, row: &Row) -> Result<Option<Vec<KeyValue>>> {
        let mut out = Vec::with_capacity(meta.order.len());
        for (i, col) in meta.order.iter().enumerate() {
            let Some(bytes) = cell(row, i) else {
                return Ok(None);
            };
            out.push(match col.kind {
                KeyKind::Int => KeyValue::Int(
                    std::str::from_utf8(&bytes)
                        .ok()
                        .and_then(|t| t.parse().ok())
                        .ok_or_else(|| SourceError::protocol("olap_key_parse"))?,
                ),
                KeyKind::Decimal => KeyValue::Decimal(
                    String::from_utf8(bytes)
                        .map_err(|_| SourceError::protocol("olap_key_parse"))?,
                ),
                KeyKind::Text => KeyValue::Text(
                    String::from_utf8(bytes)
                        .map_err(|_| SourceError::protocol("olap_key_parse"))?,
                ),
                KeyKind::Bytes => match String::from_utf8(bytes) {
                    Ok(t) if !t.chars().any(char::is_control) => KeyValue::Text(t),
                    Ok(t) => KeyValue::Bytes(t.bytes().map(|b| format!("{b:02x}")).collect()),
                    Err(e) => {
                        KeyValue::Bytes(e.into_bytes().iter().map(|b| format!("{b:02x}")).collect())
                    }
                },
            });
        }
        Ok(Some(out))
    }

    /// Full-width key boundaries every `every` rows inside `[lo, hi)`, in
    /// source order, strictly inside the range and distinct.
    async fn sample(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
        every: u64,
    ) -> Result<Vec<Vec<KeyValue>>> {
        let meta = self.meta_ref(table)?.clone();
        if meta.order.is_empty() || every == 0 {
            return Ok(vec![]);
        }
        let aliases: Vec<String> = (0..meta.order.len())
            .map(|i| format!("__oo_k{i}"))
            .collect();
        let inner = meta
            .order
            .iter()
            .zip(&aliases)
            .map(|(c, a)| format!("{} AS {a}", c.sample))
            .collect::<Vec<_>>()
            .join(", ");
        let by = meta
            .order
            .iter()
            .map(|c| c.expr.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {} FROM (SELECT {inner}, row_number() OVER (ORDER BY {by}) AS __oo_rn FROM {}{}) AS __oo_s \
             WHERE (__oo_rn - 1) % {every} = 0 AND __oo_rn > 1 ORDER BY __oo_rn",
            aliases.join(", "),
            meta.from,
            self.predicate(&meta, lo, hi)?,
        );
        let rows = self.exec(&sql).await?;
        let mut out: Vec<Vec<KeyValue>> = Vec::new();
        for row in &rows {
            let Some(key) = Self::key_from_row(&meta, row)? else {
                continue;
            };
            if Some(key.as_slice()) == lo || out.last() == Some(&key) || Some(key.as_slice()) == hi
            {
                continue;
            }
            out.push(key);
        }
        Ok(out)
    }

    /// Partition lower bounds in source order with row estimates.
    async fn partitions(&mut self, table: &TableRef) -> Result<Vec<(Vec<KeyValue>, u64)>> {
        let meta = self.meta_ref(table)?.clone();
        let target = table_sql(&table.schema, &table.name)?;
        match &meta.partitioning {
            Partitioning::None => Ok(vec![]),
            Partitioning::ClickhouseId => {
                // Without FINAL: an upper-bound estimate from part metadata.
                let rows = self
                    .exec(&format!(
                        "SELECT _partition_id, count() FROM {target} GROUP BY _partition_id ORDER BY _partition_id"
                    ))
                    .await?;
                rows.iter()
                    .map(|r| {
                        Ok((
                            vec![KeyValue::Text(
                                text(r, 0).ok_or_else(|| SourceError::catalog("partition_id"))?,
                            )],
                            text(r, 1).and_then(|t| t.parse().ok()).unwrap_or(0),
                        ))
                    })
                    .collect()
            }
            Partitioning::Range { lowers, .. } => {
                let mut counts = BTreeMap::new();
                for batch in lowers.chunks(COUNT_BATCH) {
                    let sql = batch
                        .iter()
                        .map(|(_, n)| {
                            Ok(format!(
                                "SELECT {}, count(*) FROM {target} PARTITION ({})",
                                text_literal(n)?,
                                ident(n)?
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?
                        .join(" UNION ALL ");
                    for row in self.exec(&sql).await? {
                        counts.insert(
                            text(&row, 0).unwrap_or_default(),
                            text(&row, 1)
                                .and_then(|t| t.parse::<u64>().ok())
                                .unwrap_or(0),
                        );
                    }
                }
                Ok(lowers
                    .iter()
                    .map(|(value, name)| {
                        (vec![value.clone()], counts.get(name).copied().unwrap_or(0))
                    })
                    .collect())
            }
        }
    }

    async fn plan_inner(&mut self, table: &TableRef) -> Result<TableChunkPlan> {
        // Partitions may have changed since a previous plan in this session.
        self.metas.remove(table);
        self.meta(table).await?;
        let meta = self.meta_ref(table)?.clone();
        let max = self.budget.max_chunk_rows.clamp(1, 100_000);
        let mut bounds: Vec<Vec<KeyValue>> = Vec::new();
        let mut estimates: Vec<Option<u64>> = Vec::new();
        let parts = self.partitions(table).await?;
        let total;
        if !parts.is_empty() {
            total = parts.iter().map(|(_, n)| *n).sum::<u64>();
            for (i, (lower, rows)) in parts.iter().enumerate() {
                let lo = (i > 0).then_some(lower.as_slice());
                let hi = parts.get(i + 1).map(|(b, _)| b.as_slice());
                if i > 0 {
                    bounds.push(lower.clone());
                }
                let inner = self.cut(table, &meta, lower, lo, hi, *rows, max).await?;
                let pieces = inner.len() as u64 + 1;
                for _ in 0..pieces {
                    estimates.push(Some(rows.div_ceil(pieces)));
                }
                bounds.extend(inner);
            }
        } else {
            total = self.count(table, None, None).await?;
            bounds = self.cut(table, &meta, &[], None, None, total, max).await?;
            let pieces = bounds.len() as u64 + 1;
            estimates = vec![Some(total.div_ceil(pieces)); pieces as usize];
        }
        let chunks = chunks_from(table, 0, PLAN_REVISION, &bounds, &estimates);
        self.chunks.retain(|(t, _), _| t != table);
        for c in &chunks {
            self.chunks.insert((table.clone(), c.chunk_id), c.clone());
        }
        self.next_id.insert(table.clone(), chunks.len() as u32);
        Ok(TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table: table.clone(),
            key: meta.key.clone(),
            strategy: if meta.order.is_empty() {
                ChunkStrategy::SingleRead
            } else {
                ChunkStrategy::Keyset {
                    columns: meta.order.iter().map(|c| c.name.clone()).collect(),
                }
            },
            estimated_rows: Some(total),
            chunks,
        })
    }

    /// Interior bounds of one partition (or the table) of `rows` rows:
    /// stable bucket boundaries where a bucket column exists, else sampled.
    #[allow(clippy::too_many_arguments)]
    async fn cut(
        &mut self,
        table: &TableRef,
        meta: &TableMeta,
        prefix: &[KeyValue],
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
        rows: u64,
        max: u64,
    ) -> Result<Vec<Vec<KeyValue>>> {
        if rows <= max {
            return Ok(vec![]);
        }
        match meta.order.get(prefix.len()) {
            Some(c) if c.name == BUCKET_COLUMN => Ok(bucket_bounds(prefix, pieces(rows, max))),
            _ => self.sample(table, lo, hi, max).await,
        }
    }

    fn registered(&self, chunk: &ChunkSpec) -> Result<()> {
        match self.chunks.get(&(chunk.table.clone(), chunk.chunk_id)) {
            Some(known) if known == chunk => Ok(()),
            _ => Err(SourceError::config("chunk_not_registered")),
        }
    }

    async fn mutations_pending(&mut self, table: &TableRef) -> Result<()> {
        if self.family != EngineFamily::Clickhouse {
            return Ok(());
        }
        let sql = format!(
            "SELECT count() FROM system.mutations WHERE database = {} AND table = {} AND NOT is_done",
            text_literal(&table.schema)?,
            text_literal(&table.name)?
        );
        match self.exec(&sql).await {
            Ok(rows) if rows.first().and_then(|r| text(r, 0)).as_deref() == Some("0") => Ok(()),
            Ok(_) => Err(SourceError::catalog("deletes_lagged_mutation_pending")),
            // Probe reports PartitionRefresh as NeedsSetup without this grant.
            Err(e) if e.kind() == crate::SourceErrorKind::Privilege => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn read_inner(&mut self, chunk: &ChunkSpec) -> Result<(ChunkReceipt, RecordBatch)> {
        self.registered(chunk)?;
        // A snapshot under writes is consistency B and is corrected by later
        // range digests, so a pending mutation only blocks digests (C4/C5).
        let meta = self.meta_ref(&chunk.table)?.clone();
        let max_rows = self.budget.max_chunk_rows.clamp(1, 100_000);
        let max_bytes = self.budget.max_chunk_bytes.clamp(1, 32 * 1024 * 1024);
        let selects = meta
            .cols
            .iter()
            .map(|c| c.select.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let mut by: Vec<String> = meta.order.iter().map(|c| c.expr.clone()).collect();
        for c in &meta.cols {
            if let Some(o) = &c.order
                && !meta.order.iter().any(|k| k.name == c.name)
            {
                by.push(o.clone());
            }
        }
        let sql = format!(
            "SELECT {selects} FROM {}{}{} LIMIT {}",
            meta.from,
            self.predicate(&meta, chunk.lo.as_deref(), chunk.hi.as_deref())?,
            if by.is_empty() {
                String::new()
            } else {
                format!(" ORDER BY {}", by.join(", "))
            },
            max_rows + 1
        );
        let rows = self.query(&sql, max_rows as usize, max_bytes).await?;
        let cells: Vec<Vec<Option<Vec<u8>>>> = rows
            .iter()
            .map(|r| (0..meta.cols.len()).map(|i| cell(r, i)).collect())
            .collect();
        let batch = types::batch(&meta.cols, &cells)?;
        let (bytes, sha256) = ipc_digest(&batch)?;
        if bytes > max_bytes {
            return Err(SourceError::budget("chunk_ipc_over_budget"));
        }
        self.stats.chunks_read += 1;
        self.stats.rows_read += rows.len() as u64;
        let receipt = ChunkReceipt {
            table: chunk.table.clone(),
            plan_revision: chunk.plan_revision,
            chunk_id: chunk.chunk_id,
            rows: rows.len() as u64,
            bytes,
            sha256,
            low: None,
            high: None,
        };
        Ok((receipt, batch))
    }

    async fn digest_inner(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest> {
        self.meta(table).await?;
        self.mutations_pending(table).await?;
        let meta = self.meta_ref(table)?.clone();
        let filter = self.predicate(&meta, lo, hi)?;
        let sql = if self.family == EngineFamily::Clickhouse {
            let row = meta
                .cols
                .iter()
                .map(|c| c.select.clone())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "SELECT count(), sum(__oo_h), groupBitXor(__oo_h) FROM (SELECT cityHash64(toString(tuple({row}))) AS __oo_h FROM {}{filter})",
                meta.from
            )
        } else {
            let framed = meta
                .cols
                .iter()
                .map(|c| {
                    let v = &c.digest;
                    format!("IFNULL(CONCAT(CAST(LENGTH({v}) AS STRING), ':', {v}), '~')")
                })
                .collect::<Vec<_>>()
                .join(", ");
            let (a, b) = mpp::digest_functions(self.family);
            format!(
                "SELECT count(*), SUM(CAST({a}(__oo_r) AS LARGEINT)), SUM(CAST({b}(__oo_r) AS LARGEINT)) \
                 FROM (SELECT CONCAT({framed}) AS __oo_r FROM {}{filter}) AS __oo_d",
                meta.from
            )
        };
        let rows = self.exec(&sql).await?;
        let row = rows
            .first()
            .ok_or_else(|| SourceError::protocol("olap_digest"))?;
        let count: u64 = text(row, 0)
            .and_then(|t| t.parse().ok())
            .ok_or_else(|| SourceError::protocol("olap_digest"))?;
        let part = |i| text(row, i).unwrap_or_else(|| "-".into());
        self.stats.digests += 1;
        Ok(RangeDigest {
            table: table.clone(),
            lo: lo.map(<[KeyValue]>::to_vec),
            hi: hi.map(<[KeyValue]>::to_vec),
            rows: count,
            digest: format!("{:?}-c5v1:{count}:{}:{}", self.family, part(1), part(2))
                .to_ascii_lowercase(),
        })
    }

    async fn split_inner(&mut self, chunk: &ChunkSpec) -> Result<Option<Vec<ChunkSpec>>> {
        self.registered(chunk)?;
        let rows = self
            .count(&chunk.table, chunk.lo.as_deref(), chunk.hi.as_deref())
            .await?;
        if rows < 2 {
            return Ok(None);
        }
        let every = rows.div_ceil(2).min(self.budget.max_chunk_rows.max(1));
        let inner = self
            .sample(
                &chunk.table,
                chunk.lo.as_deref(),
                chunk.hi.as_deref(),
                every,
            )
            .await?;
        if inner.is_empty() {
            return Ok(None);
        }
        let first = *self.next_id.get(&chunk.table).unwrap_or(&0);
        let pieces = inner.len() as u64 + 1;
        let mut children = chunks_from(
            &chunk.table,
            first,
            chunk.plan_revision + 1,
            &inner,
            &vec![Some(rows.div_ceil(pieces)); pieces as usize],
        );
        children.first_mut().expect("child").lo = chunk.lo.clone();
        children.last_mut().expect("child").hi = chunk.hi.clone();
        self.next_id
            .insert(chunk.table.clone(), first + children.len() as u32);
        for c in &children {
            self.chunks.insert((c.table.clone(), c.chunk_id), c.clone());
        }
        Ok(Some(children))
    }

    async fn watermark_inner(&mut self) -> Result<Option<Watermark>> {
        if self.family == EngineFamily::Clickhouse || self.metas.is_empty() {
            return Ok(None);
        }
        let tables: Vec<TableRef> = self.metas.keys().cloned().collect();
        let mut hasher = Sha256::new();
        for table in tables {
            let rows = self
                .exec(&format!(
                    "SHOW PARTITIONS FROM {}",
                    table_sql(&table.schema, &table.name)?
                ))
                .await?;
            let mut versions: Vec<(String, String)> = rows
                .iter()
                .map(|r| {
                    (
                        named(r, "PartitionName").unwrap_or_default(),
                        named(r, "VisibleVersion").unwrap_or_default(),
                    )
                })
                .collect();
            versions.sort();
            hasher.update(
                serde_json::to_vec(&(&table, versions))
                    .map_err(|_| SourceError::protocol("watermark"))?,
            );
        }
        Ok(Some(Watermark::Opaque {
            value: format!(
                "{:?}-visible-versions-v1:{:x}",
                self.family,
                hasher.finalize()
            )
            .to_ascii_lowercase(),
        }))
    }

    /// Table incarnation and column fingerprint. A recreated table or a
    /// schema change yields a different epoch, which forces a new baseline.
    pub async fn source_epoch(&mut self, tables: &[TableRef]) -> Result<String> {
        let started = Instant::now();
        let result = self.epoch_inner(tables).await;
        self.rest(started).await;
        result
    }

    async fn epoch_inner(&mut self, tables: &[TableRef]) -> Result<String> {
        let version = self.ensure_version().await?;
        let mut hasher = Sha256::new();
        hasher.update(format!(
            "{:?}:{}.{}",
            self.family, version.major, version.minor
        ));
        for table in tables {
            let (db, name) = (text_literal(&table.schema)?, text_literal(&table.name)?);
            let identity = if self.family == EngineFamily::Clickhouse {
                self.exec(&format!(
                    "SELECT toString(uuid), toString(metadata_modification_time) FROM system.tables WHERE database = {db} AND name = {name}"
                ))
                .await?
            } else {
                self.exec(&format!(
                    "SELECT CAST(CREATE_TIME AS STRING) FROM information_schema.tables WHERE table_schema = {db} AND table_name = {name}"
                ))
                .await?
            };
            let identity: Vec<Vec<Option<String>>> = identity
                .iter()
                .map(|r| (0..r.len()).map(|i| text(r, i)).collect())
                .collect();
            if identity.len() != 1 {
                return Err(SourceError::catalog("table_not_found"));
            }
            let columns = if self.family == EngineFamily::Clickhouse {
                self.exec(&format!(
                    "SELECT name, type FROM system.columns WHERE database = {db} AND table = {name} ORDER BY position"
                ))
                .await?
            } else {
                self.exec(&format!("DESC {}", table_sql(&table.schema, &table.name)?))
                    .await?
            };
            let columns: Vec<Vec<Option<String>>> = columns
                .iter()
                .map(|r| (0..r.len().min(4)).map(|i| text(r, i)).collect())
                .collect();
            hasher.update(
                serde_json::to_vec(&(table, identity, columns))
                    .map_err(|_| SourceError::protocol("epoch"))?,
            );
        }
        Ok(format!("{:?}-epoch-v1:{:x}", self.family, hasher.finalize()).to_ascii_lowercase())
    }
}

impl SourceEngine for OlapEngine {
    fn family(&self) -> EngineFamily {
        self.family
    }

    async fn discover_tables(&mut self, budget: &ReadBudget) -> Result<Vec<TableRef>> {
        self.use_budget(budget).await?;
        let started = Instant::now();
        let db = text_literal(&self.config.database)?;
        let sql = if self.family == EngineFamily::Clickhouse {
            format!(
                "SELECT database, name FROM system.tables WHERE database = {db} AND engine LIKE '%MergeTree' AND NOT is_temporary ORDER BY name LIMIT 10000"
            )
        } else {
            format!(
                "SELECT table_schema, table_name FROM information_schema.tables WHERE table_schema = {db} AND table_type = 'BASE TABLE' ORDER BY table_name LIMIT 10000"
            )
        };
        let result = self.exec(&sql).await.map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    Some(TableRef {
                        schema: text(r, 0)?,
                        name: text(r, 1)?,
                    })
                })
                .collect()
        });
        self.rest(started).await;
        result
    }

    async fn probe(&mut self) -> Result<ProbeReport> {
        let started = Instant::now();
        let result = self.observe().await;
        self.rest(started).await;
        result
    }

    async fn plan_chunks(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<TableChunkPlan> {
        self.use_budget(budget).await?;
        let started = Instant::now();
        let result = self.plan_inner(table).await;
        self.rest(started).await;
        result
    }

    async fn read_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt> {
        self.use_budget(budget).await?;
        let started = Instant::now();
        let result = self.read_inner(chunk).await;
        self.rest(started).await;
        let (receipt, batch) = result?;
        sink.push(batch)?;
        Ok(receipt)
    }

    async fn split_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
    ) -> Result<Option<Vec<ChunkSpec>>> {
        self.use_budget(budget).await?;
        let started = Instant::now();
        let result = self.split_inner(chunk).await;
        self.rest(started).await;
        result
    }

    async fn watermark(&mut self) -> Result<Option<Watermark>> {
        let started = Instant::now();
        let result = self.watermark_inner().await;
        self.rest(started).await;
        result
    }

    async fn digest_range(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest> {
        let started = Instant::now();
        let result = self.digest_inner(table, lo, hi).await;
        self.rest(started).await;
        result
    }
}

/// Every family's version rules, for `oo doctor` and the support matrix.
pub fn version_rules() -> Vec<crate::engine::VersionRule> {
    let mut rules = clickhouse::version_rules();
    rules.extend(mpp::version_rules());
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> TableRef {
        TableRef {
            schema: "fx".into(),
            name: "events".into(),
        }
    }

    #[test]
    fn chunks_cover_the_key_space_in_order() {
        let p = |s: &str| vec![KeyValue::Text(s.into())];
        let bounds = vec![
            p("202402"),
            vec![KeyValue::Text("202402".into()), KeyValue::Int(50)],
            p("202403"),
        ];
        let chunks = chunks_from(
            &t(),
            0,
            PLAN_REVISION,
            &bounds,
            &[Some(1), Some(2), Some(3), Some(4)],
        );
        assert_eq!(chunks.len(), 4);
        assert!(chunks[0].lo.is_none() && chunks[3].hi.is_none());
        for w in chunks.windows(2) {
            assert_eq!(w[0].hi, w[1].lo);
        }
        assert_eq!(
            chunks.iter().map(|c| c.chunk_id).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(chunks[2].estimated_rows, Some(3));
        let single = chunks_from(&t(), 0, PLAN_REVISION, &[], &[Some(9)]);
        assert_eq!(
            (single.len(), single[0].lo.is_none(), single[0].hi.is_none()),
            (1, true, true)
        );
    }

    #[test]
    fn construction_refuses_other_families_and_bad_endpoints() {
        let config = OlapConfig {
            host: "127.0.0.1".into(),
            port: 9004,
            database: "fx".into(),
            user: "reader".into(),
            tls: None,
            connect_timeout_ms: 5_000,
        };
        let pw = || Zeroizing::new(String::from("x"));
        assert!(
            OlapEngine::new(
                EngineFamily::Mysql,
                config.clone(),
                pw(),
                ReadBudget::default()
            )
            .is_err()
        );
        let mut bad = config.clone();
        bad.database = "fx`; --".into();
        assert!(OlapEngine::new(EngineFamily::Doris, bad, pw(), ReadBudget::default()).is_err());
        let e =
            OlapEngine::new(EngineFamily::Starrocks, config, pw(), ReadBudget::default()).unwrap();
        assert_eq!(e.family(), EngineFamily::Starrocks);
        assert!(
            e.registered(&chunks_from(&t(), 0, 1, &[], &[None])[0])
                .is_err()
        );
    }

    #[test]
    fn bucket_cuts_depend_only_on_size_band() {
        assert_eq!(pieces(4_000, 4_000), 1);
        assert_eq!(pieces(4_001, 4_000), 2);
        assert_eq!(pieces(10_000, 4_000), 4);
        assert_eq!(pieces(10_100, 4_000), 4);
        assert_eq!(pieces(u64::MAX, 1), BUCKETS);
        let p = [KeyValue::Text("202402".into())];
        let b = bucket_bounds(&p, 4);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0], vec![p[0].clone(), KeyValue::Int(1024)]);
        assert_eq!(b[2][1], KeyValue::Int(3072));
        let ch = bucket_col(EngineFamily::Clickhouse, &["d".into(), "id".into()])
            .unwrap()
            .unwrap();
        assert_eq!(ch.expr, "(cityHash64(`d`, `id`) % 4096)");
        let sr = bucket_col(EngineFamily::Starrocks, &["id".into()])
            .unwrap()
            .unwrap();
        assert_eq!(
            sr.expr,
            "bitand(murmur_hash3_32(IFNULL(CAST(`id` AS STRING), '')), 4095)"
        );
        assert!(bucket_col(EngineFamily::Doris, &[]).unwrap().is_none());
    }

    #[test]
    fn range_partitions_map_rows_to_their_lower_bound() {
        let prune = OrderCol {
            name: "d".into(),
            expr: "`d`".into(),
            sample: "`d`".into(),
            kind: KeyKind::Text,
            cast: "date".into(),
        };
        let lowers: Vec<(KeyValue, String)> = ["2024-01-01", "2024-02-01", "2024-03-01"]
            .iter()
            .map(|d| (KeyValue::Text((*d).into()), format!("p{}", &d[..7])))
            .collect();
        let col = partition_col(&lowers, &prune).unwrap();
        assert_eq!(
            col.expr,
            "(CASE WHEN `d` >= CAST('2024-03-01' AS date) THEN CAST('2024-03-01' AS date) \
             WHEN `d` >= CAST('2024-02-01' AS date) THEN CAST('2024-02-01' AS date) \
             ELSE CAST('2024-01-01' AS date) END)"
        );
        assert_eq!(col.name, PARTITION_COLUMN);
        assert_eq!(
            partition_col(&lowers[..1], &prune).unwrap().expr,
            "CAST('2024-01-01' AS date)"
        );
        assert!(partition_col(&[], &prune).is_err());
        let feb = [KeyValue::Text("2024-02-01".into())];
        let sub = [KeyValue::Text("2024-02-01".into()), KeyValue::Int(1024)];
        assert_eq!(
            pruning(&prune, Some(&feb), Some(&sub)).unwrap(),
            ["`d` >= CAST('2024-02-01' AS date)"]
        );
        assert_eq!(
            pruning(&prune, None, Some(&feb)).unwrap(),
            ["(`d` < CAST('2024-02-01' AS date) OR `d` IS NULL)"]
        );
    }

    #[test]
    fn key_shapes_follow_unique_keys() {
        assert_eq!(key_shape(&[]), KeyShape::NoKey);
        assert_eq!(
            key_shape(&[("id".into(), KeyKind::Int)]),
            KeyShape::IntegerPrimaryKey {
                column: "id".into()
            }
        );
        assert!(matches!(
            key_shape(&[("d".into(), KeyKind::Text), ("id".into(), KeyKind::Int)]),
            KeyShape::CompositePrimaryKey { .. }
        ));
        assert_eq!(
            version_rules()
                .iter()
                .filter(|r| r.family == EngineFamily::Clickhouse)
                .count(),
            18
        );
    }
}
