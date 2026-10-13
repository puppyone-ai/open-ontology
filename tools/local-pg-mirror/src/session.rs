//! One repeatable-read read-only source cut.
//! Catalog, keyset pages, and the health read share this transaction.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use oo_connectors::types::{assemble_batch, map_column, ColumnPlan, RunBinding, TypeFacts, TypeGraph};
use oo_connectors::{
    numeric_parts, stable_ident, ColumnPrivileges, SourceColumn, TypeSupport, TypeUnsupportedReason,
    PG17_RESERVED,
};
use oo_pipeline::copy_encoder::{
    CopyBudgets, CopyColumn, CopyEncoder, CopyPiece, ExactDecimal, MapDecision, UnavailablePolicy,
};
use oo_pipeline::copy_sink::hash_batch;
use oo_pipeline::target_installer::sha256_hex;
use sqlx::postgres::{PgConnection, PgValueFormat};
use sqlx::{Connection, Row, ValueRef};

use crate::codec::{self, key_i64};
use crate::error::MirrorError;
use crate::ident::quote_ident;
use crate::manifest::Quarantine;
use crate::pg;
use crate::plan::{Plan, TableShape};
use crate::spool::{self, BatchBlob, Header};

const APPROVED: &[i64] = &[
    16, 17, 20, 21, 23, 25, 114, 700, 701, 1042, 1043, 1082, 1083, 1114, 1184, 1700, 2950,
];

pub struct Digests {
    pub config_digest: String,
    pub type_digest: String,
    pub input_digest: String,
    pub input_bytes: [u8; 32],
    pub schema_digest: String,
    pub hook_digest: String,
}

pub enum CutMode {
    Finish,
    Cancel,
    StopAfter(u64),
    Drop,
}

pub struct Measured {
    pub peak: u64,
    pub fact: &'static str,
    pub snapshot_id: Option<String>,
    pub backend_pid: Option<i64>,
    pub read_only: bool,
    pub write_tuples: Option<i64>,
}

pub enum CutKind {
    Ready,
    Dropped,
    Stopped,
    Cancelled,
    Failed,
}

pub struct Cut {
    pub measured: Measured,
    pub kind: CutKind,
    pub state: &'static str,
    pub failure: &'static str,
    pub batches: Vec<BatchBlob>,
    pub quarantine: Vec<Quarantine>,
    pub spool: &'static str,
}

struct Health {
    session_user: String,
    current_user: String,
    database: String,
    in_recovery: bool,
    postmaster: String,
    read_only: String,
    snapshot_id: String,
    backend_pid: i64,
    isolation: String,
}

struct LiveTable {
    index: u32,
    shape: TableShape,
    plans: Vec<ColumnPlan>,
    key_index: usize,
    key_oid: i64,
    key_name: String,
    next_pos: u64,
    batches: u64,
    rows: u64,
    key_hold: i64,
    started: bool,
}

#[allow(clippy::too_many_arguments)]
pub async fn read_cut(
    mut conn: PgConnection,
    plan: &Plan,
    deadline: Instant,
    budget: Duration,
    mode: CutMode,
    spool_dir: PathBuf,
    digests: &Digests,
    copy_budget: CopyBudgets,
) -> Cut {
    let result = read_inner(&mut conn, plan, deadline, budget, mode, &spool_dir, digests, copy_budget).await;
    drop(conn);
    result
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn read_inner(
    conn: &mut PgConnection,
    plan: &Plan,
    deadline: Instant,
    _budget: Duration,
    mode: CutMode,
    spool_dir: &Path,
    digests: &Digests,
    copy_budget: CopyBudgets,
) -> Cut {
    if Instant::now() >= deadline {
        return failed("incomplete", "deadline", false, None, None, Vec::new(), "none");
    }
    let mut tx = match conn.begin().await {
        Ok(tx) => tx,
        Err(err) => return failed_from(pg::map_sqlx(err), false),
    };
    if let Err(err) = sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
    {
        return failed_from(pg::map_sqlx(err), false);
    }
    let health = match health_row(&mut tx).await {
        Ok(health) => health,
        Err(err) => return failed_from(pg::map_sqlx(err), false),
    };
    if health.isolation != "repeatable read" || health.read_only != "on" {
        return failed("refused", "isolation", true, Some(health), None, Vec::new(), "none");
    }
    if health.session_user != plan.expected_principal
        || health.current_user != plan.expected_principal
        || health.database != plan.expected_database
        || health.in_recovery != plan.expected_in_recovery
        || health.postmaster != plan.expected_postmaster
    {
        return failed(
            "refused",
            "identity_drift",
            true,
            Some(health),
            None,
            Vec::new(),
            "none",
        );
    }
    let mut peak = match sample_peak(&mut tx).await {
        Ok(peak) => peak,
        Err(err) => return failed_from(err, true),
    };
    if peak > 1 {
        return failed("refused", "source_peak", true, Some(health), Some(peak), Vec::new(), "none");
    }
    let writes_at = match sample_writes(&mut tx).await {
        Ok(writes) => writes,
        Err(err) => return failed_from(err, true),
    };
    let reserved = reserved_words();
    let mut tables = Vec::new();
    let mut quarantine = Vec::new();
    for (index, admitted) in plan.admitted.iter().enumerate() {
        let shape = match read_shape(&mut tx, admitted).await {
            Ok(shape) => shape,
            Err(err) => return failed_from(err, true),
        };
        if &shape != admitted {
            return failed(
                "refused",
                "catalog_drift",
                true,
                Some(health),
                Some(peak),
                Vec::new(),
                "none",
            );
        }
        if !select_allowed(&mut tx, &shape.schema, &shape.name).await.unwrap_or(false) {
            return failed(
                "refused",
                "select_not_granted",
                true,
                Some(health),
                Some(peak),
                Vec::new(),
                "none",
            );
        }
        let facts = match read_facts(&mut tx, &shape).await {
            Ok(facts) => facts,
            Err(err) => return failed_from(err, true),
        };
        match approve_table(&shape, &facts, &reserved, index) {
            Ok(live) => tables.push(live),
            Err(ApproveFail::Columns(rows)) => quarantine.extend(rows),
            Err(ApproveFail::Key(fact)) => {
                return failed("refused", fact, true, Some(health), Some(peak), Vec::new(), "none");
            }
        }
    }
    if !quarantine.is_empty() {
        return failed(
            "refused",
            "unsupported_column",
            true,
            Some(health),
            Some(peak),
            quarantine,
            "none",
        );
    }
    if matches!(mode, CutMode::Cancel) {
        let end = finish_measure(&mut tx, &health, peak, writes_at).await;
        let _ = tx.rollback().await;
        return end_cut(end, CutKind::Cancelled, "incomplete", "cancel", Vec::new(), "none");
    }
    if Instant::now() >= deadline {
        let _ = tx.rollback().await;
        return failed("incomplete", "deadline", true, Some(health), Some(peak), Vec::new(), "none");
    }
    let header = Header {
        plan_sha256: plan.plan_sha256.clone(),
        generation: plan.config.generation,
        snapshot_id: health.snapshot_id.clone(),
        config_digest: digests.config_digest.clone(),
        type_digest: digests.type_digest.clone(),
        input_digest: digests.input_digest.clone(),
        schema_digest: digests.schema_digest.clone(),
    };
    let mut writer = match spool::begin(spool_dir, header) {
        Ok(writer) => writer,
        Err(err) => {
            let _ = tx.rollback().await;
            return failed("refused", err.fact, true, Some(health), Some(peak), Vec::new(), "none");
        }
    };
    let mut batches = Vec::new();
    let limit = match mode {
        CutMode::StopAfter(limit) => Some(limit),
        _ => None,
    };
    let mut encoded = 0u64;
    let read = read_tables(
        &mut tx,
        plan,
        digests,
        copy_budget,
        &mut tables,
        &mut writer,
        &mut batches,
        &mut peak,
        &mut encoded,
        limit,
        deadline,
    )
    .await;
    if let Err(fact) = read {
        let _ = tx.rollback().await;
        let _ = spool::discard(spool_dir);
        let state = if fact == "deadline" || fact == "statement_timeout" || fact == "lock_timeout" {
            "incomplete"
        } else {
            "refused"
        };
        return failed(state, fact, true, Some(health), Some(peak), Vec::new(), "none");
    }
    let end_writes = match sample_writes(&mut tx).await {
        Ok(writes) => writes,
        Err(err) => {
            let _ = tx.rollback().await;
            let _ = spool::discard(spool_dir);
            return failed_from(err, true);
        }
    };
    peak = peak.max(match sample_peak(&mut tx).await {
        Ok(value) => value,
        Err(err) => {
            let _ = tx.rollback().await;
            let _ = spool::discard(spool_dir);
            return failed_from(err, true);
        }
    });
    let delta = (end_writes.0 - writes_at.0) + (end_writes.1 - writes_at.1) + (end_writes.2 - writes_at.2);
    if delta != 0 || peak > 1 {
        let _ = tx.rollback().await;
        let _ = spool::discard(spool_dir);
        let fact = if peak > 1 { "source_peak" } else { "source_write" };
        return failed("refused", fact, true, Some(health), Some(peak), Vec::new(), "none");
    }
    let measured = measured_of(&health, peak, Some(delta));
    if matches!(mode, CutMode::StopAfter(_)) {
        let _ = tx.rollback().await;
        let _ = spool::discard(spool_dir);
        return end_cut(Ok(measured), CutKind::Stopped, "incomplete", "stop_after_batches", batches, "none");
    }
    if writer.complete().is_err() {
        let _ = tx.rollback().await;
        let _ = spool::discard(spool_dir);
        return failed("refused", "spool", true, Some(health), Some(peak), Vec::new(), "none");
    }
    let _ = tx.rollback().await;
    let kind = if matches!(mode, CutMode::Drop) {
        CutKind::Dropped
    } else {
        CutKind::Ready
    };
    let (state, failure) = if matches!(mode, CutMode::Drop) {
        ("incomplete", "source_dropped")
    } else {
        ("complete", "none")
    };
    end_cut(Ok(measured), kind, state, failure, batches, "complete")
}

#[allow(clippy::too_many_arguments)]
async fn read_tables(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    plan: &Plan,
    digests: &Digests,
    copy_budget: CopyBudgets,
    tables: &mut [LiveTable],
    writer: &mut spool::Writer,
    batches: &mut Vec<BatchBlob>,
    peak: &mut u64,
    encoded: &mut u64,
    limit: Option<u64>,
    deadline: Instant,
) -> Result<(), &'static str> {
    for table in tables.iter_mut() {
        loop {
            if Instant::now() >= deadline {
                return Err("deadline");
            }
            if let Some(limit) = limit && *encoded >= limit {
                return Ok(());
            }
            let batch_rows = plan.config.batch_rows;
            let page = fetch_page(tx, table, batch_rows).await.map_err(|err| err.fact)?;
            if page.is_empty() {
                if table.rows == 0 && table.batches == 0 {
                    let blob = encode_page(plan, digests, copy_budget, table, &[])?;
                    writer.push(&blob).map_err(|err| err.fact)?;
                    table.batches += 1;
                    batches.push(blob);
                    *encoded += 1;
                }
                break;
            }
            let got = page.len() as u64;
            if table.rows + got > plan.config.max_rows {
                return Err("max_rows");
            }
            let blob = encode_page(plan, digests, copy_budget, table, &page)?;
            if blob.copy_bytes.len() as u64 > plan.config.max_bytes {
                return Err("max_bytes");
            }
            writer.push(&blob).map_err(|err| err.fact)?;
            table.rows += blob.rows;
            table.batches += 1;
            table.next_pos += blob.rows;
            let key = page_key(&page, table).ok_or("key_type")?;
            table.key_hold = key;
            table.started = true;
            batches.push(blob);
            *encoded += 1;
            *peak = (*peak).max(sample_peak(tx).await.map_err(|err| err.fact)?);
            if *peak > 1 {
                return Err("source_peak");
            }
            if got < batch_rows {
                if table.rows == plan.config.max_rows {
                    let extra = fetch_page(tx, table, 1).await.map_err(|err| err.fact)?;
                    if !extra.is_empty() {
                        return Err("max_rows");
                    }
                }
                break;
            }
        }
    }
    Ok(())
}

fn page_key(page: &[Vec<Option<Vec<u8>>>], table: &LiveTable) -> Option<i64> {
    let cell = page.last()?.get(table.key_index)?;
    let bytes = cell.as_ref()?;
    key_i64(bytes, table.key_oid).ok()
}

async fn fetch_page(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &LiveTable,
    limit: u64,
) -> Result<Vec<Vec<Option<Vec<u8>>>>, MirrorError> {
    let sql = page_sql(table, table.started)?;
    let limit = i64::try_from(limit).map_err(|_| MirrorError::new("budget", "batch_rows"))?;
    let rows = if table.started {
        let key = table.key_hold;
        match table.key_oid {
            20 => {
                sqlx::query(sqlx::AssertSqlSafe(sql))
                    .bind(key)
                    .bind(limit)
                    .fetch_all(&mut **tx)
                    .await
            }
            23 => {
                let key = i32::try_from(key).map_err(|_| MirrorError::new("refused", "key_type"))?;
                sqlx::query(sqlx::AssertSqlSafe(sql))
                    .bind(key)
                    .bind(limit)
                    .fetch_all(&mut **tx)
                    .await
            }
            21 => {
                let key = i16::try_from(key).map_err(|_| MirrorError::new("refused", "key_type"))?;
                sqlx::query(sqlx::AssertSqlSafe(sql))
                    .bind(key)
                    .bind(limit)
                    .fetch_all(&mut **tx)
                    .await
            }
            _ => return Err(MirrorError::new("refused", "key_type")),
        }
    } else {
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(limit)
            .fetch_all(&mut **tx)
            .await
    }
    .map_err(pg::map_sqlx)?;
    let mut page = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut cells = Vec::with_capacity(table.plans.len());
        for (index, plan) in table.plans.iter().enumerate() {
            let raw = row.try_get_raw(index).map_err(pg::map_sqlx)?;
            if raw.format() != PgValueFormat::Binary {
                return Err(MirrorError::new("refused", "codec"));
            }
            if let Some(oid) = raw.type_info().oid() {
                let oid = i64::from(oid.0);
                if oid != plan.oid && oid != plan.decode_oid {
                    return Err(MirrorError::new("refused", "catalog_drift"));
                }
            }
            if raw.is_null() {
                cells.push(None);
            } else {
                let bytes = raw.as_bytes().map_err(|_| MirrorError::new("refused", "codec"))?;
                cells.push(Some(bytes.to_vec()));
            }
        }
        page.push(cells);
    }
    Ok(page)
}

fn page_sql(table: &LiveTable, continued: bool) -> Result<String, MirrorError> {
    let schema = quote_ident(&table.shape.schema)?;
    let name = quote_ident(&table.shape.name)?;
    let key = quote_ident(&table.key_name)?;
    let mut list = String::new();
    for (index, column) in table.shape.columns.iter().enumerate() {
        if index > 0 {
            list.push_str(", ");
        }
        list.push_str(&quote_ident(&column.name)?);
    }
    let cast = match table.key_oid {
        20 => "int8",
        21 => "int2",
        23 => "int4",
        _ => return Err(MirrorError::new("refused", "key_type")),
    };
    if continued {
        Ok(format!(
            "SELECT {list} FROM {schema}.{name} WHERE {key} > $1::{cast} ORDER BY {key} LIMIT $2"
        ))
    } else {
        Ok(format!("SELECT {list} FROM {schema}.{name} ORDER BY {key} LIMIT $1"))
    }
}

fn encode_page(
    plan: &Plan,
    digests: &Digests,
    copy_budget: CopyBudgets,
    table: &mut LiveTable,
    page: &[Vec<Option<Vec<u8>>>],
) -> Result<BatchBlob, &'static str> {
    let mut decoded = Vec::with_capacity(page.len());
    for row in page {
        if row.len() != table.plans.len() {
            return Err("codec");
        }
        let mut cells = Vec::with_capacity(table.plans.len());
        for (plan_column, raw) in table.plans.iter().zip(row.iter()) {
            let cell = match raw {
                None => oo_connectors::types::DecodedCell::sql_null(),
                Some(bytes) => codec::decode_cell(plan_column, bytes),
            };
            if cell.presence == oo_connectors::types::CellPresence::Unavailable {
                return Err("codec");
            }
            cells.push(cell);
        }
        decoded.push(cells);
    }
    let binding = RunBinding {
        source_id: plan.config.source_id.clone(),
        schema: table.shape.schema.clone(),
        table: table.shape.name.clone(),
        run_generation: plan.config.generation,
        input_digest: digests.input_bytes,
    };
    let key_attnums = [table.plans[table.key_index].attnum];
    let typed = assemble_batch(&binding, table.plans.clone(), &key_attnums, decoded).map_err(|_| "batch")?;
    if !typed.rejects.is_empty() {
        return Err("codec");
    }
    let mut positions = Vec::with_capacity(typed.rows.len());
    for offset in 0..typed.rows.len() {
        let index = table.next_pos + offset as u64;
        positions.push(ExactDecimal::new(false, index.to_string(), 0).map_err(|_| "position")?);
    }
    let mut columns = Vec::with_capacity(typed.plans.len() + 1);
    columns.push(CopyColumn::SystemPosition);
    for column in &typed.plans {
        if column.mapping.decision == MapDecision::Lossless {
            columns.push(CopyColumn::User { attnum: column.attnum });
        }
    }
    let first = encode_bytes(&typed, &columns, &positions, copy_budget)?;
    let second = encode_bytes(&typed, &columns, &positions, copy_budget)?;
    if first != second {
        return Err("codec");
    }
    let hashed = hash_batch(&typed, &columns, &positions, copy_budget).map_err(|_| "codec")?;
    if hashed.rows != typed.rows.len() as u64 {
        return Err("codec");
    }
    let batch_id = format!("t{}.b{}", table.index, table.batches);
    let applied_pos = positions
        .last()
        .map(ExactDecimal::to_plain_string)
        .unwrap_or_else(|| "0".to_string());
    let quality = spool::quality_bytes(&batch_id, typed.rows.len() as u64, &digests.input_digest);
    let mut mixed = first.clone();
    mixed.extend_from_slice(&quality);
    let mut row_hashes = Vec::with_capacity(typed.rows.len() * 32);
    for row in &typed.rows {
        row_hashes.extend_from_slice(&row.canonical_hash);
    }
    Ok(BatchBlob {
        table_index: table.index,
        rows: typed.rows.len() as u64,
        batch_id,
        payload_digest: sha256_hex(&mixed),
        applied_pos,
        row_hashes,
        copy_bytes: first,
    })
}

fn encode_bytes(
    batch: &oo_connectors::types::TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
    budgets: CopyBudgets,
) -> Result<Vec<u8>, &'static str> {
    let mut encoder = CopyEncoder::open(batch, columns, positions, budgets, UnavailablePolicy::Refuse)
        .map_err(|_| "codec")?;
    let mut out = Vec::new();
    loop {
        match encoder.pull().map_err(|_| "codec")? {
            CopyPiece::Done => break,
            CopyPiece::Header { bytes } | CopyPiece::Trailer { bytes } | CopyPiece::Rows { bytes, .. } => {
                out.extend_from_slice(&bytes);
            }
        }
    }
    Ok(out)
}

enum ApproveFail {
    Columns(Vec<Quarantine>),
    Key(&'static str),
}

fn approve_table(
    shape: &TableShape,
    facts: &TypeGraph,
    reserved: &BTreeSet<String>,
    index: usize,
) -> Result<LiveTable, ApproveFail> {
    let mut quarantine = Vec::new();
    let mut plans = Vec::new();
    for column in &shape.columns {
        if !APPROVED.contains(&column.oid) {
            quarantine.push(Quarantine {
                schema: shape.schema.clone(),
                table: shape.name.clone(),
                column: column.name.clone(),
                oid: column.oid,
                type_name: type_name(facts, column.oid),
                reason: "unsupported_column".to_string(),
            });
            continue;
        }
        let source = source_column(shape, column, facts, reserved);
        let key = shape.primary_key.len() == 1 && shape.primary_key[0] == column.name;
        let plan = map_column(&source, facts, key);
        if plan.mapping.decision != MapDecision::Lossless || plan.decode_oid == 3802 {
            quarantine.push(Quarantine {
                schema: shape.schema.clone(),
                table: shape.name.clone(),
                column: column.name.clone(),
                oid: column.oid,
                type_name: type_name(facts, column.oid),
                reason: "unsupported_column".to_string(),
            });
            continue;
        }
        plans.push(plan);
    }
    if !quarantine.is_empty() {
        return Err(ApproveFail::Columns(quarantine));
    }
    let (key_index, key_oid, key_name) = match shape.primary_key.as_slice() {
        [name] => {
            let Some(index) = shape.columns.iter().position(|column| &column.name == name) else {
                return Err(ApproveFail::Key("no_key"));
            };
            let oid = shape.columns[index].oid;
            if !matches!(oid, 20 | 21 | 23) {
                return Err(ApproveFail::Key("key_type"));
            }
            (index, oid, name.clone())
        }
        [] => return Err(ApproveFail::Key("no_key")),
        _ => return Err(ApproveFail::Key("key_type")),
    };
    Ok(LiveTable {
        index: u32::try_from(index).unwrap_or(u32::MAX),
        shape: shape.clone(),
        plans,
        key_index,
        key_oid,
        key_name,
        next_pos: 0,
        batches: 0,
        rows: 0,
        key_hold: 0,
        started: false,
    })
}

fn type_name(facts: &TypeGraph, oid: i64) -> String {
    facts
        .get(oid)
        .map(|fact| fact.name.clone())
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 63
                && name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == ' ')
        })
        .unwrap_or_else(|| "type".to_string())
}

fn source_column(
    shape: &TableShape,
    column: &crate::plan::ColumnShape,
    facts: &TypeGraph,
    reserved: &BTreeSet<String>,
) -> SourceColumn {
    let fact = facts.get(column.oid);
    let (precision, scale) = numeric_parts(column.typmod).unzip();
    let name_ident = stable_ident(&column.name, reserved).unwrap_or_else(|_| "q_closed".to_string());
    let _ = shape;
    SourceColumn {
        attnum: column.attnum,
        name: column.name.clone(),
        name_ident,
        type_oid: column.oid,
        type_schema: fact.map(|fact| fact.schema.clone()).unwrap_or_else(|| "pg_catalog".to_string()),
        type_name: fact.map(|fact| fact.name.clone()).unwrap_or_else(|| "type".to_string()),
        typtype: fact.map(|fact| fact.typtype.clone()).unwrap_or_else(|| "b".to_string()),
        typmod: column.typmod,
        base_type_oid: fact.and_then(|fact| (fact.typbasetype > 0).then_some(fact.typbasetype)),
        numeric_precision: precision,
        numeric_scale: scale,
        not_null: column.not_null,
        collation_oid: 0,
        collation_name: None,
        identity: String::new(),
        generated: String::new(),
        default_expr: None,
        comment: None,
        enum_labels: Vec::new(),
        domain_checks: Vec::new(),
        type_support: TypeSupport {
            converted: false,
            reason: TypeUnsupportedReason::KnownBuiltinNotConverted,
        },
        current_privileges: ColumnPrivileges {
            select: Some(true),
            insert: None,
            update: None,
        },
    }
}

async fn read_shape(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admitted: &TableShape,
) -> Result<TableShape, MirrorError> {
    let rows = sqlx::query(
        "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod::int4, a.attnotnull \
         FROM pg_catalog.pg_attribute AS a \
         JOIN pg_catalog.pg_class AS c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace AS ns ON ns.oid = c.relnamespace \
         WHERE ns.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
    .bind(&admitted.schema)
    .bind(&admitted.name)
    .fetch_all(&mut **tx)
    .await
    .map_err(pg::map_sqlx)?;
    let mut columns = Vec::new();
    for row in &rows {
        columns.push(crate::plan::ColumnShape {
            attnum: row.try_get(0).map_err(pg::map_sqlx)?,
            name: row.try_get(1).map_err(pg::map_sqlx)?,
            oid: row.try_get(2).map_err(pg::map_sqlx)?,
            typmod: row.try_get(3).map_err(pg::map_sqlx)?,
            not_null: row.try_get(4).map_err(pg::map_sqlx)?,
        });
    }
    let keys = sqlx::query(
        "SELECT a.attname::text \
         FROM pg_catalog.pg_index AS i \
         JOIN pg_catalog.pg_class AS c ON c.oid = i.indrelid \
         JOIN pg_catalog.pg_namespace AS ns ON ns.oid = c.relnamespace \
         CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord) \
         JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid AND a.attnum = k.attnum \
         WHERE ns.nspname = $1 AND c.relname = $2 AND i.indisprimary \
         ORDER BY k.ord",
    )
    .bind(&admitted.schema)
    .bind(&admitted.name)
    .fetch_all(&mut **tx)
    .await
    .map_err(pg::map_sqlx)?;
    let mut primary_key = Vec::new();
    for row in &keys {
        primary_key.push(row.try_get(0).map_err(pg::map_sqlx)?);
    }
    Ok(TableShape {
        schema: admitted.schema.clone(),
        name: admitted.name.clone(),
        columns,
        primary_key,
    })
}

async fn read_facts(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    shape: &TableShape,
) -> Result<TypeGraph, MirrorError> {
    let mut graph = TypeGraph::new(Vec::<TypeFacts>::new());
    for column in &shape.columns {
        if graph.get(column.oid).is_some() {
            continue;
        }
        let row = sqlx::query(
            "SELECT t.oid::int8, t.typname::text, ns.nspname::text, t.typtype::text, t.typlen::int4, \
                    t.typelem::int8, t.typbasetype::int8, t.typtypmod::int4, t.typsend::int8, \
                    p.proname::text, pn.nspname::text \
             FROM pg_catalog.pg_type AS t \
             JOIN pg_catalog.pg_namespace AS ns ON ns.oid = t.typnamespace \
             JOIN pg_catalog.pg_proc AS p ON p.oid = t.typsend \
             JOIN pg_catalog.pg_namespace AS pn ON pn.oid = p.pronamespace \
             WHERE t.oid = $1",
        )
        .bind(column.oid)
        .fetch_optional(&mut **tx)
        .await
        .map_err(pg::map_sqlx)?;
        let Some(row) = row else {
            continue;
        };
        graph.insert(TypeFacts {
            oid: row.try_get(0).map_err(pg::map_sqlx)?,
            name: row.try_get(1).map_err(pg::map_sqlx)?,
            schema: row.try_get(2).map_err(pg::map_sqlx)?,
            typtype: row.try_get(3).map_err(pg::map_sqlx)?,
            typlen: row.try_get(4).map_err(pg::map_sqlx)?,
            typelem: row.try_get(5).map_err(pg::map_sqlx)?,
            typbasetype: row.try_get(6).map_err(pg::map_sqlx)?,
            typtypmod: row.try_get(7).map_err(pg::map_sqlx)?,
            typsend_oid: row.try_get(8).map_err(pg::map_sqlx)?,
            typsend_name: row.try_get(9).map_err(pg::map_sqlx)?,
            typsend_schema: row.try_get(10).map_err(pg::map_sqlx)?,
        });
    }
    Ok(graph)
}

async fn select_allowed(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    schema: &str,
    table: &str,
) -> Result<bool, MirrorError> {
    let relation = format!("{}.{}", quote_ident(schema)?, quote_ident(table)?);
    let row = sqlx::query(
        "SELECT pg_catalog.has_table_privilege(current_user, $1, 'SELECT'), \
                pg_catalog.has_table_privilege(current_user, $1, 'INSERT'), \
                pg_catalog.has_table_privilege(current_user, $1, 'UPDATE'), \
                pg_catalog.has_table_privilege(current_user, $1, 'DELETE'), \
                pg_catalog.has_table_privilege(current_user, $1, 'TRUNCATE')",
    )
    .bind(relation)
    .fetch_one(&mut **tx)
    .await
    .map_err(pg::map_sqlx)?;
    let select: bool = row.try_get(0).map_err(pg::map_sqlx)?;
    let _insert: bool = row.try_get(1).map_err(pg::map_sqlx)?;
    let _update: bool = row.try_get(2).map_err(pg::map_sqlx)?;
    let _delete: bool = row.try_get(3).map_err(pg::map_sqlx)?;
    let _truncate: bool = row.try_get(4).map_err(pg::map_sqlx)?;
    Ok(select)
}

async fn health_row(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Result<Health, sqlx::Error> {
    let row = sqlx::query(
        "SELECT session_user::text, current_user::text, current_database()::text, \
                pg_catalog.pg_is_in_recovery(), pg_catalog.pg_postmaster_start_time()::text, \
                current_setting('transaction_read_only'), pg_catalog.pg_export_snapshot()::text, \
                pg_catalog.pg_backend_pid()::int8, current_setting('transaction_isolation')",
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(Health {
        session_user: row.try_get(0)?,
        current_user: row.try_get(1)?,
        database: row.try_get(2)?,
        in_recovery: row.try_get(3)?,
        postmaster: row.try_get(4)?,
        read_only: row.try_get(5)?,
        snapshot_id: row.try_get(6)?,
        backend_pid: row.try_get(7)?,
        isolation: row.try_get(8)?,
    })
}

async fn sample_peak(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Result<u64, MirrorError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*)::int8 FROM pg_catalog.pg_stat_activity \
         WHERE datname = current_database() AND usename = current_user \
           AND application_name = 'oo-pg-source'",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(pg::map_sqlx)?;
    Ok(u64::try_from(count).unwrap_or(u64::MAX))
}

async fn sample_writes(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(i64, i64, i64), MirrorError> {
    let row = sqlx::query(
        "SELECT tup_inserted::int8, tup_updated::int8, tup_deleted::int8 \
         FROM pg_catalog.pg_stat_database WHERE datname = current_database()",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(pg::map_sqlx)?;
    Ok((
        row.try_get(0).map_err(pg::map_sqlx)?,
        row.try_get(1).map_err(pg::map_sqlx)?,
        row.try_get(2).map_err(pg::map_sqlx)?,
    ))
}

async fn finish_measure(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    health: &Health,
    mut peak: u64,
    writes_at: (i64, i64, i64),
) -> Result<Measured, &'static str> {
    let end = sample_writes(tx).await.map_err(|err| err.fact)?;
    peak = peak.max(sample_peak(tx).await.map_err(|err| err.fact)?);
    if peak > 1 {
        return Err("source_peak");
    }
    let delta = (end.0 - writes_at.0) + (end.1 - writes_at.1) + (end.2 - writes_at.2);
    if delta != 0 {
        return Err("source_write");
    }
    Ok(measured_of(health, peak, Some(delta)))
}

fn measured_of(health: &Health, peak: u64, delta: Option<i64>) -> Measured {
    Measured {
        peak,
        fact: "repeatable_read_read_only",
        snapshot_id: Some(health.snapshot_id.clone()),
        backend_pid: Some(health.backend_pid),
        read_only: true,
        write_tuples: delta,
    }
}

fn failed_from(err: MirrorError, opened: bool) -> Cut {
    let state = if matches!(
        err.fact,
        "statement_timeout" | "lock_timeout" | "connection" | "io" | "connect_timeout"
    ) {
        "incomplete"
    } else {
        "refused"
    };
    failed(state, err.fact, opened, None, None, Vec::new(), "none")
}

fn failed(
    state: &'static str,
    failure: &'static str,
    opened: bool,
    health: Option<Health>,
    peak: Option<u64>,
    quarantine: Vec<Quarantine>,
    spool: &'static str,
) -> Cut {
    let measured = if let Some(health) = health {
        let verified = health.isolation == "repeatable read" && health.read_only == "on";
        Measured {
            peak: peak.unwrap_or(1),
            fact: if verified {
                "repeatable_read_read_only"
            } else {
                "not_verified"
            },
            snapshot_id: Some(health.snapshot_id),
            backend_pid: Some(health.backend_pid),
            read_only: health.read_only == "on",
            write_tuples: None,
        }
    } else if opened {
        Measured {
            peak: peak.unwrap_or(1),
            fact: "repeatable_read_read_only",
            snapshot_id: None,
            backend_pid: None,
            read_only: true,
            write_tuples: None,
        }
    } else {
        Measured {
            peak: 0,
            fact: "not_opened",
            snapshot_id: None,
            backend_pid: None,
            read_only: false,
            write_tuples: None,
        }
    };
    Cut {
        measured,
        kind: CutKind::Failed,
        state,
        failure,
        batches: Vec::new(),
        quarantine,
        spool,
    }
}

fn end_cut(
    measured: Result<Measured, &'static str>,
    kind: CutKind,
    state: &'static str,
    failure: &'static str,
    batches: Vec<BatchBlob>,
    spool: &'static str,
) -> Cut {
    match measured {
        Ok(measured) => Cut {
            measured,
            kind,
            state,
            failure,
            batches,
            quarantine: Vec::new(),
            spool,
        },
        Err(fact) => Cut {
            measured: Measured {
                peak: 1,
                fact: "repeatable_read_read_only",
                snapshot_id: None,
                backend_pid: None,
                read_only: true,
                write_tuples: None,
            },
            kind: CutKind::Failed,
            state: "refused",
            failure: fact,
            batches: Vec::new(),
            quarantine: Vec::new(),
            spool: "none",
        },
    }
}

fn reserved_words() -> BTreeSet<String> {
    PG17_RESERVED.iter().map(|word| (*word).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::{failed, Health};

    fn health(isolation: &str, read_only: &str) -> Health {
        Health {
            session_user: "oo_src_read".to_string(),
            current_user: "oo_src_read".to_string(),
            database: "srcdb".to_string(),
            in_recovery: false,
            postmaster: "2026-10-08 00:00:00+00".to_string(),
            read_only: read_only.to_string(),
            snapshot_id: "0000000A-00000008-1".to_string(),
            backend_pid: 91,
            isolation: isolation.to_string(),
        }
    }

    #[test]
    fn failed_fact_isolation_refusal_does_not_claim_repeatable_read() {
        let cut = failed(
            "refused",
            "isolation",
            true,
            Some(health("read committed", "off")),
            None,
            Vec::new(),
            "none",
        );
        assert_eq!(cut.state, "refused");
        assert_eq!(cut.failure, "isolation");
        assert_eq!(cut.measured.fact, "not_verified");
        assert!(!cut.measured.read_only);
        assert_eq!(cut.measured.snapshot_id.as_deref(), Some("0000000A-00000008-1"));
        assert_eq!(cut.measured.backend_pid, Some(91));
        assert_eq!(cut.measured.peak, 1);
    }

    #[test]
    fn failed_fact_read_only_without_repeatable_isolation_stays_not_verified() {
        let cut = failed(
            "refused",
            "isolation",
            true,
            Some(health("read committed", "on")),
            None,
            Vec::new(),
            "none",
        );
        assert_eq!(cut.measured.fact, "not_verified");
        assert!(cut.measured.read_only);
    }

    #[test]
    fn failed_fact_verified_readback_keeps_repeatable_read_on_a_later_fault() {
        let cut = failed(
            "refused",
            "catalog_drift",
            true,
            Some(health("repeatable read", "on")),
            Some(1),
            Vec::new(),
            "none",
        );
        assert_eq!(cut.failure, "catalog_drift");
        assert_eq!(cut.measured.fact, "repeatable_read_read_only");
        assert!(cut.measured.read_only);
        assert_eq!(cut.measured.peak, 1);
        assert_eq!(cut.measured.snapshot_id.as_deref(), Some("0000000A-00000008-1"));
    }
}
