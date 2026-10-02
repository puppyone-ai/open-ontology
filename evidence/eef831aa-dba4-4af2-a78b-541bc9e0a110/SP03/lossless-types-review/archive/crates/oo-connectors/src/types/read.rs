//! One bounded binary read. Identifiers come from `quote_ident`. Values do not
//! enter the SQL text. The transaction is repeatable-read, read-only, and
//! rolled back.

use sqlx::postgres::PgValueFormat;
use sqlx::{Row, ValueRef};

use super::batch::{assemble_batch, DecodedCell, TypedBatch};
use super::binary::decode_payload;
use super::facts::{TypeFacts, TypeGraph};
use super::mapping::{map_column, ColumnPlan, MapDecision};
use super::{PgTypePolicy, RunBinding, UnsupportedColumnPolicy};
use crate::error::SourceError;
use crate::model::SourceColumn;
use crate::session::{self, PgSession};
use crate::sql;

/// `quote_ident` output is accepted only as a single identifier token.
pub fn valid_quoted_ident(raw: &str) -> bool {
    if raw.is_empty()
        || raw.len() > 256
        || raw.contains('\0')
        || raw.contains(';')
        || raw.contains("--")
        || raw.contains("/*")
        || raw.contains("*/")
    {
        return false;
    }
    if raw.as_bytes().first() == Some(&b'"') {
        return valid_quoted(raw);
    }
    valid_bare(raw)
}

fn valid_bare(raw: &str) -> bool {
    if raw.len() > 63 {
        return false;
    }
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '_' && !first.is_alphabetic() {
        return false;
    }
    chars.all(|ch| ch == '_' || ch == '$' || ch.is_alphanumeric())
}

fn valid_quoted(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || *bytes.last().unwrap_or(&0) != b'"' {
        return false;
    }
    let inner = &raw[1..raw.len() - 1];
    let mut chars = inner.chars();
    let mut body = 0usize;
    while let Some(ch) = chars.next() {
        if ch == '"' && chars.next() != Some('"') {
            return false;
        }
        body += 1;
    }
    body > 0
}

/// `SELECT` of quoted columns, or a refusal. An empty list is not a presence scan.
pub fn compose_select(schema: &str, table: &str, columns: &[String]) -> Result<String, SourceError> {
    if columns.is_empty() || !valid_quoted_ident(schema) || !valid_quoted_ident(table) {
        return Err(SourceError::catalog("quote_ident"));
    }
    for column in columns {
        if !valid_quoted_ident(column) {
            return Err(SourceError::catalog("quote_ident"));
        }
    }
    let mut sql = String::from("SELECT ");
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            sql.push(',');
        }
        sql.push_str(column);
    }
    sql.push_str(" FROM ");
    sql.push_str(schema);
    sql.push('.');
    sql.push_str(table);
    sql.push_str(" LIMIT $1");
    Ok(sql)
}

/// Row count when every column is omitted. The constant is not part of the frame.
pub fn compose_presence(schema: &str, table: &str) -> Result<String, SourceError> {
    if !valid_quoted_ident(schema) || !valid_quoted_ident(table) {
        return Err(SourceError::catalog("quote_ident"));
    }
    Ok(format!("SELECT 1 FROM {schema}.{table} LIMIT $1"))
}

pub(crate) fn guard_plans(plans: &[ColumnPlan], policy: PgTypePolicy) -> Result<(), SourceError> {
    for plan in plans {
        let MapDecision::Unsupported(reason) = plan.mapping.decision else {
            continue;
        };
        if plan.key {
            return Err(SourceError::protocol(if reason == "select_not_granted" {
                "select_not_granted"
            } else {
                "unsupported_key"
            }));
        }
        if policy.unsupported == UnsupportedColumnPolicy::FailRead {
            return Err(SourceError::protocol("fail_read"));
        }
    }
    Ok(())
}

pub(crate) fn require_binary(format: PgValueFormat) -> Result<(), SourceError> {
    if format == PgValueFormat::Binary {
        Ok(())
    } else {
        Err(SourceError::protocol("text_format_refused"))
    }
}

impl PgSession {
    /// Read one selected table under the conversion protocol.
    ///
    /// Catalog `type_support.converted` is ignored. The snapshot is rolled back.
    pub async fn read_typed(
        &mut self,
        binding: &RunBinding,
        policy: PgTypePolicy,
    ) -> Result<TypedBatch, SourceError> {
        self.ensure_open()?;
        if binding.source_id != self.source_id {
            return Err(SourceError::config("binding_source"));
        }
        if !self.selection.tables().iter().any(|table| {
            table.schema() == binding.schema && table.table() == binding.table
        }) {
            return Err(SourceError::config("binding_table"));
        }
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
        let result = self.read_in_snapshot(binding, policy).await;
        self.finish(result).await
    }

    async fn read_in_snapshot(
        &mut self,
        binding: &RunBinding,
        policy: PgTypePolicy,
    ) -> Result<TypedBatch, SourceError> {
        let catalog = self.collect_catalog().await?;
        let Some(table) = catalog
            .tables
            .into_iter()
            .find(|table| table.schema == binding.schema && table.name == binding.table)
        else {
            return Err(SourceError::catalog("selected_table_missing"));
        };
        let key_attnums: Vec<i32> = table.primary_key.into_iter().map(|key| key.attnum).collect();
        let graph = self.load_type_graph(&table.columns).await?;
        let mut plans = Vec::with_capacity(table.columns.len());
        for column in &table.columns {
            let key = key_attnums.contains(&column.attnum);
            plans.push(map_column(column, &graph, key));
        }
        guard_plans(&plans, policy)?;
        let schema_sql = self.quote_one(&binding.schema).await?;
        let table_sql = self.quote_one(&binding.table).await?;
        let mut column_sql = Vec::new();
        let mut readable = Vec::new();
        for plan in &plans {
            if plan.mapping.decision != MapDecision::Lossless {
                continue;
            }
            column_sql.push(self.quote_one(&plan.name).await?);
            readable.push(plan.attnum);
        }
        let sql = if column_sql.is_empty() {
            compose_presence(&schema_sql, &table_sql)?
        } else {
            compose_select(&schema_sql, &table_sql, &column_sql)?
        };
        let limit = i64::from(self.budget.max_rows()) + 1;
        let fetched = fetch_limit(self, &sql, limit).await?;
        let mut used = u64::try_from(sql.len()).unwrap_or(u64::MAX);
        let max_bytes = self.budget.max_bytes();
        if used > max_bytes {
            return Err(SourceError::budget("max_bytes"));
        }
        let mut decoded = Vec::with_capacity(fetched.len());
        for row in &fetched {
            if readable.is_empty() {
                let raw = row
                    .try_get_raw(0)
                    .map_err(|_| SourceError::protocol("binary_payload"))?;
                require_binary(raw.format())?;
                decoded.push(Vec::new());
                continue;
            }
            if row.columns().len() != readable.len() {
                return Err(SourceError::protocol("row_width"));
            }
            let mut cells = Vec::with_capacity(readable.len());
            for (index, plan) in plans
                .iter()
                .filter(|plan| plan.mapping.decision == MapDecision::Lossless)
                .enumerate()
            {
                let raw = row
                    .try_get_raw(index)
                    .map_err(|_| SourceError::protocol("binary_payload"))?;
                require_binary(raw.format())?;
                let actual = raw
                    .type_info()
                    .oid()
                    .map(|oid| i64::from(oid.0))
                    .ok_or_else(|| SourceError::protocol("type_oid_mismatch"))?;
                if !plan.accepts_result_oid(actual) {
                    return Err(SourceError::protocol("type_oid_mismatch"));
                }
                if raw.is_null() {
                    cells.push(DecodedCell::sql_null());
                    continue;
                }
                let bytes = raw
                    .as_bytes()
                    .map_err(|_| SourceError::protocol("binary_payload"))?;
                charge_bytes(&mut used, max_bytes, bytes.len() as u64)?;
                cells.push(decode_payload(plan, bytes));
            }
            decoded.push(cells);
        }
        assemble_batch(binding, plans, &key_attnums, decoded)
    }

    async fn load_type_graph(&mut self, columns: &[SourceColumn]) -> Result<TypeGraph, SourceError> {
        let mut graph = TypeGraph::default();
        let mut pending: Vec<i64> = columns.iter().map(|column| column.type_oid).collect();
        for depth in 0..=8 {
            pending.sort_unstable();
            pending.dedup();
            pending.retain(|oid| *oid != 0 && graph.get(*oid).is_none());
            if pending.is_empty() {
                break;
            }
            let rows = session::fetch_all(self, sqlx::query(sql::TYPE_FACTS).bind(&pending)).await?;
            if rows.len() != pending.len() {
                return Err(SourceError::catalog("type_facts_missing"));
            }
            let mut next = Vec::new();
            for row in &rows {
                let facts = type_facts(row)?;
                if depth < 8 {
                    if facts.typelem != 0 {
                        next.push(facts.typelem);
                    }
                    if facts.typbasetype != 0 {
                        next.push(facts.typbasetype);
                    }
                }
                graph.insert(facts);
            }
            pending = next;
        }
        let enum_oids: Vec<i64> = graph
            .iter()
            .filter(|facts| facts.typtype == "e")
            .map(|facts| facts.oid)
            .collect();
        if !enum_oids.is_empty() {
            let rows =
                session::fetch_all(self, sqlx::query(sql::ENUM_LABELS).bind(&enum_oids)).await?;
            let mut grouped: Vec<(i64, Vec<String>)> = Vec::new();
            for row in &rows {
                let oid = session::cell_i64(row, 0)?;
                let label = session::cell_string(row, 1)?;
                if let Some((_, labels)) = grouped.iter_mut().find(|(item, _)| *item == oid) {
                    labels.push(label);
                } else {
                    grouped.push((oid, vec![label]));
                }
            }
            for (oid, labels) in grouped {
                graph.set_enum_labels(oid, labels);
            }
        }
        Ok(graph)
    }

    async fn quote_one(&mut self, name: &str) -> Result<String, SourceError> {
        let row = session::fetch_one(self, sqlx::query(sql::QUOTE_IDENT).bind(name)).await?;
        let quoted = session::cell_string(&row, 0)?;
        if !valid_quoted_ident(&quoted) {
            return Err(SourceError::catalog("quote_ident"));
        }
        Ok(quoted)
    }
}

fn type_facts(row: &sqlx::postgres::PgRow) -> Result<TypeFacts, SourceError> {
    Ok(TypeFacts {
        oid: session::cell_i64(row, 0)?,
        name: session::cell_string(row, 1)?,
        schema: session::cell_string(row, 2)?,
        typtype: session::cell_string(row, 3)?,
        typlen: session::cell_i32(row, 4)?,
        typelem: session::cell_i64(row, 5)?,
        typbasetype: session::cell_i64(row, 6)?,
        typtypmod: session::cell_i32(row, 7)?,
        typsend_oid: session::cell_i64(row, 8)?,
        typsend_name: session::cell_string(row, 9)?,
        typsend_schema: session::cell_string(row, 10)?,
    })
}

async fn fetch_limit(
    session: &mut PgSession,
    sql: &str,
    limit: i64,
) -> Result<Vec<sqlx::postgres::PgRow>, SourceError> {
    let budget = session.budget.statement_timeout();
    // `sql` is the fixed template plus identifiers already checked by
    // `valid_quoted_ident`. The limit is a bound parameter, not text.
    let query = sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql.to_string())).bind(limit);
    let result = tokio::time::timeout(budget, query.fetch_all(&mut session.conn)).await;
    match result {
        Ok(Ok(rows)) => {
            let max = usize::try_from(session.budget.max_rows()).unwrap_or(usize::MAX);
            if rows.len() > max {
                Err(SourceError::budget("max_rows"))
            } else {
                Ok(rows)
            }
        }
        Ok(Err(err)) => Err(crate::error::map_sqlx(err)),
        Err(_) => {
            session.poisoned = true;
            Err(SourceError::budget("statement_timeout"))
        }
    }
}

fn charge_bytes(used: &mut u64, max: u64, extra: u64) -> Result<(), SourceError> {
    let next = used
        .checked_add(extra)
        .ok_or_else(|| SourceError::budget("max_bytes"))?;
    if next > max {
        return Err(SourceError::budget("max_bytes"));
    }
    *used = next;
    Ok(())
}
