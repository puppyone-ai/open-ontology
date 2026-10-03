//! Typed key plan for one open snapshot. Protocol `oo.source.pgsql.keys/v1`.
//!
//! Planning runs in the owner's repeatable-read transaction. Bounds are
//! PostgreSQL row comparisons with bound parameters. This module does not
//! sort keys in Rust and does not invent a resume cursor.

use std::time::{Duration, Instant};

use sqlx::Row;

use crate::error::SourceError;
use crate::model::{SourceTable, UniqueKey};
use crate::session::{self, PgSession};
use crate::snapshot::{self, ProbeValue};
use crate::sql;
use crate::types::{
    compose_key_scan, compose_presence_scan, compose_scan, valid_quoted_ident, ColumnPlan,
    MapDecision,
};

/// Key-plan protocol. This is not the snapshot protocol and not the whole reader.
pub const KEY_PLAN_PROTOCOL: &str = "oo.source.pgsql.keys/v1";

/// Caller deadline length for one snapshot run. The owner does not start this
/// clock. `PgBudget` statement and lock timeouts stay capped at 120 seconds.
pub const SNAPSHOT_DEADLINE_DEFAULT: Duration = Duration::from_secs(300);

/// Default and maximum `plan_keys` target. A larger request is `config` fact `target_rows`.
pub const DEFAULT_TARGET_ROWS: u32 = 50_000;

/// Upper bound on chunks for one table. The stride widens when `count / target` exceeds this.
pub const MAX_PLAN_CHUNKS: u32 = 64;

/// Where the declared key came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclaredKeyKind {
    /// Supported plain primary key.
    Primary,
    /// Supported plain unique key used because the primary key is not a declared key.
    Unique,
}

/// One declared key column. `collation_name` is the server name. Scan SQL does not append `COLLATE`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyColumnPlan {
    pub name: String,
    pub attnum: i32,
    pub type_oid: i64,
    pub collation_name: Option<String>,
}

/// The one key used to plan a table. Absent on an unkeyed full-stream chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredKey {
    pub kind: DeclaredKeyKind,
    pub index_name: String,
    pub columns: Vec<KeyColumnPlan>,
}

/// Half-open key range. `None` is unbounded on that side. The owner compares the values in PostgreSQL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRange {
    pub lower: Option<Vec<ProbeValue>>,
    pub upper: Option<Vec<ProbeValue>>,
}

/// One planned slice. `resumable` is false. The loader treats anything other than
/// [`crate::SnapshotFinished`] as incomplete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedChunk {
    pub generation: u64,
    pub input_digest: [u8; 32],
    pub schema: String,
    pub table: String,
    pub ordinal: u32,
    pub keyed: bool,
    pub lower: Option<Vec<ProbeValue>>,
    pub upper: Option<Vec<ProbeValue>>,
    pub resumable: bool,
}

/// Plan for one selected table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableKeyPlan {
    pub schema: String,
    pub table: String,
    pub declared: Option<DeclaredKey>,
    /// `count(*)` inside the snapshot. Absent when the table has no declared key.
    pub row_count: Option<i64>,
    /// Page size used to walk boundaries. Absent when the table has no declared key.
    pub stride: Option<i64>,
    pub chunks: Vec<PlannedChunk>,
}

/// Plan for every selected table on this cut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPlan {
    pub protocol: &'static str,
    pub generation: u64,
    pub input_digest: [u8; 32],
    pub target_rows: u32,
    pub tables: Vec<TableKeyPlan>,
}

pub(crate) struct ReadSpec {
    pub sql: String,
    pub lower: Vec<ProbeValue>,
    pub upper: Vec<ProbeValue>,
    pub key_oids: Vec<i64>,
    pub key_attnums: Vec<i32>,
}

struct ResolvedKey {
    declared: DeclaredKey,
    quoted: Vec<String>,
    oids: Vec<i64>,
}

pub(crate) struct PlanIn<'a> {
    pub session: &'a mut PgSession,
    pub source: &'a SourceTable,
    pub schema_sql: &'a str,
    pub table_sql: &'a str,
    pub plans: &'a [ColumnPlan],
    pub generation: u64,
    pub input_digest: [u8; 32],
    pub target_rows: u32,
    pub deadline: Instant,
}

pub(crate) async fn plan_table(input: PlanIn<'_>) -> Result<TableKeyPlan, SourceError> {
    ensure_deadline(input.deadline)?;
    let resolved = resolve_key(input.session, input.source, input.plans, input.deadline).await?;
    let Some(resolved) = resolved else {
        return Ok(TableKeyPlan {
            schema: input.source.schema.clone(),
            table: input.source.name.clone(),
            declared: None,
            row_count: None,
            stride: None,
            chunks: assemble_chunks(
                input.generation,
                input.input_digest,
                &input.source.schema,
                &input.source.name,
                false,
                &[],
            )?,
        });
    };
    ensure_deadline(input.deadline)?;
    let count = read_count(input.session, input.schema_sql, input.table_sql).await?;
    let stride = widened_stride(count, i64::from(input.target_rows))?;
    let starts = if count == 0 {
        Vec::new()
    } else {
        walk_bounds(
            input.session,
            input.schema_sql,
            input.table_sql,
            &resolved,
            stride,
            input.deadline,
        )
        .await?
    };
    Ok(TableKeyPlan {
        schema: input.source.schema.clone(),
        table: input.source.name.clone(),
        declared: Some(resolved.declared),
        row_count: Some(count),
        stride: Some(stride),
        chunks: assemble_chunks(
            input.generation,
            input.input_digest,
            &input.source.schema,
            &input.source.name,
            true,
            &starts,
        )?,
    })
}

pub(crate) struct ReadIn<'a> {
    pub session: &'a mut PgSession,
    pub source: &'a SourceTable,
    pub plans: &'a [ColumnPlan],
    pub schema_sql: &'a str,
    pub table_sql: &'a str,
    pub select_sql: &'a [String],
    pub deadline: Instant,
}

pub(crate) async fn chunk_read(
    input: ReadIn<'_>,
    chunk: &PlannedChunk,
) -> Result<ReadSpec, SourceError> {
    if chunk.generation == 0 || chunk.resumable {
        return Err(SourceError::protocol("key_plan"));
    }
    if chunk.schema != input.source.schema || chunk.table != input.source.name {
        return Err(SourceError::protocol("key_plan"));
    }
    let resolved = resolve_key(input.session, input.source, input.plans, input.deadline).await?;
    spec_from_resolved(
        input.schema_sql,
        input.table_sql,
        input.select_sql,
        resolved.as_ref(),
        chunk.keyed,
        chunk.lower.clone(),
        chunk.upper.clone(),
    )
}

pub(crate) async fn range_read(
    input: ReadIn<'_>,
    range: &KeyRange,
) -> Result<ReadSpec, SourceError> {
    let resolved = resolve_key(input.session, input.source, input.plans, input.deadline).await?;
    if resolved.is_none() {
        return Err(SourceError::protocol("key_required"));
    }
    spec_from_resolved(
        input.schema_sql,
        input.table_sql,
        input.select_sql,
        resolved.as_ref(),
        true,
        range.lower.clone(),
        range.upper.clone(),
    )
}

fn spec_from_resolved(
    schema_sql: &str,
    table_sql: &str,
    select_sql: &[String],
    resolved: Option<&ResolvedKey>,
    keyed: bool,
    lower: Option<Vec<ProbeValue>>,
    upper: Option<Vec<ProbeValue>>,
) -> Result<ReadSpec, SourceError> {
    if keyed != resolved.is_some() {
        return Err(SourceError::protocol("key_plan"));
    }
    let Some(resolved) = resolved else {
        if lower.is_some() || upper.is_some() {
            return Err(SourceError::protocol("key_plan"));
        }
        let sql = if select_sql.is_empty() {
            compose_presence_scan(schema_sql, table_sql)?
        } else {
            compose_scan(schema_sql, table_sql, select_sql)?
        };
        return Ok(ReadSpec {
            sql,
            lower: Vec::new(),
            upper: Vec::new(),
            key_oids: Vec::new(),
            key_attnums: Vec::new(),
        });
    };
    let width = resolved.oids.len();
    let lower = bound_side(lower, width)?;
    let upper = bound_side(upper, width)?;
    if select_sql.is_empty() {
        return Err(SourceError::protocol("key_plan"));
    }
    let sql = compose_key_scan(
        schema_sql,
        table_sql,
        select_sql,
        &resolved.quoted,
        !lower.is_empty(),
        !upper.is_empty(),
    )?;
    Ok(ReadSpec {
        sql,
        lower,
        upper,
        key_oids: resolved.oids.clone(),
        key_attnums: resolved.declared.columns.iter().map(|column| column.attnum).collect(),
    })
}

fn bound_side(bound: Option<Vec<ProbeValue>>, width: usize) -> Result<Vec<ProbeValue>, SourceError> {
    match bound {
        None => Ok(Vec::new()),
        Some(values) if values.len() == width && !values.is_empty() => Ok(values),
        Some(_) => Err(SourceError::protocol("key_plan")),
    }
}

async fn resolve_key(
    session: &mut PgSession,
    source: &SourceTable,
    plans: &[ColumnPlan],
    deadline: Instant,
) -> Result<Option<ResolvedKey>, SourceError> {
    ensure_deadline(deadline)?;
    if let Some(columns) = local_columns(source, plans, &pk_attnums(source)) {
        let primary = read_primary(session, &source.schema, &source.name).await?;
        if primary.plain {
            return finish_key(session, source, DeclaredKeyKind::Primary, primary.name, columns, deadline)
                .await
                .map(Some);
        }
    }
    for unique in &source.unique_keys {
        let Some(columns) = local_columns(source, plans, &unique_attnums(unique)) else {
            continue;
        };
        if !index_plain(session, &source.schema, &source.name, &unique.name).await? {
            continue;
        }
        return finish_key(
            session,
            source,
            DeclaredKeyKind::Unique,
            unique.name.clone(),
            columns,
            deadline,
        )
        .await
        .map(Some);
    }
    Ok(None)
}

fn pk_attnums(source: &SourceTable) -> Vec<i32> {
    source.primary_key.iter().map(|key| key.attnum).collect()
}

fn unique_attnums(unique: &UniqueKey) -> Vec<i32> {
    unique.columns.iter().map(|key| key.attnum).collect()
}

fn local_columns(
    source: &SourceTable,
    plans: &[ColumnPlan],
    attnums: &[i32],
) -> Option<Vec<KeyColumnPlan>> {
    if attnums.is_empty() || attnums.len() > 8 {
        return None;
    }
    let mut columns = Vec::with_capacity(attnums.len());
    for attnum in attnums {
        if *attnum <= 0 {
            return None;
        }
        let column = source.columns.iter().find(|column| column.attnum == *attnum)?;
        if !column.not_null || !snapshot::supported_key_oid(column.type_oid) {
            return None;
        }
        let plan = plans.iter().find(|plan| plan.attnum == *attnum)?;
        if plan.mapping.decision != MapDecision::Lossless {
            return None;
        }
        columns.push(KeyColumnPlan {
            name: column.name.clone(),
            attnum: column.attnum,
            type_oid: column.type_oid,
            collation_name: None,
        });
    }
    Some(columns)
}

async fn finish_key(
    session: &mut PgSession,
    source: &SourceTable,
    kind: DeclaredKeyKind,
    index_name: String,
    mut columns: Vec<KeyColumnPlan>,
    deadline: Instant,
) -> Result<ResolvedKey, SourceError> {
    let mut quoted = Vec::with_capacity(columns.len());
    for column in &mut columns {
        ensure_deadline(deadline)?;
        quoted.push(quote_ident(session, &column.name).await?);
        if matches!(
            column.type_oid,
            snapshot::OID_TEXT | snapshot::OID_VARCHAR | snapshot::OID_BPCHAR
        ) {
            column.collation_name =
                read_collation(session, &source.schema, &source.name, &column.name).await?;
        }
    }
    let oids = columns.iter().map(|column| column.type_oid).collect();
    Ok(ResolvedKey {
        declared: DeclaredKey {
            kind,
            index_name,
            columns,
        },
        quoted,
        oids,
    })
}

struct PrimaryIndex {
    name: String,
    plain: bool,
}

async fn read_primary(
    session: &mut PgSession,
    schema: &str,
    table: &str,
) -> Result<PrimaryIndex, SourceError> {
    session.arm_rows(1);
    let rows = session::fetch_all(
        session,
        sqlx::query(sql::KEY_PRIMARY_PLAIN).bind(schema).bind(table),
    )
    .await?;
    let Some(row) = rows.first() else {
        return Err(SourceError::protocol("key_plan"));
    };
    if rows.len() != 1 {
        return Err(SourceError::protocol("key_plan"));
    }
    Ok(PrimaryIndex {
        name: session::cell_string(row, 0).map_err(|_| SourceError::protocol("key_plan"))?,
        plain: session::cell_bool(row, 1).map_err(|_| SourceError::protocol("key_plan"))?,
    })
}

async fn index_plain(
    session: &mut PgSession,
    schema: &str,
    table: &str,
    index: &str,
) -> Result<bool, SourceError> {
    session.arm_rows(1);
    let rows = session::fetch_all(
        session,
        sqlx::query(sql::KEY_INDEX_PLAIN)
            .bind(schema)
            .bind(table)
            .bind(index),
    )
    .await?;
    let Some(row) = rows.first() else {
        return Err(SourceError::protocol("key_plan"));
    };
    if rows.len() != 1 {
        return Err(SourceError::protocol("key_plan"));
    }
    session::cell_bool(row, 0).map_err(|_| SourceError::protocol("key_plan"))
}

async fn read_collation(
    session: &mut PgSession,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<Option<String>, SourceError> {
    session.arm_rows(1);
    let rows = session::fetch_all(
        session,
        sqlx::query(sql::KEY_COLLATION)
            .bind(schema)
            .bind(table)
            .bind(column),
    )
    .await?;
    let Some(row) = rows.first() else {
        return Err(SourceError::protocol("key_plan"));
    };
    session::cell_opt_string(row, 0).map_err(|_| SourceError::protocol("key_plan"))
}

async fn quote_ident(session: &mut PgSession, name: &str) -> Result<String, SourceError> {
    let row = session::fetch_one(session, sqlx::query(sql::QUOTE_IDENT).bind(name)).await?;
    let quoted = session::cell_string(&row, 0).map_err(|_| SourceError::catalog("quote_ident"))?;
    if !valid_quoted_ident(&quoted) {
        return Err(SourceError::catalog("quote_ident"));
    }
    Ok(quoted)
}

async fn read_count(
    session: &mut PgSession,
    schema_sql: &str,
    table_sql: &str,
) -> Result<i64, SourceError> {
    let sql = compose_count(schema_sql, table_sql)?;
    session.arm_rows(1);
    let rows = session::fetch_all(session, sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql))).await?;
    if rows.len() != 1 {
        return Err(SourceError::protocol("key_plan"));
    }
    session::cell_i64(&rows[0], 0).map_err(|_| SourceError::protocol("key_plan"))
}

async fn walk_bounds(
    session: &mut PgSession,
    schema_sql: &str,
    table_sql: &str,
    key: &ResolvedKey,
    stride: i64,
    deadline: Instant,
) -> Result<Vec<Vec<ProbeValue>>, SourceError> {
    ensure_deadline(deadline)?;
    let first = read_first(session, schema_sql, table_sql, key).await?;
    let Some(first) = first else {
        return Err(SourceError::protocol("key_plan"));
    };
    let mut starts = vec![first.clone()];
    let mut prev = first;
    loop {
        ensure_deadline(deadline)?;
        let Some((next, rows)) = read_page(session, schema_sql, table_sql, key, &prev, stride).await?
        else {
            break;
        };
        if rows < stride {
            break;
        }
        if rows != stride || next == prev || starts.len() >= usize::try_from(MAX_PLAN_CHUNKS).unwrap_or(64)
        {
            return Err(SourceError::protocol("key_plan"));
        }
        starts.push(next.clone());
        prev = next;
    }
    Ok(starts)
}

async fn read_first(
    session: &mut PgSession,
    schema_sql: &str,
    table_sql: &str,
    key: &ResolvedKey,
) -> Result<Option<Vec<ProbeValue>>, SourceError> {
    let sql = compose_first(schema_sql, table_sql, &key.quoted)?;
    session.arm_rows(1);
    let rows = session::fetch_all(session, sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql))).await?;
    match rows.as_slice() {
        [] => Ok(None),
        [row] => Ok(Some(decode_key(row, &key.oids)?)),
        _ => Err(SourceError::protocol("key_plan")),
    }
}

async fn read_page(
    session: &mut PgSession,
    schema_sql: &str,
    table_sql: &str,
    key: &ResolvedKey,
    prev: &[ProbeValue],
    stride: i64,
) -> Result<Option<(Vec<ProbeValue>, i64)>, SourceError> {
    let sql = compose_page(schema_sql, table_sql, &key.quoted)?;
    let mut query = sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql));
    if prev.len() != key.oids.len() {
        return Err(SourceError::protocol("key_plan"));
    }
    for (value, oid) in prev.iter().zip(key.oids.iter().copied()) {
        query = snapshot::bind_one(query, value, oid)?;
    }
    query = query.bind(stride);
    session.arm_rows(1);
    let rows = session::fetch_all(session, query).await?;
    match rows.as_slice() {
        [] => Ok(None),
        [row] => {
            let values = decode_key(row, &key.oids)?;
            let count = session::cell_i64(row, key.oids.len()).map_err(|_| SourceError::protocol("key_plan"))?;
            Ok(Some((values, count)))
        }
        _ => Err(SourceError::protocol("key_plan")),
    }
}

fn decode_key(row: &sqlx::postgres::PgRow, oids: &[i64]) -> Result<Vec<ProbeValue>, SourceError> {
    let mut values = Vec::with_capacity(oids.len());
    for (index, oid) in oids.iter().copied().enumerate() {
        values.push(decode_cell(row, index, oid)?);
    }
    Ok(values)
}

fn decode_cell(
    row: &sqlx::postgres::PgRow,
    index: usize,
    oid: i64,
) -> Result<ProbeValue, SourceError> {
    let bad = || SourceError::protocol("key_plan");
    match oid {
        snapshot::OID_INT2 => {
            let value: Option<i16> = row.try_get(index).map_err(|_| bad())?;
            Ok(ProbeValue::Int8(i64::from(value.ok_or_else(bad)?)))
        }
        snapshot::OID_INT4 => {
            let value: Option<i32> = row.try_get(index).map_err(|_| bad())?;
            Ok(ProbeValue::Int8(i64::from(value.ok_or_else(bad)?)))
        }
        snapshot::OID_INT8 => {
            let value: Option<i64> = row.try_get(index).map_err(|_| bad())?;
            Ok(ProbeValue::Int8(value.ok_or_else(bad)?))
        }
        snapshot::OID_BOOL => {
            let value: Option<bool> = row.try_get(index).map_err(|_| bad())?;
            Ok(ProbeValue::Bool(value.ok_or_else(bad)?))
        }
        snapshot::OID_TEXT | snapshot::OID_VARCHAR | snapshot::OID_BPCHAR => {
            let value: Option<String> = row.try_get(index).map_err(|_| bad())?;
            Ok(ProbeValue::Text(value.ok_or_else(bad)?))
        }
        _ => Err(bad()),
    }
}

pub(crate) fn compose_count(schema: &str, table: &str) -> Result<String, SourceError> {
    if !valid_quoted_ident(schema) || !valid_quoted_ident(table) {
        return Err(SourceError::catalog("quote_ident"));
    }
    let mut sql = String::from("SELECT count(*)::int8 FROM ");
    sql.push_str(schema);
    sql.push('.');
    sql.push_str(table);
    Ok(sql)
}

pub(crate) fn compose_first(
    schema: &str,
    table: &str,
    keys: &[String],
) -> Result<String, SourceError> {
    checked_keys(schema, table, keys)?;
    let mut sql = String::from("SELECT ");
    push_list(&mut sql, keys);
    sql.push_str(" FROM ");
    sql.push_str(schema);
    sql.push('.');
    sql.push_str(table);
    sql.push_str(" ORDER BY ");
    push_list(&mut sql, keys);
    sql.push_str(" LIMIT 1");
    Ok(sql)
}

pub(crate) fn compose_page(
    schema: &str,
    table: &str,
    keys: &[String],
) -> Result<String, SourceError> {
    checked_keys(schema, table, keys)?;
    let limit_at = keys.len() + 1;
    // The count column stays unnamed. A key column may already be named `n`.
    // The outer query orders by position so that name is never resolved.
    let mut sql = String::from("SELECT * FROM (SELECT ");
    push_list(&mut sql, keys);
    sql.push_str(", (count(*) OVER ())::int8 FROM (SELECT ");
    push_list(&mut sql, keys);
    sql.push_str(" FROM ");
    sql.push_str(schema);
    sql.push('.');
    sql.push_str(table);
    sql.push_str(" WHERE (");
    push_list(&mut sql, keys);
    sql.push_str(") > (");
    let mut next = 1usize;
    push_params(&mut sql, keys.len(), &mut next);
    sql.push_str(") ORDER BY ");
    push_list(&mut sql, keys);
    sql.push_str(" LIMIT $");
    sql.push_str(&limit_at.to_string());
    sql.push_str(") AS page) AS counted ORDER BY ");
    for index in 1..=keys.len() {
        if index > 1 {
            sql.push(',');
        }
        sql.push_str(&index.to_string());
        sql.push_str(" DESC");
    }
    sql.push_str(" LIMIT 1");
    Ok(sql)
}

fn checked_keys(schema: &str, table: &str, keys: &[String]) -> Result<(), SourceError> {
    if keys.is_empty() || keys.len() > 8 || !valid_quoted_ident(schema) || !valid_quoted_ident(table) {
        return Err(SourceError::protocol("key_plan"));
    }
    if keys.iter().any(|key| !valid_quoted_ident(key)) {
        return Err(SourceError::catalog("quote_ident"));
    }
    Ok(())
}

fn push_list(sql: &mut String, items: &[String]) {
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            sql.push(',');
        }
        sql.push_str(item);
    }
}

fn push_params(sql: &mut String, count: usize, next: &mut usize) {
    for index in 0..count {
        if index > 0 {
            sql.push(',');
        }
        sql.push('$');
        sql.push_str(&next.to_string());
        *next += 1;
    }
}

pub(crate) fn widened_stride(count: i64, target: i64) -> Result<i64, SourceError> {
    if target < 1 || target > i64::from(DEFAULT_TARGET_ROWS) {
        return Err(SourceError::config("target_rows"));
    }
    if count < 0 {
        return Err(SourceError::protocol("key_plan"));
    }
    if count == 0 {
        return Ok(target);
    }
    let cap = i64::from(MAX_PLAN_CHUNKS);
    if ceil_div(count, target)? <= cap {
        return Ok(target);
    }
    ceil_div(count, cap)
}

fn ceil_div(numerator: i64, denominator: i64) -> Result<i64, SourceError> {
    if numerator < 0 || denominator <= 0 {
        return Err(SourceError::protocol("key_plan"));
    }
    if numerator == 0 {
        return Ok(0);
    }
    let quotient = numerator / denominator;
    if numerator % denominator == 0 {
        Ok(quotient)
    } else {
        quotient.checked_add(1).ok_or_else(|| SourceError::protocol("key_plan"))
    }
}

pub(crate) fn assemble_chunks(
    generation: u64,
    input_digest: [u8; 32],
    schema: &str,
    table: &str,
    keyed: bool,
    starts: &[Vec<ProbeValue>],
) -> Result<Vec<PlannedChunk>, SourceError> {
    let id = ChunkId {
        generation,
        input_digest,
        schema,
        table,
    };
    if !keyed {
        if !starts.is_empty() {
            return Err(SourceError::protocol("key_plan"));
        }
        return Ok(vec![one_chunk(id, 0, false, None, None)]);
    }
    if starts.is_empty() {
        return Ok(vec![one_chunk(id, 0, true, None, None)]);
    }
    if starts.len() > usize::try_from(MAX_PLAN_CHUNKS).unwrap_or(64) {
        return Err(SourceError::protocol("key_plan"));
    }
    let mut chunks = Vec::with_capacity(starts.len());
    for (index, start) in starts.iter().enumerate() {
        if start.is_empty() || (index > 0 && start == &starts[index - 1]) {
            return Err(SourceError::protocol("key_plan"));
        }
        let upper = if index + 1 == starts.len() {
            None
        } else {
            Some(starts[index + 1].clone())
        };
        chunks.push(one_chunk(
            id,
            u32::try_from(index).unwrap_or(u32::MAX),
            true,
            Some(start.clone()),
            upper,
        ));
    }
    Ok(chunks)
}

#[derive(Clone, Copy)]
struct ChunkId<'a> {
    generation: u64,
    input_digest: [u8; 32],
    schema: &'a str,
    table: &'a str,
}

fn one_chunk(
    id: ChunkId<'_>,
    ordinal: u32,
    keyed: bool,
    lower: Option<Vec<ProbeValue>>,
    upper: Option<Vec<ProbeValue>>,
) -> PlannedChunk {
    PlannedChunk {
        generation: id.generation,
        input_digest: id.input_digest,
        schema: id.schema.to_string(),
        table: id.table.to_string(),
        ordinal,
        keyed,
        lower,
        upper,
        resumable: false,
    }
}

fn ensure_deadline(deadline: Instant) -> Result<(), SourceError> {
    if Instant::now() >= deadline {
        Err(SourceError::budget("deadline"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_key(value: &str) -> ProbeValue {
        ProbeValue::Text(value.to_string())
    }

    #[test]
    fn deadline_default_is_300_seconds() {
        assert_eq!(SNAPSHOT_DEADLINE_DEFAULT, Duration::from_secs(300));
        assert_eq!(DEFAULT_TARGET_ROWS, 50_000);
        assert_eq!(MAX_PLAN_CHUNKS, 64);
        assert_eq!(KEY_PLAN_PROTOCOL, "oo.source.pgsql.keys/v1");
    }

    #[test]
    fn stride_stays_when_the_chunk_count_fits() {
        assert_eq!(widened_stride(100, 2).unwrap(), 2);
        assert_eq!(widened_stride(0, 2).unwrap(), 2);
        assert_eq!(widened_stride(64, 1).unwrap(), 1);
    }

    #[test]
    fn stride_widens_to_the_chunk_cap() {
        assert_eq!(widened_stride(200, 2).unwrap(), 4);
        assert_eq!(widened_stride(65, 1).unwrap(), 2);
        assert_eq!(
            widened_stride(1, 0).unwrap_err().fact(),
            Some("target_rows")
        );
        assert_eq!(
            widened_stride(1, 50_001).unwrap_err().fact(),
            Some("target_rows")
        );
    }

    #[test]
    fn half_open_chunks_cover_starts_without_a_resume_flag() {
        let digest = [7_u8; 32];
        let starts = vec![
            vec![ProbeValue::Int8(-40)],
            vec![ProbeValue::Int8(0)],
            vec![ProbeValue::Int8(10)],
        ];
        let chunks = assemble_chunks(3, digest, "r3k", "sparse", true, &starts).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].lower, Some(vec![ProbeValue::Int8(-40)]));
        assert_eq!(chunks[0].upper, Some(vec![ProbeValue::Int8(0)]));
        assert_eq!(chunks[1].lower, Some(vec![ProbeValue::Int8(0)]));
        assert_eq!(chunks[1].upper, Some(vec![ProbeValue::Int8(10)]));
        assert_eq!(chunks[2].lower, Some(vec![ProbeValue::Int8(10)]));
        assert_eq!(chunks[2].upper, None);
        assert!(chunks.iter().all(|chunk| !chunk.resumable && chunk.keyed));
        let empty = assemble_chunks(3, digest, "r3k", "sparse", true, &[]).unwrap();
        assert_eq!(empty.len(), 1);
        assert!(empty[0].lower.is_none() && empty[0].upper.is_none() && empty[0].keyed);
        let nokey = assemble_chunks(3, digest, "r3k", "nokey", false, &[]).unwrap();
        assert!(!nokey[0].keyed && !nokey[0].resumable);
    }

    #[test]
    fn key_scan_is_parameterized_and_has_no_offset_or_collate() {
        let sql = compose_key_scan(
            "\"s\"",
            "\"t\"",
            &["\"a\"".to_string(), "\"b\"".to_string()],
            &["\"k\"".to_string(), "\"n\"".to_string()],
            true,
            true,
        )
        .unwrap();
        assert_eq!(
            sql,
            "SELECT \"a\",\"b\" FROM \"s\".\"t\" WHERE (\"k\",\"n\") >= ($1,$2) AND (\"k\",\"n\") < ($3,$4) ORDER BY \"k\",\"n\""
        );
        let upper = sql.to_ascii_uppercase();
        assert!(!upper.contains("OFFSET"));
        assert!(!upper.contains("COLLATE"));
        assert!(!upper.contains(" LIMIT"));
        let open = compose_key_scan(
            "\"s\"",
            "\"t\"",
            &["\"a\"".to_string()],
            &["\"k\"".to_string()],
            false,
            false,
        )
        .unwrap();
        assert_eq!(open, "SELECT \"a\" FROM \"s\".\"t\" ORDER BY \"k\"");
        assert!(!open.contains("WHERE"));
    }

    #[test]
    fn page_sql_uses_limit_and_row_comparison() {
        let page = compose_page("\"s\"", "\"t\"", &["\"k\"".to_string()]).unwrap();
        assert!(page.contains("WHERE (\"k\") > ($1)"));
        assert!(page.contains("LIMIT $2"));
        assert!(page.contains("ORDER BY \"k\""));
        assert!(page.contains("ORDER BY 1 DESC"));
        assert!(!page.to_ascii_uppercase().contains("OFFSET"));
        assert!(!page.to_ascii_uppercase().contains("COLLATE"));
        let composite = compose_page(
            "\"s\"",
            "\"t\"",
            &["\"name\"".to_string(), "\"n\"".to_string()],
        )
        .unwrap();
        assert!(composite.contains("ORDER BY 1 DESC,2 DESC"));
        assert!(!composite.contains(" AS n"));
        let first = compose_first("\"s\"", "\"t\"", &["\"k\"".to_string(), "\"n\"".to_string()]).unwrap();
        assert_eq!(
            first,
            "SELECT \"k\",\"n\" FROM \"s\".\"t\" ORDER BY \"k\",\"n\" LIMIT 1"
        );
        let _ = text_key("a");
    }
}
