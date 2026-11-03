//! Reuse the authenticated session, bounded binary portal and Arrow conversion.
use std::io::{self, Write};

use arrow_ipc::writer::StreamWriter;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgIncomingBounds;
use sqlx_core::sql_str::AssertSqlSafe;

use super::{Planned, sqltext};
use crate::engine::*;
use crate::session::{self, cell_i64, cell_string};
use crate::types::{MapDecision, RunBinding, assemble_batch, map_column};
use crate::{PgSession, SourceError};

fn predicate(
    p: &Planned,
    lo: Option<&[KeyValue]>,
    hi: Option<&[KeyValue]>,
) -> Result<(String, Vec<String>), SourceError> {
    let physical = matches!(p.plan.strategy, ChunkStrategy::PhysicalRange { .. });
    let width = if physical { 1 } else { p.keys.len() };
    let mut values = Vec::new();
    for bound in [lo, hi].into_iter().flatten() {
        if width == 0 || bound.len() != width {
            return Err(SourceError::protocol("bound_width"));
        }
        for v in bound {
            if physical {
                super::plan::tid_block(v)?;
            }
            values.push(sqltext::bind_text(v)?);
        }
    }
    let (sql, _) = if physical {
        sqltext::ctid_where(lo.is_some(), hi.is_some())
    } else {
        sqltext::range_where(
            &p.keys,
            sqltext::CompareForm::Row,
            lo.is_some(),
            hi.is_some(),
        )
    };
    Ok((sql, values))
}

struct HashWriter {
    hash: Sha256,
    bytes: u64,
    cap: u64,
}
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| io::Error::other("byte budget"))?;
        if self.bytes > self.cap {
            return Err(io::Error::other("byte budget"));
        }
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) async fn chunk(
    s: &mut PgSession,
    p: &Planned,
    chunk: &ChunkSpec,
    budget: &ReadBudget,
    sink: &mut dyn ChunkSink,
) -> Result<(u64, u64, String), SourceError> {
    let catalog = s.collect_catalog().await?;
    if catalog.schema_hash != p.schema_hash {
        return Err(SourceError::catalog("schema_drift"));
    }
    let table = catalog
        .tables
        .iter()
        .find(|t| t.schema == chunk.table.schema && t.name == chunk.table.name)
        .ok_or_else(|| SourceError::catalog("selected_table_missing"))?;
    let graph = s.load_type_graph(&table.columns).await?;
    let keys: Vec<i32> = table
        .columns
        .iter()
        .filter(|c| p.keys.iter().any(|k| k.name == c.name))
        .map(|c| c.attnum)
        .collect();
    let plans = table
        .columns
        .iter()
        .map(|c| map_column(c, &graph, keys.contains(&c.attnum)))
        .collect::<Vec<_>>();
    // A receipt must not silently omit unsupported columns or rejected values.
    if plans.is_empty()
        || plans
            .iter()
            .any(|p| p.mapping.decision != MapDecision::Lossless)
    {
        return Err(SourceError::protocol("unsupported_column"));
    }
    let mut columns = Vec::new();
    for c in &table.columns {
        columns.push(s.quote_one(&c.name).await?);
    }
    let (predicate, values) = predicate(p, chunk.lo.as_deref(), chunk.hi.as_deref())?;
    let sql = sqltext::chunk_select(&p.relation, &columns, &predicate, &p.keys);

    let cap =
        u32::try_from(budget.max_chunk_bytes).map_err(|_| SourceError::budget("max_bytes"))?;
    s.conn
        .set_incoming_bounds(Some(
            PgIncomingBounds::try_new(
                cap.saturating_add(8192),
                cap,
                cap,
                u16::try_from(columns.len().max(1)).map_err(|_| SourceError::budget("columns"))?,
            )
            .map_err(|_| SourceError::budget("incoming_bounds"))?,
        ))
        .map_err(crate::error::map_sqlx)?;
    let binding = RunBinding {
        source_id: s.source_id.clone(),
        schema: table.schema.clone(),
        table: table.name.clone(),
        run_generation: u64::from(chunk.plan_revision),
        input_digest: Sha256::digest(catalog.schema_hash.as_bytes()).into(),
    };
    let mut declare = sqlx::query(AssertSqlSafe(sqltext::declare(&sql)));
    for value in values {
        declare = declare.bind(value);
    }
    declare
        .execute(&mut s.conn)
        .await
        .map_err(crate::error::map_sqlx)?;
    let mut rows = 0;
    let mut page = Vec::new();
    let mut page_bytes = 0u64;
    let mut writer: Option<StreamWriter<HashWriter>> = None;
    loop {
        let mut count = 0;
        // FETCH text is identical across tables but its result schema is not.
        let mut stream = sqlx::query(AssertSqlSafe(sqltext::fetch(1024)))
            .persistent(false)
            .fetch(&mut s.conn);
        while let Some(row) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            let row = row.map_err(crate::error::map_sqlx)?;
            count += 1;
            rows += 1;
            if rows > budget.max_chunk_rows {
                return Err(SourceError::budget("chunk_rows_replan"));
            }
            let (cells, payload) = crate::bounded_read::decode_row(&plans, &row)?;
            page_bytes = page_bytes.saturating_add(payload);
            if page_bytes > budget.max_chunk_bytes {
                return Err(SourceError::budget("chunk_bytes_replan"));
            }
            page.push(cells);
            if page.len() >= 1024 || page_bytes >= budget.max_chunk_bytes / 2 {
                flush(
                    &binding,
                    &plans,
                    &keys,
                    &mut page,
                    &mut writer,
                    budget,
                    sink,
                )?;
                page_bytes = 0;
            }
        }
        if count < 1024 {
            break;
        }
    }
    if !page.is_empty() || rows == 0 {
        flush(
            &binding,
            &plans,
            &keys,
            &mut page,
            &mut writer,
            budget,
            sink,
        )?;
    }
    sqlx::query(AssertSqlSafe(sqltext::close()))
        .execute(&mut s.conn)
        .await
        .map_err(crate::error::map_sqlx)?;
    s.conn
        .set_incoming_bounds(None)
        .map_err(crate::error::map_sqlx)?;
    let mut writer = writer.ok_or_else(|| SourceError::protocol("arrow_ipc"))?;
    writer
        .finish()
        .map_err(|_| SourceError::budget("chunk_bytes_replan"))?;
    let h = writer
        .into_inner()
        .map_err(|_| SourceError::protocol("arrow_ipc"))?;
    Ok((rows, h.bytes, format!("{:x}", h.hash.finalize())))
}

#[allow(clippy::too_many_arguments)]
fn flush(
    binding: &RunBinding,
    plans: &[crate::types::ColumnPlan],
    keys: &[i32],
    page: &mut Vec<Vec<crate::types::DecodedCell>>,
    writer: &mut Option<StreamWriter<HashWriter>>,
    budget: &ReadBudget,
    sink: &mut dyn ChunkSink,
) -> Result<(), SourceError> {
    let typed = assemble_batch(binding, plans.to_vec(), keys, std::mem::take(page))?;
    if !typed.rejects.is_empty() {
        return Err(SourceError::protocol("rejected_cell"));
    }
    let batch = typed.record_batch()?;
    if writer.is_none() {
        *writer = Some(
            StreamWriter::try_new(
                HashWriter {
                    hash: Sha256::new(),
                    bytes: 0,
                    cap: budget.max_chunk_bytes,
                },
                &batch.schema(),
            )
            .map_err(|_| SourceError::budget("chunk_bytes_replan"))?,
        );
    }
    writer
        .as_mut()
        .unwrap()
        .write(&batch)
        .map_err(|_| SourceError::budget("chunk_bytes_replan"))?;
    sink.push(batch)
}

pub(super) async fn digest(
    s: &mut PgSession,
    p: &Planned,
    lo: Option<&[KeyValue]>,
    hi: Option<&[KeyValue]>,
) -> Result<RangeDigest, SourceError> {
    let (predicate, values) = predicate(p, lo, hi)?;
    let catalog = s.collect_catalog().await?;
    if catalog.schema_hash != p.schema_hash {
        return Err(SourceError::catalog("schema_drift"));
    }
    let t = catalog
        .tables
        .iter()
        .find(|t| t.schema == p.plan.table.schema && t.name == p.plan.table.name)
        .ok_or_else(|| SourceError::catalog("selected_table_missing"))?;
    let mut cols = Vec::new();
    for c in &t.columns {
        cols.push(s.quote_one(&c.name).await?);
    }
    let sql = sqltext::digest_select(&p.relation, &cols, &predicate);
    let mut q = sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql.clone()));
    for v in values {
        q = q.bind(v);
    }
    let r = session::fetch_one(s, q).await?;
    Ok(RangeDigest {
        table: p.plan.table.clone(),
        lo: lo.map(<[_]>::to_vec),
        hi: hi.map(<[_]>::to_vec),
        rows: cell_i64(&r, 0)? as u64,
        digest: format!(
            "pg-text-sha256-sum128/v1:{}:{}",
            cell_string(&r, 1)?,
            cell_string(&r, 2)?
        ),
    })
}
