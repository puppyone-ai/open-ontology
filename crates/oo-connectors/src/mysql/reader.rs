//! One page is pulled only when requested. Existing C1 snapshot transactions can
//! use this same reader without opening a second business snapshot.
use super::{
    MysqlConfig, MysqlError,
    map::{MappedColumn, StoreKind},
    raw_cell,
    values::{self, RawCell},
};
use crate::types::{RunBinding, TypedBatch, assemble_batch};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row, Value};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{sync::Notify, time::timeout};

type Result<T> = std::result::Result<T, MysqlError>;
const BATCH_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
    wake: Arc<Notify>,
}
impl Cancel {
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

pub struct Batch {
    pub typed: TypedBatch,
    /// Raw driver values are retained for the existing C1 frame adapter only.
    /// They have the same bounded row/byte lifetime as this Arrow batch.
    pub rows: Vec<Row>,
    pub bytes: u64,
    pub rejects: Vec<(usize, usize, values::RejectNote)>,
    pub statement: super::StatementNote,
}

pub struct TableReader {
    schema: String,
    table: String,
    columns: Vec<MappedColumn>,
    binding: RunBinding,
    batch_rows: u64,
    max_rows: u64,
    max_bytes: u64,
    rows: u64,
    bytes: u64,
    deadline: Instant,
    statement_timeout: Duration,
    last: Option<mysql_async::Value>,
    done: bool,
}
impl TableReader {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema: &str,
        table: &str,
        columns: Vec<MappedColumn>,
        binding: RunBinding,
        batch_rows: u64,
        max_rows: u64,
        max_bytes: u64,
        deadline_ms: u64,
        statement_ms: u64,
    ) -> Result<Self> {
        Self::new_inner(
            schema,
            table,
            columns,
            binding,
            batch_rows,
            max_rows,
            max_bytes,
            deadline_ms,
            statement_ms,
            true,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn new_inner(
        schema: &str,
        table: &str,
        columns: Vec<MappedColumn>,
        binding: RunBinding,
        batch_rows: u64,
        max_rows: u64,
        max_bytes: u64,
        deadline_ms: u64,
        statement_ms: u64,
        single_key: bool,
    ) -> Result<Self> {
        if !super::mysql_ident(schema)
            || !super::mysql_ident(table)
            || columns.is_empty()
            || columns.iter().any(|c| !super::mysql_ident(&c.plan.name))
            || (single_key && columns.iter().filter(|c| c.key).count() != 1)
            || !(1..=10000).contains(&batch_rows)
            || !(1..=1000000).contains(&max_rows)
            || !(1..=33554432).contains(&max_bytes)
            || !(1..=300000).contains(&deadline_ms)
            || !(1..=30000).contains(&statement_ms)
        {
            return Err(MysqlError::new("config", "read_budget"));
        }
        Ok(Self {
            schema: schema.to_owned(),
            table: table.to_owned(),
            columns,
            binding,
            batch_rows,
            max_rows,
            max_bytes,
            rows: 0,
            bytes: 0,
            deadline: Instant::now() + Duration::from_millis(deadline_ms),
            statement_timeout: Duration::from_millis(statement_ms),
            last: None,
            done: false,
        })
    }
    pub fn after_integer_key(&mut self, key: u64) -> Result<()> {
        if self.rows != 0 || self.last.is_some() || self.done {
            return Err(MysqlError::new("config", "cursor_started"));
        }
        self.last = Some(mysql_async::Value::UInt(key));
        Ok(())
    }
    async fn next(&mut self, conn: &mut Conn, cancel: &Cancel) -> Result<Option<Batch>> {
        if cancel.is_cancelled() {
            return Err(MysqlError::new("cancel", "requested"));
        }
        if self.done {
            return Ok(None);
        }
        let duration = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| MysqlError::new("budget", "snapshot_deadline"))?;
        let result = tokio::select! {
            biased;
            _=cancel.wake.notified()=>Err(MysqlError::new("cancel","requested")),
            result=timeout(duration, self.pull(conn))=>result.map_err(|_|MysqlError::new("budget","snapshot_deadline"))?,
        };
        let result = if cancel.is_cancelled() {
            Err(MysqlError::new("cancel", "requested"))
        } else if Instant::now() > self.deadline {
            Err(MysqlError::new("budget", "snapshot_deadline"))
        } else {
            result
        };
        if result.is_err() {
            self.done = true;
        }
        result
    }
    async fn pull(&mut self, conn: &mut Conn) -> Result<Option<Batch>> {
        let key = self
            .columns
            .iter()
            .find(|c| c.key)
            .ok_or_else(|| MysqlError::new("catalog", "primary_key"))?;
        let key_index = self
            .columns
            .iter()
            .position(|c| c.key)
            .ok_or_else(|| MysqlError::new("catalog", "primary_key"))?;
        // Mapping permits only exact integer keys. SQL collation therefore cannot merge chunks.
        if !matches!(
            key.store,
            super::map::StoreKind::Int { .. } | super::map::StoreKind::Numeric { scale: 0, .. }
        ) {
            return Err(MysqlError::new("catalog", "key_order"));
        }
        let list = self
            .columns
            .iter()
            .map(|c| format!("`{}`", c.plan.name))
            .collect::<Vec<_>>()
            .join(",");
        let bound = if matches!(key.store, super::map::StoreKind::Numeric { scale: 0, .. }) {
            "CAST(? AS DECIMAL(65,0))"
        } else {
            "?"
        };
        let predicate = if self.last.is_some() {
            format!(" WHERE `{}` > {bound}", key.plan.name)
        } else {
            String::new()
        };
        let sql = format!(
            "SELECT {list} FROM `{}`.`{}`{predicate} ORDER BY `{}` LIMIT ?",
            self.schema, self.table, key.plan.name
        );
        let limit = self
            .batch_rows
            .min(self.max_rows.saturating_sub(self.rows).max(1));
        let mut result = if let Some(last) = &self.last {
            timeout(
                self.statement_timeout,
                conn.exec_iter(&sql, (last.clone(), limit)),
            )
            .await
        } else {
            timeout(self.statement_timeout, conn.exec_iter(&sql, (limit,))).await
        }
        .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
        .map_err(|_| MysqlError::new("protocol", "query"))?;
        let mut rows = Vec::new();
        let mut page_bytes = 0u64;
        let mut eof = false;
        loop {
            let row = timeout(self.statement_timeout, result.next())
                .await
                .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
                .map_err(|_| MysqlError::new("protocol", "query"))?;
            let Some(row) = row else {
                eof = true;
                break;
            };
            if self.rows + rows.len() as u64 >= self.max_rows {
                return Err(MysqlError::new("budget", "max_rows"));
            }
            let mut size = 0u64;
            for index in 0..self.columns.len() {
                size = size.saturating_add(match raw_cell(&row, index)? {
                    RawCell::Bytes(v) => v.len() as u64,
                    _ => 8,
                });
            }
            if size > BATCH_BYTES {
                return Err(MysqlError::new("budget", "batch_bytes"));
            }
            if page_bytes + size > BATCH_BYTES {
                break;
            }
            if self.bytes + page_bytes + size > self.max_bytes {
                return Err(MysqlError::new("budget", "max_bytes"));
            }
            page_bytes += size;
            rows.push(row);
        }
        // Drain at most the SQL LIMIT, never collect the remainder. A page cut at
        // the byte cap re-reads the un-emitted key on the same snapshot next time.
        while timeout(self.statement_timeout, result.next())
            .await
            .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
            .map_err(|_| MysqlError::new("protocol", "query"))?
            .is_some()
        {}
        if rows.is_empty() {
            self.done = true;
            if self.rows > 0 {
                return Ok(None);
            }
        }
        if !rows.is_empty() {
            let last = raw_cell(
                rows.last()
                    .ok_or_else(|| MysqlError::new("protocol", "key"))?,
                key_index,
            )?;
            self.last = Some(match last {
                RawCell::Int(v) => mysql_async::Value::Int(v),
                RawCell::Uint(v) => mysql_async::Value::UInt(v),
                RawCell::Bytes(v) => mysql_async::Value::Bytes(v),
                _ => return Err(MysqlError::new("value", "key")),
            });
        }
        let batch = encode_batch(&self.columns, &self.binding, rows, page_bytes, &sql)?;
        self.rows += batch.rows.len() as u64;
        self.bytes += page_bytes;
        self.done |= eof && (batch.rows.len() as u64) < limit;
        Ok(Some(batch))
    }
}

pub(crate) fn encode_batch(
    columns: &[MappedColumn],
    binding: &RunBinding,
    rows: Vec<Row>,
    bytes: u64,
    sql: &str,
) -> Result<Batch> {
    encode_rows(columns, binding, rows, bytes, sql, false)
}

pub(crate) fn encode_chunk_batch(
    columns: &[MappedColumn],
    binding: &RunBinding,
    rows: Vec<Row>,
    bytes: u64,
    sql: &str,
) -> Result<Batch> {
    encode_rows(columns, binding, rows, bytes, sql, true)
}

fn encode_rows(
    columns: &[MappedColumn],
    binding: &RunBinding,
    rows: Vec<Row>,
    bytes: u64,
    sql: &str,
    mask_rejects: bool,
) -> Result<Batch> {
    let mut decoded = Vec::with_capacity(rows.len());
    let mut rejects = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        let mut cells = Vec::with_capacity(columns.len());
        for (column_index, c) in columns.iter().enumerate() {
            let raw = raw_cell(row, column_index)?;
            let decoded = values::decode_mapped(c, &raw);
            let (cell, reject) = match decoded {
                Ok(decoded) => decoded,
                Err(error) if mask_rejects && !c.key => {
                    let value_len = match &raw {
                        RawCell::Bytes(v) => v.len() as u64,
                        _ => 8,
                    };
                    (
                        crate::types::DecodedCell {
                            presence: crate::types::CellPresence::Unavailable,
                            value: None,
                            arrow: None,
                            reject: Some(crate::types::CellReject {
                                reason: error.fact,
                                value_len,
                            }),
                            actual_precision: None,
                            actual_scale: None,
                            float_bits: None,
                        },
                        Some(values::RejectNote {
                            reason: error.fact,
                            value_len,
                        }),
                    )
                }
                Err(error) => return Err(error),
            };
            if let Some(note) = reject {
                rejects.push((row_index, column_index, note));
            }
            cells.push(cell);
        }
        decoded.push(cells);
    }
    let keys = columns
        .iter()
        .filter(|c| c.key)
        .map(|c| c.plan.attnum)
        .collect::<Vec<_>>();
    let typed = assemble_batch(
        binding,
        columns.iter().map(|c| c.plan.clone()).collect(),
        &keys,
        decoded,
    )
    .map_err(|_| MysqlError::new("batch", "assemble"))?;
    let arrow = typed
        .record_batch()
        .map_err(|_| MysqlError::new("batch", "arrow"))?;
    if arrow.get_array_memory_size() as u64 > BATCH_BYTES {
        return Err(MysqlError::new("budget", "batch_bytes"));
    }
    Ok(Batch {
        typed,
        rows,
        bytes,
        rejects,
        statement: super::StatementNote {
            verb: "SELECT".to_owned(),
            sha256: super::sha256_hex(sql.as_bytes()),
        },
    })
}

/// Stop this exact still-owned connection using a second connection of the same
/// source identity, then discard both. The caller must never reuse a failed pull.
pub async fn cancel_connection(conn: Conn, config: &MysqlConfig, password: &str) -> Result<()> {
    let id = conn.id();
    let cleanup = async {
        let mut control = super::connect(config, password).await?;
        control
            .query_drop(format!("KILL CONNECTION {id}"))
            .await
            .map_err(|_| MysqlError::new("cancel", "kill"))?;
        control
            .disconnect()
            .await
            .map_err(|_| MysqlError::new("cancel", "disconnect"))?;
        Ok(())
    };
    let result = timeout(Duration::from_secs(5), cleanup)
        .await
        .map_err(|_| MysqlError::new("cancel", "cleanup_deadline"))?;
    drop(conn);
    result
}

/// Owns the source connection for cancellation/error cleanup. Credentials are
/// source-only, zeroized and never Debug; no target credential can be supplied.
pub struct ReadConnection {
    conn: Option<Conn>,
    config: MysqlConfig,
    password: zeroize::Zeroizing<String>,
}
impl ReadConnection {
    /// Adopt the caller's already checked read-only snapshot (including C1).
    pub fn new(conn: Conn, config: MysqlConfig, password: zeroize::Zeroizing<String>) -> Self {
        Self {
            conn: Some(conn),
            config,
            password,
        }
    }
    pub async fn read_chunk(
        &mut self,
        reader: &mut TableReader,
        cancel: &Cancel,
    ) -> Result<Option<Batch>> {
        let result = reader
            .next(
                self.conn
                    .as_mut()
                    .ok_or_else(|| MysqlError::new("cancel", "closed"))?,
                cancel,
            )
            .await;
        if result.is_err() {
            self.cancel().await?;
        }
        result
    }
    pub async fn cancel(&mut self) -> Result<()> {
        if let Some(conn) = self.conn.take() {
            cancel_connection(conn, &self.config, &self.password).await?;
        }
        Ok(())
    }
    /// Only successful callers regain the connection to finish their transaction.
    pub fn into_connection(mut self) -> Result<Conn> {
        self.conn
            .take()
            .ok_or_else(|| MysqlError::new("cancel", "closed"))
    }
}

pub struct FullReader {
    inner: TableReader,
    capture_rejects: bool,
}

impl FullReader {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema: &str,
        table: &str,
        columns: Vec<MappedColumn>,
        binding: RunBinding,
        max_rows: u64,
        max_bytes: u64,
        deadline_ms: u64,
        statement_ms: u64,
    ) -> Result<Self> {
        if binding.schema != schema || binding.table != table {
            return Err(MysqlError::new("config", "binding"));
        }
        let inner = TableReader::new(
            schema,
            table,
            columns,
            binding,
            1,
            max_rows,
            max_bytes,
            deadline_ms,
            statement_ms,
        )?;
        let key = inner
            .columns
            .iter()
            .find(|c| c.key)
            .ok_or_else(|| MysqlError::new("catalog", "primary_key"))?;
        if !key.not_null
            || key.plan.mapping.nullable
            || !matches!(
                key.store,
                StoreKind::Int { .. } | StoreKind::Numeric { scale: 0, .. }
            ) && key.oid != 1043
        {
            return Err(MysqlError::new("catalog", "key_type"));
        }
        Ok(Self {
            inner,
            capture_rejects: false,
        })
    }

    /// Reader for an empty composite-key table: keeps the key flags, does not require exactly one key column.
    #[allow(clippy::too_many_arguments)]
    pub fn new_composite(
        schema: &str,
        table: &str,
        columns: Vec<MappedColumn>,
        binding: RunBinding,
        max_rows: u64,
        max_bytes: u64,
        deadline_ms: u64,
        statement_ms: u64,
    ) -> Result<Self> {
        if binding.schema != schema || binding.table != table || columns.iter().filter(|c| c.key).count() < 2 {
            return Err(MysqlError::new("config", "composite_full"));
        }
        Ok(Self {
            capture_rejects: false,
            inner: TableReader::new_inner(
                schema, table, columns, binding, 1, max_rows, max_bytes, deadline_ms, statement_ms, false,
            )?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_unkeyed(
        schema: &str,
        table: &str,
        columns: Vec<MappedColumn>,
        binding: RunBinding,
        max_rows: u64,
        max_bytes: u64,
        deadline_ms: u64,
        statement_ms: u64,
    ) -> Result<Self> {
        if binding.schema != schema || binding.table != table || columns.iter().any(|c| c.key) {
            return Err(MysqlError::new("config", "unkeyed_full"));
        }
        Ok(Self {
            capture_rejects: true,
            inner: TableReader::new_inner(
                schema,
                table,
                columns,
                binding,
                1,
                max_rows,
                max_bytes,
                deadline_ms,
                statement_ms,
                false,
            )?,
        })
    }

    pub async fn read(
        self,
        owned: &mut ReadConnection,
        cancel: &Cancel,
        notes: &mut Vec<super::StatementNote>,
    ) -> Result<Batch> {
        let conn = owned
            .conn
            .as_mut()
            .ok_or_else(|| MysqlError::new("cancel", "closed"))?;
        let result = self.read_on(conn, cancel, notes).await;
        if result.is_err() {
            owned.cancel().await?;
        }
        result
    }

    pub async fn read_on(
        self,
        conn: &mut Conn,
        cancel: &Cancel,
        notes: &mut Vec<super::StatementNote>,
    ) -> Result<Batch> {
        if cancel.is_cancelled() {
            return Err(MysqlError::new("cancel", "requested"));
        }
        let duration = self
            .inner
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| MysqlError::new("budget", "snapshot_deadline"))?;
        let result = tokio::select! {
            biased;
            _ = cancel.wake.notified() => Err(MysqlError::new("cancel", "requested")),
            result = timeout(duration, self.snapshot(conn, notes)) => result.map_err(|_| MysqlError::new("budget", "snapshot_deadline"))?,
        };
        if cancel.is_cancelled() {
            return Err(MysqlError::new("cancel", "requested"));
        }
        if Instant::now() > self.inner.deadline {
            return Err(MysqlError::new("budget", "snapshot_deadline"));
        }
        result
    }

    async fn snapshot(
        &self,
        conn: &mut Conn,
        notes: &mut Vec<super::StatementNote>,
    ) -> Result<Batch> {
        for sql in [
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
        ] {
            timeout(
                self.inner.statement_timeout,
                super::query_drop(conn, notes, sql),
            )
            .await
            .map_err(|_| MysqlError::new("budget", "statement_timeout"))??;
        }
        if self
            .inner
            .columns
            .iter()
            .any(|c| matches!(c.store, StoreKind::Text { limit: None } | StoreKind::Bytes))
        {
            let sizes = self
                .inner
                .columns
                .iter()
                .map(|c| {
                    if matches!(
                        c.store,
                        StoreKind::Text { .. } | StoreKind::Json | StoreKind::Bytes
                    ) {
                        let expression = if self.capture_rejects {
                            super::map::capture_expression(c)
                        } else {
                            format!("`{}`", c.plan.name)
                        };
                        let alias = format!(" AS `{}`", c.plan.name);
                        format!(
                            "COALESCE(OCTET_LENGTH({}),8)",
                            expression.strip_suffix(&alias).unwrap_or(&expression)
                        )
                    } else {
                        "8".into()
                    }
                })
                .collect::<Vec<_>>()
                .join("+");
            let sql = format!(
                "SELECT MAX(__oo_row_bytes) FROM (SELECT ({sizes}) AS __oo_row_bytes FROM `{}`.`{}` LIMIT {}) x",
                self.inner.schema,
                self.inner.table,
                self.inner.max_rows + 1
            );
            notes.push(super::StatementNote {
                verb: "SELECT".into(),
                sha256: super::sha256_hex(sql.as_bytes()),
            });
            let largest: Option<Option<u64>> =
                timeout(self.inner.statement_timeout, conn.query_first(sql))
                    .await
                    .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
                    .map_err(|_| MysqlError::new("protocol", "row_bytes"))?;
            if largest
                .flatten()
                .is_some_and(|bytes| bytes > self.inner.max_bytes)
            {
                return Err(MysqlError::new("budget", "max_bytes"));
            }
        }
        let sql = self.select_sql();
        notes.push(super::StatementNote {
            verb: "SELECT".to_owned(),
            sha256: super::sha256_hex(sql.as_bytes()),
        });
        let batch = timeout(self.inner.statement_timeout, self.pull(conn, &sql))
            .await
            .map_err(|_| MysqlError::new("budget", "statement_timeout"))??;
        let catalog_sql = "SELECT ENGINE FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND TABLE_TYPE = 'BASE TABLE'";
        notes.push(super::StatementNote {
            verb: "SELECT".to_owned(),
            sha256: super::sha256_hex(catalog_sql.as_bytes()),
        });
        let engine: Option<String> = timeout(
            self.inner.statement_timeout,
            conn.exec_first(catalog_sql, (&self.inner.schema, &self.inner.table)),
        )
        .await
        .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
        .map_err(|_| MysqlError::new("catalog", "engine"))?;
        if engine.as_deref() != Some("InnoDB") {
            return Err(MysqlError::new("catalog", "snapshot_engine"));
        }
        timeout(
            self.inner.statement_timeout,
            super::query_drop(conn, notes, "COMMIT"),
        )
        .await
        .map_err(|_| MysqlError::new("budget", "statement_timeout"))??;
        Ok(batch)
    }

    fn select_sql(&self) -> String {
        let list = self
            .inner
            .columns
            .iter()
            .map(|c| {
                if self.capture_rejects {
                    super::map::capture_expression(c)
                } else {
                    format!("`{}`", c.plan.name)
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "SELECT {list} FROM `{}`.`{}` LIMIT ?",
            self.inner.schema, self.inner.table
        )
    }

    async fn pull(&self, conn: &mut Conn, sql: &str) -> Result<Batch> {
        let mut result = conn
            .exec_iter(sql, (self.inner.max_rows + 1,))
            .await
            .map_err(|_| MysqlError::new("protocol", "query"))?;
        let mut rows = Vec::new();
        let mut bytes = 0u64;
        while let Some(row) = result
            .next()
            .await
            .map_err(|_| MysqlError::new("protocol", "query"))?
        {
            if rows.len() as u64 == self.inner.max_rows {
                return Err(MysqlError::new("budget", "max_rows"));
            }
            for index in 0..self.inner.columns.len() {
                bytes = bytes.saturating_add(match raw_cell(&row, index)? {
                    RawCell::Bytes(value) => value.len() as u64,
                    _ => 8,
                });
            }
            if bytes > self.inner.max_bytes {
                return Err(MysqlError::new("budget", "max_bytes"));
            }
            if bytes > BATCH_BYTES {
                return Err(MysqlError::new("budget", "batch_bytes"));
            }
            rows.push(row);
        }
        if self.capture_rejects {
            encode_chunk_batch(&self.inner.columns, &self.inner.binding, rows, bytes, sql)
        } else {
            encode_batch(&self.inner.columns, &self.inner.binding, rows, bytes, sql)
        }
    }
}

/// Chunk defaults. Caps apply to one chunk; a table has no cumulative limit.
pub const CHUNK_MAX_ROWS: u64 = 50_000;
pub const CHUNK_MAX_BYTES: u64 = 8 * 1024 * 1024;
pub const CHUNK_DEADLINE_MS: u64 = 120_000;
pub const CHUNK_STATEMENT_MS: u64 = 30_000;
/// The Arrow memory of one chunk is capped at the 32 MiB part limit.
const CHUNK_ARROW_BYTES: u64 = 32 * 1024 * 1024;

/// Keyset position for chunk mode. `after` is exclusive and `None` means no lower
/// bound. Persist `key_hi` of a committed chunk and resume with `resume_after`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChunkCursor {
    after: Option<i128>,
    avg_row_bytes: u64,
}
impl ChunkCursor {
    pub fn start() -> Self {
        Self::default()
    }
    /// First chunk of a table. `avg_row_bytes` (for example information_schema
    /// AVG_ROW_LENGTH) sizes the first LIMIT, so a wide table does not stream
    /// and discard `max_rows` rows before the byte budget cuts the chunk.
    pub fn start_with_hint(avg_row_bytes: u64) -> Self {
        Self {
            after: None,
            avg_row_bytes,
        }
    }
    pub fn resume_after(key_hi: i128) -> Self {
        Self {
            after: Some(key_hi),
            avg_row_bytes: 0,
        }
    }
    /// Resume after a persisted `key_hi`, carrying the row-size hint across the
    /// restart. A hint of 0 means unknown.
    pub fn resume_after_with_hint(key_hi: i128, avg_row_bytes: u64) -> Self {
        Self {
            after: Some(key_hi),
            avg_row_bytes,
        }
    }
    pub fn after(&self) -> Option<i128> {
        self.after
    }
    /// Observed or supplied bytes per row. 0 means unknown.
    pub fn avg_row_bytes(&self) -> u64 {
        self.avg_row_bytes
    }
    fn advance(&mut self, key_hi: i128, rows: u64, bytes: u64) {
        self.after = Some(key_hi);
        self.avg_row_bytes = (bytes / rows.max(1)).max(1);
    }
}

/// One keyset chunk. `key_lo` is the exclusive lower bound (`None` for the first
/// chunk). `key_hi` is the last key included and is `None` only when `terminal`.
/// `terminal` means the read returned 0 rows, so there is nothing after `key_lo`.
pub struct ChunkBatch {
    pub typed: TypedBatch,
    /// Exclusive lower bound the chunk was read after (the previous `key_hi`).
    pub key_lo: Option<i128>,
    /// Smallest key actually in the chunk. `None` when the chunk is empty.
    pub key_first: Option<i128>,
    pub key_hi: Option<i128>,
    pub rows: u64,
    pub bytes: u64,
    pub terminal: bool,
    pub rejects: Vec<(usize, usize, values::RejectNote)>,
    pub statement: super::StatementNote,
}

/// Keyset chunk reader for one table with one integer primary key. Each chunk is a
/// single autocommit SELECT on the connection passed in. It assumes no snapshot or
/// open transaction, so the same reader can run against a replica connection. The
/// caller must pass a connection that is not inside a transaction.
pub struct ChunkReader {
    schema: String,
    table: String,
    columns: Vec<MappedColumn>,
    binding: RunBinding,
    key_index: usize,
    max_rows: u64,
    max_bytes: u64,
    deadline: Duration,
    statement_timeout: Duration,
}
impl ChunkReader {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema: &str,
        table: &str,
        columns: Vec<MappedColumn>,
        binding: RunBinding,
        max_rows: u64,
        max_bytes: u64,
        chunk_deadline_ms: u64,
        statement_ms: u64,
    ) -> Result<Self> {
        if !super::mysql_ident(schema)
            || !super::mysql_ident(table)
            || columns.is_empty()
            || columns.iter().any(|c| !super::mysql_ident(&c.plan.name))
        {
            return Err(MysqlError::new("config", "chunk_shape"));
        }
        let keys: Vec<usize> = columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.key)
            .map(|(index, _)| index)
            .collect();
        let key_index = match keys.as_slice() {
            [only] => *only,
            _ => return Err(MysqlError::new("config", "chunk_shape")),
        };
        if !key_store_ok(columns[key_index].store) {
            return Err(MysqlError::new("catalog", "key_type"));
        }
        if !(1..=1_000_000).contains(&max_rows)
            || !(1..=33_554_432).contains(&max_bytes)
            || !(1..=300_000).contains(&chunk_deadline_ms)
            || !(1..=30_000).contains(&statement_ms)
        {
            return Err(MysqlError::new("config", "read_budget"));
        }
        Ok(Self {
            schema: schema.to_owned(),
            table: table.to_owned(),
            columns,
            binding,
            key_index,
            max_rows,
            max_bytes,
            deadline: Duration::from_millis(chunk_deadline_ms),
            statement_timeout: Duration::from_millis(statement_ms),
        })
    }

    /// Reads the next chunk after `cursor`. On success the cursor moves to `key_hi`,
    /// but only when the chunk is not terminal. On error the connection is cancelled,
    /// as in `ReadConnection::read_chunk`, and the cursor is left unchanged.
    pub async fn read_chunk(
        &self,
        conn: &mut ReadConnection,
        cursor: &mut ChunkCursor,
        cancel: &Cancel,
    ) -> Result<ChunkBatch> {
        let deadline = Instant::now() + self.deadline;
        let result = match conn.conn.as_mut() {
            Some(raw) => self.step(raw, cursor, cancel, deadline).await,
            None => Err(MysqlError::new("cancel", "closed")),
        };
        match &result {
            Ok(batch) => {
                if let Some(key_hi) = batch.key_hi {
                    cursor.advance(key_hi, batch.rows, batch.bytes);
                }
            }
            Err(_) => conn.cancel().await?,
        }
        result
    }

    async fn step(
        &self,
        conn: &mut Conn,
        cursor: &ChunkCursor,
        cancel: &Cancel,
        deadline: Instant,
    ) -> Result<ChunkBatch> {
        if cancel.is_cancelled() {
            return Err(MysqlError::new("cancel", "requested"));
        }
        let duration = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| MysqlError::new("budget", "chunk_deadline"))?;
        let result = tokio::select! {
            biased;
            _=cancel.wake.notified()=>Err(MysqlError::new("cancel","requested")),
            result=timeout(duration, self.pull(conn, cursor))=>result.map_err(|_|MysqlError::new("budget","chunk_deadline"))?,
        };
        if cancel.is_cancelled() {
            Err(MysqlError::new("cancel", "requested"))
        } else if Instant::now() > deadline {
            Err(MysqlError::new("budget", "chunk_deadline"))
        } else {
            result
        }
    }

    async fn pull(&self, conn: &mut Conn, cursor: &ChunkCursor) -> Result<ChunkBatch> {
        let key = &self.columns[self.key_index];
        let key_name = key.plan.name.as_str();
        let list = self
            .columns
            .iter()
            .map(|c| format!("`{}`", c.plan.name))
            .collect::<Vec<_>>()
            .join(",");
        let predicate = if cursor.after.is_some() {
            // Mapping permits exact integers only. DECIMAL keys compare after CAST.
            let bound = if matches!(key.store, StoreKind::Numeric { .. }) {
                "CAST(? AS DECIMAL(65,0))"
            } else {
                "?"
            };
            format!(" WHERE `{key_name}` > {bound}")
        } else {
            String::new()
        };
        let sql = format!(
            "SELECT {list} FROM `{}`.`{}`{predicate} ORDER BY `{key_name}` LIMIT ?",
            self.schema, self.table
        );
        let limit = fetch_limit(self.max_rows, self.max_bytes, cursor.avg_row_bytes);
        let mut result = if let Some(after) = cursor.after {
            let param = key_param(key.store, after)?;
            timeout(self.statement_timeout, conn.exec_iter(&sql, (param, limit))).await
        } else {
            timeout(self.statement_timeout, conn.exec_iter(&sql, (limit,))).await
        }
        .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
        .map_err(|_| MysqlError::new("protocol", "query"))?;
        // Read to EOF. Rows after the cut are drained, not kept, so the connection
        // stays usable for the next statement.
        let mut budget = ChunkBudget::new(self.max_rows, self.max_bytes);
        let mut rows = Vec::new();
        loop {
            let next = timeout(self.statement_timeout, result.next())
                .await
                .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
                .map_err(|_| MysqlError::new("protocol", "query"))?;
            let Some(row) = next else { break };
            if budget.is_full() {
                continue;
            }
            let size = encoded_row_bytes(&row, self.columns.len())?;
            let key = key_value_at(&row, self.key_index)?;
            if budget.admit(key, size)? == Admit::Take {
                rows.push(row);
            }
        }
        let row_count = budget.rows;
        let mut decoded = Vec::with_capacity(rows.len());
        let mut rejects = Vec::new();
        for (row_index, row) in rows.iter().enumerate() {
            let mut cells = Vec::with_capacity(self.columns.len());
            for (column_index, c) in self.columns.iter().enumerate() {
                let (cell, reject) = values::decode_mapped(c, &raw_cell(row, column_index)?)?;
                if let Some(note) = reject {
                    rejects.push((row_index, column_index, note));
                }
                cells.push(cell);
            }
            decoded.push(cells);
        }
        let keys = self
            .columns
            .iter()
            .filter(|c| c.key)
            .map(|c| c.plan.attnum)
            .collect::<Vec<_>>();
        let typed = assemble_batch(
            &self.binding,
            self.columns.iter().map(|c| c.plan.clone()).collect(),
            &keys,
            decoded,
        )
        .map_err(|_| MysqlError::new("batch", "assemble"))?;
        let arrow = typed
            .record_batch()
            .map_err(|_| MysqlError::new("batch", "arrow"))?;
        if arrow.get_array_memory_size() as u64 > CHUNK_ARROW_BYTES {
            return Err(MysqlError::new("budget", "chunk_arrow"));
        }
        Ok(ChunkBatch {
            typed,
            key_lo: cursor.after,
            key_first: budget.first_key,
            key_hi: budget.key_hi,
            rows: row_count,
            bytes: budget.bytes,
            terminal: row_count == 0,
            rejects,
            statement: super::StatementNote {
                verb: "SELECT".to_owned(),
                sha256: super::sha256_hex(sql.as_bytes()),
            },
        })
    }
}

/// Pure per-chunk budget. SQL returns rows in key order. The first row that does not
/// fit ends the chunk, and every later row is excluded too, so the chunk is a key prefix.
struct ChunkBudget {
    max_rows: u64,
    max_bytes: u64,
    rows: u64,
    bytes: u64,
    first_key: Option<i128>,
    key_hi: Option<i128>,
    full: bool,
}
#[derive(Debug, PartialEq, Eq)]
enum Admit {
    Take,
    Cut,
}
impl ChunkBudget {
    fn new(max_rows: u64, max_bytes: u64) -> Self {
        Self {
            max_rows,
            max_bytes,
            rows: 0,
            bytes: 0,
            first_key: None,
            key_hi: None,
            full: false,
        }
    }
    fn is_full(&self) -> bool {
        self.full
    }
    fn admit(&mut self, key: i128, size: u64) -> Result<Admit> {
        if self.full {
            return Ok(Admit::Cut);
        }
        if self.rows >= self.max_rows {
            self.full = true;
            return Ok(Admit::Cut);
        }
        // A row bigger than the whole budget cannot be placed in any chunk.
        if size > self.max_bytes {
            return Err(MysqlError::new("budget", "row_bytes"));
        }
        if self.bytes + size > self.max_bytes {
            self.full = true;
            return Ok(Admit::Cut);
        }
        self.rows += 1;
        self.bytes += size;
        self.first_key.get_or_insert(key);
        self.key_hi = Some(key);
        Ok(Admit::Take)
    }
}

/// LIMIT for the next statement. Before a chunk is observed it is the row cap. After
/// that it is the count the byte budget holds at the observed row size, plus 12.5 %,
/// which keeps the discarded tail of each statement small.
fn fetch_limit(max_rows: u64, max_bytes: u64, avg_row_bytes: u64) -> u64 {
    if avg_row_bytes == 0 {
        return max_rows;
    }
    ((max_bytes + max_bytes / 8) / avg_row_bytes).clamp(1, max_rows)
}

/// Integer keys only: BIGINT/INT families and DECIMAL(p,0) with p <= 38, which fits i128.
fn key_store_ok(store: StoreKind) -> bool {
    match store {
        StoreKind::Int { .. } => true,
        StoreKind::Numeric { precision, scale } => scale == 0 && precision <= 38,
        _ => false,
    }
}

/// Binds an exact integer. Signed columns take `Int`. Values above i64::MAX take `UInt`.
/// DECIMAL keys are bound as decimal text and compared after CAST.
fn key_param(store: StoreKind, key: i128) -> Result<Value> {
    match store {
        StoreKind::Int { .. } => {
            if let Ok(v) = i64::try_from(key) {
                Ok(Value::Int(v))
            } else if let Ok(v) = u64::try_from(key) {
                Ok(Value::UInt(v))
            } else {
                Err(MysqlError::new("config", "key_range"))
            }
        }
        StoreKind::Numeric { .. } => Ok(Value::Bytes(key.to_string().into_bytes())),
        _ => Err(MysqlError::new("catalog", "key_type")),
    }
}

fn key_of(cell: &RawCell) -> Result<i128> {
    match cell {
        RawCell::Int(v) => Ok(i128::from(*v)),
        RawCell::Uint(v) => Ok(i128::from(*v)),
        RawCell::Bytes(v) => std::str::from_utf8(v)
            .ok()
            .and_then(|s| s.parse::<i128>().ok())
            .ok_or_else(|| MysqlError::new("value", "key")),
        _ => Err(MysqlError::new("value", "key")),
    }
}

fn key_value_at(row: &Row, index: usize) -> Result<i128> {
    key_of(&raw_cell(row, index)?)
}

/// Same accounting as `TableReader::pull`. Byte cells count their length, and every
/// other cell counts 8.
fn encoded_row_bytes(row: &Row, columns: usize) -> Result<u64> {
    let mut size = 0u64;
    for index in 0..columns {
        size = size.saturating_add(match raw_cell(row, index)? {
            RawCell::Bytes(v) => v.len() as u64,
            _ => 8,
        });
    }
    Ok(size)
}

#[cfg(test)]
mod chunk_tests {
    use super::*;
    use crate::PG17_RESERVED;
    use crate::mysql::map::{MysqlColumn, map_table};
    use std::collections::BTreeSet;

    fn col(name: &str, ordinal: i32, data: &str, column_type: &str, key: bool) -> MysqlColumn {
        MysqlColumn {
            name: name.to_owned(),
            ordinal,
            data_type: data.to_owned(),
            column_type: column_type.to_owned(),
            nullable: false,
            char_len: None,
            precision: None,
            scale: None,
            charset: None,
            column_key: if key { "PRI" } else { "" }.to_owned(),
        }
    }
    fn mapped(columns: &[MysqlColumn]) -> Vec<MappedColumn> {
        let reserved: BTreeSet<String> = PG17_RESERVED.iter().map(|w| (*w).to_owned()).collect();
        map_table(columns, false, true, &reserved).unwrap()
    }
    fn binding() -> RunBinding {
        RunBinding {
            source_id: "src".to_owned(),
            schema: "db".to_owned(),
            table: "t".to_owned(),
            run_generation: 1,
            input_digest: [0; 32],
        }
    }
    fn payload() -> MysqlColumn {
        let mut payload = col("payload", 2, "varchar", "varchar(64)", false);
        payload.char_len = Some(64);
        payload.charset = Some("utf8mb4".to_owned());
        payload
    }
    fn unsigned_key_table() -> Vec<MappedColumn> {
        mapped(&[col("id", 1, "bigint", "bigint unsigned", true), payload()])
    }
    fn new_reader(
        columns: Vec<MappedColumn>,
        rows: u64,
        bytes: u64,
        deadline: u64,
        statement: u64,
    ) -> Result<ChunkReader> {
        ChunkReader::new(
            "db",
            "t",
            columns,
            binding(),
            rows,
            bytes,
            deadline,
            statement,
        )
    }

    /// Same contract as `WHERE pk > ? ORDER BY pk LIMIT ?` plus the streaming budget.
    struct Sim {
        admitted: Vec<(i128, u64)>,
        rows: u64,
        bytes: u64,
        key_hi: Option<i128>,
        first_key: Option<i128>,
    }
    fn simulate(
        table: &[(i128, u64)],
        after: Option<i128>,
        limit: u64,
        max_rows: u64,
        max_bytes: u64,
    ) -> Result<Sim> {
        let mut budget = ChunkBudget::new(max_rows, max_bytes);
        let mut admitted = Vec::new();
        for &(key, size) in table
            .iter()
            .filter(|(k, _)| after.is_none_or(|a| *k > a))
            .take(limit as usize)
        {
            if budget.admit(key, size)? == Admit::Take {
                admitted.push((key, size));
            }
        }
        Ok(Sim {
            admitted,
            rows: budget.rows,
            bytes: budget.bytes,
            key_hi: budget.key_hi,
            first_key: budget.first_key,
        })
    }
    fn lcg(seed: &mut u64) -> u64 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *seed >> 33
    }
    fn sized_table(mut seed: u64, count: usize, start: i128) -> Vec<(i128, u64)> {
        let mut key = start;
        (0..count)
            .map(|_| {
                key += 1 + (lcg(&mut seed) % 3) as i128;
                (key, 8 + lcg(&mut seed) % 400)
            })
            .collect()
    }
    /// Drives the cursor the way the reader does and checks every chunk boundary.
    fn walk(table: &[(i128, u64)], max_rows: u64, max_bytes: u64) -> Vec<i128> {
        let mut cursor = ChunkCursor::start();
        let mut seen = Vec::new();
        loop {
            let limit = fetch_limit(max_rows, max_bytes, cursor.avg_row_bytes);
            let sim = simulate(table, cursor.after(), limit, max_rows, max_bytes).unwrap();
            if sim.rows == 0 {
                assert_eq!(sim.key_hi, None);
                assert!(
                    !table
                        .iter()
                        .any(|(k, _)| cursor.after().is_none_or(|a| *k > a))
                );
                return seen;
            }
            assert!(sim.rows <= max_rows && sim.bytes <= max_bytes);
            let key_hi = sim.key_hi.unwrap();
            assert_eq!(sim.first_key, sim.admitted.first().map(|a| a.0));
            // A cut that is not forced by LIMIT or by the row cap must be forced by the next row not fitting.
            if sim.rows < limit
                && sim.rows < max_rows
                && let Some(next) = table.iter().find(|(k, _)| *k > key_hi)
            {
                assert!(sim.bytes + next.1 > max_bytes);
            }
            seen.extend(sim.admitted.iter().map(|(k, _)| *k));
            cursor.advance(key_hi, sim.rows, sim.bytes);
        }
    }

    #[test]
    fn cut_by_rows_takes_a_key_prefix() {
        let table: Vec<(i128, u64)> = (1..=10).map(|k| (k, 1)).collect();
        let sim = simulate(&table, None, 10, 4, 1000).unwrap();
        assert_eq!((sim.rows, sim.bytes, sim.key_hi), (4, 4, Some(4)));
        assert_eq!(
            sim.admitted.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn cut_by_bytes_excludes_the_row_that_would_exceed_the_budget() {
        let table: Vec<(i128, u64)> = (1..=5).map(|k| (k, 300)).collect();
        let sim = simulate(&table, None, 50_000, 50_000, 1000).unwrap();
        assert_eq!((sim.rows, sim.bytes, sim.key_hi), (3, 900, Some(3)));
    }

    #[test]
    fn byte_cut_is_sticky_so_a_later_small_row_cannot_skip_the_cut_row() {
        let table = vec![(1, 300), (2, 300), (3, 300), (4, 400), (5, 50)];
        let sim = simulate(&table, None, 50_000, 50_000, 1000).unwrap();
        assert_eq!(
            sim.admitted.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(sim.key_hi, Some(3));
    }

    #[test]
    fn row_larger_than_the_chunk_budget_fails_closed() {
        let err = simulate(&[(1, 2000)], None, 10, 10, 1000).err().unwrap();
        assert_eq!((err.code, err.fact), ("budget", "row_bytes"));
    }

    #[test]
    fn oversized_row_after_a_cut_is_not_evaluated() {
        let table = vec![(1, 600), (2, 500), (3, 9000)];
        let sim = simulate(&table, None, 10, 10, 1000).unwrap();
        assert_eq!((sim.rows, sim.key_hi), (1, Some(1)));
    }

    #[test]
    fn empty_tail_is_terminal_with_no_key_hi() {
        let sim = simulate(&[(1, 1), (2, 1)], Some(2), 10, 10, 100).unwrap();
        assert_eq!((sim.rows, sim.bytes, sim.key_hi), (0, 0, None));
    }

    #[test]
    fn resume_from_key_hi_covers_every_key_once_when_cut_by_rows() {
        let table = sized_table(7, 500, 0);
        let keys: Vec<i128> = table.iter().map(|(k, _)| *k).collect();
        assert_eq!(walk(&table, 7, 2000), keys);
    }

    #[test]
    fn resume_covers_every_key_once_with_the_observed_row_size_hint() {
        let table = sized_table(11, 2000, 1_000);
        let keys: Vec<i128> = table.iter().map(|(k, _)| *k).collect();
        assert_eq!(walk(&table, 50_000, 2000), keys);
    }

    #[test]
    fn unsigned_bigint_max_key_round_trips_and_terminates() {
        let max = u64::MAX as i128;
        let text = b"18446744073709551615".to_vec();
        assert_eq!(key_of(&RawCell::Bytes(text.clone())).unwrap(), max);
        assert_eq!(key_of(&RawCell::Uint(u64::MAX)).unwrap(), max);
        assert!(matches!(
            key_param(StoreKind::Numeric { precision: 20, scale: 0 }, max),
            Ok(Value::Bytes(ref b)) if *b == text
        ));
        assert!(matches!(
            key_param(StoreKind::Int { width: 8 }, max),
            Ok(Value::UInt(u64::MAX))
        ));
        let table = vec![(max - 1, 1), (max, 1)];
        let at_max = simulate(&table, Some(max), 10, 10, 100).unwrap();
        assert_eq!(at_max.rows, 0);
        let before_max = simulate(&table, Some(max - 2), 10, 10, 100).unwrap();
        assert_eq!(before_max.key_hi, Some(max));
    }

    #[test]
    fn integer_key_params_pick_the_exact_driver_type() {
        assert!(matches!(
            key_param(StoreKind::Int { width: 8 }, i64::MIN as i128),
            Ok(Value::Int(v)) if v == i64::MIN
        ));
        assert!(key_param(StoreKind::Int { width: 8 }, u64::MAX as i128 + 1).is_err());
        assert!(key_param(StoreKind::Int { width: 8 }, i64::MIN as i128 - 1).is_err());
        assert!(key_param(StoreKind::Bool, 1).is_err());
    }

    #[test]
    fn key_cells_must_be_integers() {
        assert_eq!(key_of(&RawCell::Int(-5)).unwrap(), -5);
        assert!(key_of(&RawCell::Null).is_err());
        assert!(key_of(&RawCell::Bytes(b"12x".to_vec())).is_err());
        assert!(key_of(&RawCell::Double(1.0)).is_err());
    }

    #[test]
    fn fetch_limit_tracks_observed_row_size_and_stays_within_bounds() {
        assert_eq!(fetch_limit(50_000, 8 << 20, 0), 50_000);
        // 9 MiB of slack (8 MiB + 12.5 %) at 1000 bytes per row.
        assert_eq!(fetch_limit(50_000, 8 << 20, 1000), 9437);
        assert_eq!(fetch_limit(50_000, 8 << 20, 8), 50_000);
        assert_eq!(fetch_limit(50_000, 8 << 20, 1 << 40), 1);
    }

    #[test]
    fn cursor_starts_unbounded_and_resumes_from_a_persisted_key() {
        let mut cursor = ChunkCursor::start();
        assert_eq!(cursor.after(), None);
        cursor.advance(42, 4, 4000);
        assert_eq!(cursor.after(), Some(42));
        assert_eq!(ChunkCursor::resume_after(42).after(), Some(42));
    }

    #[test]
    fn row_size_hint_sizes_the_first_statement_of_a_table_or_a_resume() {
        let first = ChunkCursor::start_with_hint(1000);
        assert_eq!((first.after(), first.avg_row_bytes()), (None, 1000));
        assert_eq!(fetch_limit(50_000, 8 << 20, first.avg_row_bytes()), 9437);
        let resumed = ChunkCursor::resume_after_with_hint(42, 1000);
        assert_eq!((resumed.after(), resumed.avg_row_bytes()), (Some(42), 1000));
        assert_eq!(ChunkCursor::start().avg_row_bytes(), 0);
    }

    #[test]
    fn chunk_reports_the_first_admitted_key_for_the_manifest_range() {
        let sim = simulate(&[(5, 1), (6, 1)], None, 10, 10, 100).unwrap();
        assert_eq!(sim.first_key, Some(5));
        // 300 + 300 > 500, so the second row cuts the chunk and only key 5 is admitted.
        let cut = simulate(&[(5, 300), (6, 300), (7, 300)], None, 10, 50_000, 500).unwrap();
        assert_eq!((cut.first_key, cut.key_hi), (Some(5), Some(5)));
        let empty = simulate(&[(1, 1)], Some(1), 10, 10, 100).unwrap();
        assert_eq!(empty.first_key, None);
    }

    #[test]
    fn new_accepts_defaults_and_the_exact_budget_boundaries() {
        let defaults = new_reader(
            unsigned_key_table(),
            CHUNK_MAX_ROWS,
            CHUNK_MAX_BYTES,
            CHUNK_DEADLINE_MS,
            CHUNK_STATEMENT_MS,
        );
        assert!(defaults.is_ok());
        assert!(new_reader(unsigned_key_table(), 1, 1, 1, 1).is_ok());
        assert!(new_reader(unsigned_key_table(), 1_000_000, 33_554_432, 300_000, 30_000).is_ok());
    }

    #[test]
    fn new_rejects_out_of_range_budgets() {
        let bad = [
            (0, 8 << 20, 120_000, 30_000),
            (1_000_001, 8 << 20, 120_000, 30_000),
            (50_000, 0, 120_000, 30_000),
            (50_000, 33_554_433, 120_000, 30_000),
            (50_000, 8 << 20, 0, 30_000),
            (50_000, 8 << 20, 300_001, 30_000),
            (50_000, 8 << 20, 120_000, 0),
            (50_000, 8 << 20, 120_000, 30_001),
        ];
        for (rows, bytes, deadline, statement) in bad {
            let err = new_reader(unsigned_key_table(), rows, bytes, deadline, statement)
                .err()
                .expect("out of range budget is rejected");
            assert_eq!((err.code, err.fact), ("config", "read_budget"));
        }
    }

    #[test]
    fn full_reader_accepts_varchar_without_extending_keyset() {
        let mut key = col("id", 1, "varchar", "varchar(32)", true);
        key.char_len = Some(32);
        key.charset = Some("utf8".to_owned());
        let columns = mapped(&[key, payload()]);
        let full =
            FullReader::new("db", "t", columns.clone(), binding(), 10, 1024, 1000, 1000).unwrap();
        assert_eq!(
            full.select_sql(),
            "SELECT `id`,`payload` FROM `db`.`t` LIMIT ?"
        );
        assert!(new_reader(columns.clone(), 10, 1024, 1000, 1000).is_err());
        let mut nullable = columns.clone();
        nullable[0].not_null = false;
        assert!(FullReader::new("db", "t", nullable, binding(), 10, 1024, 1000, 1000).is_err());
        for (rows, bytes, deadline, statement) in [
            (0, 1024, 1000, 1000),
            (1000001, 1024, 1000, 1000),
            (10, 33554433, 1000, 1000),
            (10, 1024, 300001, 1000),
            (10, 1024, 1000, 30001),
        ] {
            assert!(
                FullReader::new(
                    "db",
                    "t",
                    columns.clone(),
                    binding(),
                    rows,
                    bytes,
                    deadline,
                    statement
                )
                .is_err()
            );
        }
        let mut wrong = binding();
        wrong.table = "other".to_owned();
        assert!(FullReader::new("db", "t", columns, wrong, 10, 1024, 1000, 1000).is_err());
    }

    #[test]
    fn new_rejects_non_integer_and_wide_decimal_keys() {
        let mut varchar_key = col("id", 1, "varchar", "varchar(16)", true);
        varchar_key.char_len = Some(16);
        varchar_key.charset = Some("utf8mb4".to_owned());
        let err = new_reader(mapped(&[varchar_key, payload()]), 10, 10, 10, 10)
            .err()
            .unwrap();
        assert_eq!((err.code, err.fact), ("catalog", "key_type"));

        let mut wide = col("id", 1, "decimal", "decimal(39,0)", true);
        wide.precision = Some(39);
        wide.scale = Some(0);
        let err = new_reader(mapped(&[wide, payload()]), 10, 10, 10, 10)
            .err()
            .unwrap();
        assert_eq!((err.code, err.fact), ("catalog", "key_type"));

        let mut decimal_key = col("id", 1, "decimal", "decimal(38,0)", true);
        decimal_key.precision = Some(38);
        decimal_key.scale = Some(0);
        assert!(new_reader(mapped(&[decimal_key, payload()]), 10, 10, 10, 10).is_ok());
    }

    #[test]
    fn new_rejects_missing_composite_and_bad_identifiers() {
        let mut none = mapped(&[col("id", 1, "bigint", "bigint", true), payload()]);
        none[0].key = false;
        assert!(new_reader(none, 10, 10, 10, 10).is_err());

        let mut two = mapped(&[col("id", 1, "bigint", "bigint", true), payload()]);
        two[1].key = true;
        assert!(new_reader(two, 10, 10, 10, 10).is_err());

        let bad_table = ChunkReader::new(
            "db",
            "t; drop",
            unsigned_key_table(),
            binding(),
            10,
            10,
            10,
            10,
        );
        assert!(bad_table.is_err());
    }
}

#[cfg(test)]
mod live_chunk_tests {
    use super::*;
    use crate::mysql::{
        MysqlConfig, SelectedTable, catalog_columns, connect, keep_requested, map::map_table,
    };
    use crate::{PG17_RESERVED, SecretRef};
    use std::collections::BTreeSet;
    use zeroize::Zeroizing;

    /// Reads `OO_MYSQL_CHUNK_FIXTURE` (a JSON file with `host`, `port`, `database`,
    /// `user`, `ca_file`, `password_file`, `secret_ref`). The test creates and drops
    /// its own tables in `database`.
    async fn open() -> (MysqlConfig, Zeroizing<String>) {
        let path = std::env::var("OO_MYSQL_CHUNK_FIXTURE").expect("OO_MYSQL_CHUNK_FIXTURE");
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let text = |key: &str| fixture[key].as_str().unwrap().to_owned();
        let config = MysqlConfig {
            mysql_host: text("host"),
            mysql_port: fixture["port"].as_u64().unwrap() as u16,
            mysql_database: text("database"),
            mysql_user: text("user"),
            mysql_ca_file: text("ca_file").into(),
            mysql_tls_server_name: None,
            secret_ref: SecretRef::new(text("secret_ref")).unwrap(),
            connect_timeout_ms: 5000,
        };
        let password = Zeroizing::new(
            std::fs::read_to_string(text("password_file"))
                .unwrap()
                .trim_end_matches(['\r', '\n'])
                .to_owned(),
        );
        (config, password)
    }

    async fn columns(conn: &mut Conn, schema: &str, table: &str) -> Vec<MappedColumn> {
        let selected = SelectedTable {
            schema: schema.to_owned(),
            name: table.to_owned(),
            columns: Vec::new(),
        };
        let catalog = catalog_columns(conn, &mut Vec::new(), &selected)
            .await
            .unwrap();
        let reserved: BTreeSet<String> = PG17_RESERVED.iter().map(|w| (*w).to_owned()).collect();
        keep_requested(
            map_table(&catalog, false, true, &reserved).unwrap(),
            &selected.columns,
        )
        .unwrap()
    }

    /// Reads every chunk and checks it against the sorted key list. Returns the chunk count.
    async fn drain_and_check(
        reader: &ChunkReader,
        owned: &mut ReadConnection,
        ids: &[i128],
        max_bytes: u64,
    ) -> usize {
        let mut cursor = ChunkCursor::start();
        let mut pos = 0usize;
        let mut chunks = 0usize;
        loop {
            let batch = reader
                .read_chunk(owned, &mut cursor, &Cancel::default())
                .await
                .unwrap();
            assert_eq!(batch.key_lo, pos.checked_sub(1).map(|i| ids[i]));
            if batch.terminal {
                assert_eq!((batch.rows, batch.key_hi), (0, None));
                assert_eq!(pos, ids.len(), "terminal only after every key");
                return chunks;
            }
            let rows = batch.rows as usize;
            assert!(rows >= 1 && pos + rows <= ids.len());
            assert_eq!(batch.typed.record_batch().unwrap().num_rows(), rows);
            assert!(batch.bytes > 0 && batch.bytes <= max_bytes);
            assert_eq!(batch.key_hi, Some(ids[pos + rows - 1]));
            assert_eq!(cursor.after(), batch.key_hi);
            pos += rows;
            chunks += 1;
        }
    }

    #[tokio::test]
    #[ignore = "requires OO_MYSQL_CHUNK_FIXTURE: an owned no-binlog TLS MySQL with TABLE general_log"]
    async fn full_snapshot_contract() {
        let (config, password) = open().await;
        let config = &config;
        let password = &password;
        use arrow_array::{Array, Decimal128Array, StringArray, TimestampMicrosecondArray};
        let mut admin = connect(config, password).await.unwrap();
        assert_eq!(
            admin
                .query_first::<u8, _>("SELECT @@log_bin")
                .await
                .unwrap(),
            Some(0)
        );
        admin
            .query_drop("DROP TABLE IF EXISTS full_items")
            .await
            .unwrap();
        admin
            .query_drop("DROP USER IF EXISTS 'full_reader_fixture'@'%'")
            .await
            .unwrap();
        admin.query_drop("CREATE TABLE full_items (id VARCHAR(32) CHARACTER SET utf8 PRIMARY KEY, label TEXT CHARACTER SET utf8, amount DECIMAL(18,2) NOT NULL, counter BIGINT UNSIGNED NOT NULL, happened DATETIME(6) NOT NULL) ENGINE=InnoDB").await.unwrap();
        let expected = ["", "A", "a-2", "é", "中"];
        for id in expected {
            admin.exec_drop("INSERT INTO full_items VALUES (?, ?, -123.45, 18446744073709551615, '2024-02-29 12:34:56.123456')", (id, format!("label-{id}"))).await.unwrap();
        }
        admin
            .query_drop(format!(
                "CREATE USER 'full_reader_fixture'@'%' IDENTIFIED BY {}",
                Value::Bytes(password.as_bytes().to_vec()).as_sql(false),
            ))
            .await
            .unwrap();
        admin.query_drop("GRANT SELECT, PROCESS, LOCK TABLES, SHOW VIEW, REPLICATION SLAVE, REPLICATION CLIENT ON *.* TO 'full_reader_fixture'@'%'").await.unwrap();
        let audit_start: String = admin
            .query_first("SELECT DATE_FORMAT(NOW(6), '%Y-%m-%d %H:%i:%s.%f')")
            .await
            .unwrap()
            .unwrap();
        let mut reader_config = config.clone();
        reader_config.mysql_user = "full_reader_fixture".to_owned();
        let mut conn = connect(&reader_config, password).await.unwrap();
        conn.query_drop("SET SESSION time_zone = '+00:00'")
            .await
            .unwrap();
        let id = conn.id();
        let (_access, grants) =
            crate::mysql::inspect_full(&mut conn, &reader_config.mysql_user, &mut Vec::new())
                .await
                .unwrap();
        assert!(grants.privileges.contains("LOCK TABLES"));
        let mapped = columns(&mut conn, &config.mysql_database, "full_items").await;
        let binding = RunBinding {
            source_id: "full_fixture".to_owned(),
            schema: config.mysql_database.clone(),
            table: "full_items".to_owned(),
            run_generation: 1,
            input_digest: [0; 32],
        };
        let full = |rows, bytes, deadline| {
            FullReader::new(
                &config.mysql_database,
                "full_items",
                mapped.clone(),
                binding.clone(),
                rows,
                bytes,
                deadline,
                30000,
            )
            .unwrap()
        };
        let mut owned = ReadConnection::new(conn, reader_config.clone(), password.clone());
        for _ in 0..2 {
            let mut notes = Vec::new();
            let batch = full(5, 65536, 30000)
                .read(&mut owned, &Cancel::default(), &mut notes)
                .await
                .unwrap();
            assert_eq!(batch.rows.len(), 5);
            assert!(batch.rejects.is_empty());
            assert_eq!(
                notes.iter().map(|n| n.verb.as_str()).collect::<Vec<_>>(),
                ["SET", "START", "SELECT", "SELECT", "COMMIT"]
            );
            let arrow = batch.typed.record_batch().unwrap();
            let keys = arrow
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let labels = arrow
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let amounts = arrow
                .column(2)
                .as_any()
                .downcast_ref::<Decimal128Array>()
                .unwrap();
            let counters = arrow
                .column(3)
                .as_any()
                .downcast_ref::<Decimal128Array>()
                .unwrap();
            let mut observed = BTreeSet::new();
            for row in 0..arrow.num_rows() {
                assert!(!keys.is_null(row));
                observed.insert(keys.value(row).to_owned());
                assert_eq!(labels.value(row), format!("label-{}", keys.value(row)));
                assert_eq!(amounts.value(row), -12345);
                assert_eq!(counters.value(row), i128::from(u64::MAX));
                let times = arrow
                    .column(4)
                    .as_any()
                    .downcast_ref::<TimestampMicrosecondArray>()
                    .unwrap();
                assert_eq!(times.value(row), 1_709_210_096_123_456);
                let cell = raw_cell(&batch.rows[row], 4).unwrap();
                assert!(matches!(
                    cell,
                    crate::mysql::values::RawCell::Date {
                        year: 2024,
                        month: 2,
                        day: 29,
                        micros: 123456,
                        ..
                    }
                ));
            }
            assert_eq!(observed, expected.iter().map(|s| s.to_string()).collect());
        }
        owned.into_connection().unwrap().disconnect().await.unwrap();
        let audit: Vec<(String, String)> = admin.exec("SELECT command_type, argument FROM mysql.general_log WHERE thread_id = ? AND event_time >= ? ORDER BY event_time", (id, audit_start)).await.unwrap();
        let mut saw_snapshot = false;
        let mut saw_rr = false;
        for (command, sql) in audit {
            if !["Query", "Prepare", "Execute"].contains(&command.as_str()) {
                continue;
            }
            saw_snapshot |= sql == "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY";
            saw_rr |= sql == "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ";
            let verb = sql
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_uppercase();
            assert!(
                ["SELECT", "SHOW", "SET", "START", "COMMIT"].contains(&verb.as_str()),
                "unexpected source statement verb {verb}"
            );
            assert!(!sql.to_ascii_uppercase().contains("GTID"));
        }
        assert!(saw_snapshot && saw_rr);
        for (rows, bytes, expected_fact) in [(4, 65536, "max_rows"), (5, 1, "max_bytes")] {
            let conn = connect(&reader_config, password).await.unwrap();
            let failed_id = conn.id();
            let mut failed = ReadConnection::new(conn, reader_config.clone(), password.clone());
            let error = full(rows, bytes, 30000)
                .read(&mut failed, &Cancel::default(), &mut Vec::new())
                .await
                .err()
                .unwrap();
            assert_eq!(error.fact, expected_fact);
            assert!(failed.into_connection().is_err());
            let active: Option<u64> = admin
                .exec_first(
                    "SELECT COUNT(*) FROM information_schema.PROCESSLIST WHERE ID = ?",
                    (failed_id,),
                )
                .await
                .unwrap();
            assert_eq!(active, Some(0));
        }
        let conn = connect(&reader_config, password).await.unwrap();
        let mut cancelled = ReadConnection::new(conn, reader_config.clone(), password.clone());
        let cancel = Cancel::default();
        cancel.cancel();
        assert_eq!(
            full(5, 65536, 30000)
                .read(&mut cancelled, &cancel, &mut Vec::new())
                .await
                .err()
                .unwrap()
                .fact,
            "requested"
        );
        assert!(cancelled.into_connection().is_err());
        let conn = connect(&reader_config, password).await.unwrap();
        let mut expired = ReadConnection::new(conn, reader_config.clone(), password.clone());
        let reader = full(5, 65536, 1);
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            reader
                .read(&mut expired, &Cancel::default(), &mut Vec::new())
                .await
                .err()
                .unwrap()
                .fact,
            "snapshot_deadline"
        );
        assert!(expired.into_connection().is_err());
        admin.query_drop("CREATE OR REPLACE VIEW full_slow AS SELECT id, label, amount, counter, happened FROM full_items WHERE SLEEP(1) = 0").await.unwrap();
        for in_flight_cancel in [false, true] {
            let mut slow_binding = binding.clone();
            slow_binding.table = "full_slow".to_owned();
            let slow = FullReader::new(
                &config.mysql_database,
                "full_slow",
                mapped.clone(),
                slow_binding,
                5,
                65536,
                30000,
                if in_flight_cancel { 30000 } else { 10 },
            )
            .unwrap();
            let conn = connect(&reader_config, password).await.unwrap();
            let slow_id = conn.id();
            let mut owned = ReadConnection::new(conn, reader_config.clone(), password.clone());
            let cancel = Cancel::default();
            let signal = cancel.clone();
            let signal_task = in_flight_cancel.then(|| {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    signal.cancel();
                })
            });
            let error = slow
                .read(&mut owned, &cancel, &mut Vec::new())
                .await
                .err()
                .unwrap();
            assert_eq!(
                error.fact,
                if in_flight_cancel {
                    "requested"
                } else {
                    "statement_timeout"
                }
            );
            assert!(owned.into_connection().is_err());
            if let Some(task) = signal_task {
                task.await.unwrap();
            }
            let active: Option<u64> = admin
                .exec_first(
                    "SELECT COUNT(*) FROM information_schema.PROCESSLIST WHERE ID = ?",
                    (slow_id,),
                )
                .await
                .unwrap();
            assert_eq!(active, Some(0));
        }
        admin.query_drop("DROP VIEW full_slow").await.unwrap();
        admin
            .query_drop("ALTER TABLE full_items ENGINE=MyISAM")
            .await
            .unwrap();
        let conn = connect(&reader_config, password).await.unwrap();
        let mut unsupported = ReadConnection::new(conn, reader_config.clone(), password.clone());
        assert_eq!(
            full(5, 65536, 30000)
                .read(&mut unsupported, &Cancel::default(), &mut Vec::new())
                .await
                .err()
                .unwrap()
                .fact,
            "snapshot_engine"
        );
        assert!(unsupported.into_connection().is_err());
        admin
            .query_drop("ALTER TABLE full_items ENGINE=InnoDB")
            .await
            .unwrap();
        admin.query_drop("DELETE FROM full_items").await.unwrap();
        let mut conn = connect(&reader_config, password).await.unwrap();
        let empty = full(5, 65536, 30000)
            .read_on(&mut conn, &Cancel::default(), &mut Vec::new())
            .await
            .unwrap();
        assert_eq!(empty.typed.record_batch().unwrap().num_rows(), 0);
        conn.disconnect().await.unwrap();
        admin.query_drop("DROP TABLE full_items").await.unwrap();
        admin
            .query_drop("DROP USER 'full_reader_fixture'@'%'")
            .await
            .unwrap();
        admin.disconnect().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires OO_MYSQL_CHUNK_FIXTURE: an owned MySQL 8.x server with TLS"]
    async fn chunks_cover_every_key_once_on_a_live_server() {
        let (config, password) = open().await;
        let schema = config.mysql_database.clone();
        let mut conn = connect(&config, &password).await.unwrap();
        let autocommit: i64 = conn
            .query_first("SELECT @@autocommit")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(autocommit, 1, "chunk reads assume autocommit");

        // Unsigned keys: 2000 small keys, then the ten largest u64 values.
        let mut unsigned: Vec<u64> = (1..=2000).collect();
        unsigned.extend(u64::MAX - 9..=u64::MAX);
        conn.query_drop("DROP TABLE IF EXISTS cb_chunk_u")
            .await
            .unwrap();
        conn.query_drop(
            "CREATE TABLE cb_chunk_u (id BIGINT UNSIGNED NOT NULL PRIMARY KEY, \
             payload VARCHAR(400) CHARACTER SET utf8mb4 NOT NULL)",
        )
        .await
        .unwrap();
        let rows: Vec<(u64, String)> = unsigned
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, "x".repeat(20 + (i * 37) % 300)))
            .collect();
        conn.exec_batch("INSERT INTO cb_chunk_u (id, payload) VALUES (?, ?)", rows)
            .await
            .unwrap();

        // Signed keys across zero, to exercise negative bounds.
        let signed: Vec<i64> = (-1000..=1000).collect();
        conn.query_drop("DROP TABLE IF EXISTS cb_chunk_s")
            .await
            .unwrap();
        conn.query_drop(
            "CREATE TABLE cb_chunk_s (id BIGINT NOT NULL PRIMARY KEY, \
             payload VARCHAR(64) CHARACTER SET utf8mb4 NOT NULL)",
        )
        .await
        .unwrap();
        let rows: Vec<(i64, String)> = signed.iter().map(|id| (*id, format!("p{id}"))).collect();
        conn.exec_batch("INSERT INTO cb_chunk_s (id, payload) VALUES (?, ?)", rows)
            .await
            .unwrap();

        let unsigned_columns = columns(&mut conn, &schema, "cb_chunk_u").await;
        let signed_columns = columns(&mut conn, &schema, "cb_chunk_s").await;
        let binding = |table: &str| RunBinding {
            source_id: "live".to_owned(),
            schema: schema.clone(),
            table: table.to_owned(),
            run_generation: 1,
            input_digest: [0; 32],
        };
        let by_rows = ChunkReader::new(
            &schema,
            "cb_chunk_u",
            unsigned_columns.clone(),
            binding("cb_chunk_u"),
            700,
            CHUNK_MAX_BYTES,
            CHUNK_DEADLINE_MS,
            CHUNK_STATEMENT_MS,
        )
        .unwrap();
        let by_bytes = ChunkReader::new(
            &schema,
            "cb_chunk_u",
            unsigned_columns,
            binding("cb_chunk_u"),
            50_000,
            16 * 1024,
            CHUNK_DEADLINE_MS,
            CHUNK_STATEMENT_MS,
        )
        .unwrap();
        let signed_reader = ChunkReader::new(
            &schema,
            "cb_chunk_s",
            signed_columns,
            binding("cb_chunk_s"),
            300,
            CHUNK_MAX_BYTES,
            CHUNK_DEADLINE_MS,
            CHUNK_STATEMENT_MS,
        )
        .unwrap();

        let mut owned = ReadConnection::new(conn, config.clone(), password.clone());
        let unsigned_ids: Vec<i128> = unsigned.iter().map(|v| i128::from(*v)).collect();
        let signed_ids: Vec<i128> = signed.iter().map(|v| i128::from(*v)).collect();
        // 2010 rows at 700 per chunk is 3 chunks.
        assert_eq!(
            drain_and_check(&by_rows, &mut owned, &unsigned_ids, CHUNK_MAX_BYTES).await,
            3
        );
        assert!(drain_and_check(&by_bytes, &mut owned, &unsigned_ids, 16 * 1024).await > 3);
        // 2001 rows at 300 per chunk is 7 chunks.
        assert_eq!(
            drain_and_check(&signed_reader, &mut owned, &signed_ids, CHUNK_MAX_BYTES).await,
            7
        );

        // A cancelled chunk never reaches the server and discards its connection.
        let conn = owned.into_connection().unwrap();
        let mut cancelled = ReadConnection::new(conn, config.clone(), password.clone());
        let cancel = Cancel::default();
        cancel.cancel();
        let mut cursor = ChunkCursor::start();
        let err = by_rows
            .read_chunk(&mut cancelled, &mut cursor, &cancel)
            .await
            .err()
            .unwrap();
        assert_eq!((err.code, err.fact), ("cancel", "requested"));
        assert_eq!(cursor.after(), None);
        assert!(cancelled.into_connection().is_err());

        let mut conn = connect(&config, &password).await.unwrap();
        conn.query_drop("DROP TABLE cb_chunk_u").await.unwrap();
        conn.query_drop("DROP TABLE cb_chunk_s").await.unwrap();
        conn.disconnect().await.unwrap();
    }
}
