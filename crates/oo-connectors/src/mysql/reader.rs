//! One page is pulled only when requested. Existing C1 snapshot transactions can
//! use this same reader without opening a second business snapshot.
use super::{
    MysqlConfig, MysqlError,
    map::MappedColumn,
    raw_cell,
    values::{self, RawCell},
};
use crate::types::{RunBinding, TypedBatch, assemble_batch};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row};
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
        if !super::mysql_ident(schema)
            || !super::mysql_ident(table)
            || columns.is_empty()
            || columns.iter().any(|c| !super::mysql_ident(&c.plan.name))
            || columns.iter().filter(|c| c.key).count() != 1
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
        let mut decoded = Vec::with_capacity(rows.len());
        let mut rejects = Vec::new();
        for (row_index, row) in rows.iter().enumerate() {
            let mut cells = Vec::with_capacity(self.columns.len());
            for (column_index, c) in self.columns.iter().enumerate() {
                let (cell, reject) = values::decode_cell(
                    c.store,
                    &raw_cell(row, column_index)?,
                    c.plan.mapping.nullable,
                )?;
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
        if arrow.get_array_memory_size() as u64 > BATCH_BYTES {
            return Err(MysqlError::new("budget", "batch_bytes"));
        }
        self.rows += rows.len() as u64;
        self.bytes += page_bytes;
        self.done |= eof && (rows.len() as u64) < limit;
        Ok(Some(Batch {
            typed,
            rows,
            bytes: page_bytes,
            rejects,
            statement: super::StatementNote {
                verb: "SELECT".to_owned(),
                sha256: super::sha256_hex(sql.as_bytes()),
            },
        }))
    }
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
