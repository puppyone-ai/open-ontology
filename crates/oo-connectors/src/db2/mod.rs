//! Opt-in Db2 LUW reads through a source-only IBM CLI worker.
//! C5 is reconciliation, not log CDC. Timestamps are observation fences,
//! never a cross-table snapshot or a recoverable log position.
#[cfg(test)]
#[path = "../../../../testing/engines/db2/acceptance.rs"]
mod acceptance;
mod worker;

use crate::{SourceError, SourceErrorKind, engine::*};
use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use worker::{DriverError, Worker};
use zeroize::Zeroizing;

pub struct ConnectConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Zeroizing<String>,
    /// Path inside the trusted worker, not inside the host.
    pub tls_ca: Option<String>,
    pub scope: Vec<TableRef>,
}

impl ConnectConfig {
    pub fn new(
        host: &str,
        port: u16,
        database: &str,
        user: &str,
        password: Zeroizing<String>,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            database: database.into(),
            user: user.into(),
            password,
            tls_ca: None,
            scope: Vec::new(),
        }
    }
}

enum Bind {
    Number(String),
    Text(String),
}

#[derive(Clone, Debug)]
struct Column {
    name: String,
    kind: String,
    nullable: bool,
}

#[derive(Clone)]
struct Plan {
    public: TableChunkPlan,
    columns: Vec<Column>,
    keys: Vec<usize>,
    budget: ReadBudget,
}

#[derive(Default, serde::Serialize)]
pub struct Stats {
    pub sessions: u32,
    pub queries: u64,
    pub rows: u64,
    pub read_micros: u64,
    pub sleep_micros: u64,
}

pub struct Db2Engine {
    worker: Worker,
    plans: BTreeMap<TableRef, Plan>,
    revision: u32,
    stats: Stats,
    budget: ReadBudget,
    scope: Vec<TableRef>,
    user: String,
}

fn error(fact: &'static str) -> SourceError {
    SourceError::config(fact)
}
fn driver(error: DriverError) -> SourceError {
    match error {
        DriverError::Source(e) => e,
        DriverError::Worker { timeout: true, .. } => SourceError::budget("db2_statement_timeout"),
        DriverError::Worker { code, .. } => {
            let _ = code;
            SourceError::new(SourceErrorKind::Protocol, Some("db2_worker_failure"), None)
        }
    }
}

fn ident(value: &str) -> Result<String, SourceError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(error("db2_identifier"));
    }
    Ok(format!("\"{}\"", value.replace('"', "\"\"")))
}
fn relation(t: &TableRef) -> Result<String, SourceError> {
    Ok(format!("{}.{}", ident(&t.schema)?, ident(&t.name)?))
}
fn validate_budget(b: &ReadBudget) -> Result<(), SourceError> {
    if b.max_chunk_rows == 0
        || b.max_chunk_rows > 100_000
        || b.max_chunk_bytes == 0
        || b.max_chunk_bytes > 32 * 1024 * 1024
        || !(1..=100).contains(&b.duty_cycle_percent)
        || b.statement_timeout_ms == 0
        || b.statement_timeout_ms > 300_000
        || b.target_chunk_ms == 0
    {
        return Err(error("db2_budget"));
    }
    Ok(())
}

// Expanded lexicographic predicates keep every comparison in Db2's collation.
fn bound(
    columns: &[String],
    values: &[KeyValue],
    lower: bool,
    strict: bool,
    binds: &mut Vec<Bind>,
) -> Result<String, SourceError> {
    if columns.is_empty() || columns.len() != values.len() {
        return Err(error("db2_bound_shape"));
    }
    let mut terms = Vec::new();
    for i in 0..columns.len() {
        let mut term = Vec::new();
        for j in 0..=i {
            let op = if j < i {
                "="
            } else if lower {
                if !strict && i + 1 == columns.len() {
                    ">="
                } else {
                    ">"
                }
            } else {
                "<"
            };
            let bind = match &values[j] {
                KeyValue::Int(v) => Bind::Number(v.to_string()),
                KeyValue::Text(v) => Bind::Text(v.clone()),
                _ => return Err(error("db2_key_type")),
            };
            binds.push(bind);
            term.push(format!("{} {op} ?", ident(&columns[j])?));
        }
        terms.push(format!("({})", term.join(" AND ")));
    }
    Ok(format!("({})", terms.join(" OR ")))
}

impl Db2Engine {
    /// The command must identify a trusted, pinned source-only worker.
    pub fn connect(command: &mut Command, config: &ConnectConfig) -> Result<Self, SourceError> {
        ident(&config.user)?;
        for table in &config.scope {
            relation(table)?;
        }
        if config.tls_ca.is_none() && !matches!(config.host.as_str(), "127.0.0.1" | "::1") {
            return Err(error("db2_tls_required"));
        }
        Ok(Self {
            worker: Worker::connect(command, config).map_err(driver)?,
            plans: BTreeMap::new(),
            revision: 0,
            stats: Stats {
                sessions: 1,
                ..Stats::default()
            },
            budget: ReadBudget::default(),
            scope: config.scope.clone(),
            user: config.user.clone(),
        })
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Bind C5 to the catalog incarnation and complete column/key definitions.
    /// This is a DDL fence, not a recoverable database log position.
    pub async fn table_lineage(&mut self, table: &TableRef) -> Result<String, SourceError> {
        relation(table)?;
        let budget = self.budget.clone();
        let binds = [
            Bind::Text(table.schema.clone()),
            Bind::Text(table.name.clone()),
        ];
        let incarnation = self.query(
            "SELECT VARCHAR(CREATE_TIME),VARCHAR(TABLEID),VARCHAR(TBSPACEID) FROM SYSCAT.TABLES WHERE TABSCHEMA=? AND TABNAME=?",
            &binds, 1, &budget,
        ).await?;
        if incarnation.len() != 1 {
            return Err(error("db2_table_not_visible"));
        }
        let columns = self.query(
            "SELECT COLNAME,TYPENAME,VARCHAR(LENGTH),VARCHAR(SCALE),NULLS,VARCHAR(CODEPAGE),TYPESCHEMA FROM SYSCAT.COLUMNS WHERE TABSCHEMA=? AND TABNAME=? ORDER BY COLNO",
            &binds, 1024, &budget,
        ).await?;
        let keys = self.query(
            "SELECT K.COLNAME FROM SYSCAT.KEYCOLUSE K JOIN SYSCAT.TABCONST C ON K.TABSCHEMA=C.TABSCHEMA AND K.TABNAME=C.TABNAME AND K.CONSTNAME=C.CONSTNAME WHERE C.TYPE='P' AND K.TABSCHEMA=? AND K.TABNAME=? ORDER BY K.COLSEQ",
            &binds, 64, &budget,
        ).await?;
        let bytes = serde_json::to_vec(&(table, incarnation, columns, keys))
            .map_err(|_| error("db2_lineage"))?;
        Ok(format!("db2-catalog:{:x}", Sha256::digest(bytes)))
    }

    /// Keep the baseline partition when source growth moves live boundaries.
    pub fn adopt_keyset_plan(&mut self, durable: &TableChunkPlan) -> Result<(), SourceError> {
        let live = self
            .plans
            .get_mut(&durable.table)
            .ok_or_else(|| error("db2_plan_required"))?;
        validate_durable_plan(&live.public, durable)?;
        live.public.chunks = durable.chunks.clone();
        Ok(())
    }

    async fn query(
        &mut self,
        sql: &str,
        binds: &[Bind],
        limit: u64,
        budget: &ReadBudget,
    ) -> Result<Vec<Vec<Option<String>>>, SourceError> {
        validate_budget(budget)?;
        self.worker
            .set_timeout(Duration::from_millis(budget.statement_timeout_ms));
        let started = Instant::now();
        let result = (|| {
            let mut rows = Vec::new();
            let mut bytes = 0u64;
            let mut over_budget = false;
            let mut received = 0u64;
            for row in self
                .worker
                .query_bounded(sql, binds, (limit + 1) as usize, budget.max_chunk_bytes)
                .map_err(driver)?
            {
                if started.elapsed() > Duration::from_millis(budget.statement_timeout_ms) {
                    return Err(SourceError::budget("db2_statement_timeout"));
                }
                let row = row.map_err(driver)?;
                received += 1;
                if received > limit + 1 {
                    return Err(error("db2_worker_row_limit"));
                }
                bytes += row
                    .iter()
                    .map(|v| v.as_ref().map_or(1, |s| s.len() as u64 + 1))
                    .sum::<u64>();
                if rows.len() as u64 >= limit || bytes > budget.max_chunk_bytes {
                    // Consume the bounded overflow sentinel and completion so
                    // the shared loader can replan on this same source session.
                    over_budget = true;
                } else {
                    rows.push(row);
                }
            }
            if over_budget {
                Err(SourceError::budget("db2_chunk_over_budget"))
            } else {
                Ok(rows)
            }
        })();
        let elapsed = started.elapsed();
        let result = if elapsed > Duration::from_millis(budget.statement_timeout_ms) {
            Err(SourceError::budget("db2_statement_timeout"))
        } else {
            result
        };
        let sleep = elapsed * (100 - u32::from(budget.duty_cycle_percent))
            / u32::from(budget.duty_cycle_percent);
        self.stats.queries += 1;
        self.stats.read_micros += elapsed.as_micros() as u64;
        tokio::time::sleep(sleep).await;
        self.stats.sleep_micros += sleep.as_micros() as u64;
        result
    }

    async fn columns(
        &mut self,
        t: &TableRef,
        b: &ReadBudget,
    ) -> Result<(Vec<Column>, Vec<usize>), SourceError> {
        relation(t)?;
        let binds = || vec![Bind::Text(t.schema.clone()), Bind::Text(t.name.clone())];
        let rows = self.query("SELECT COLNAME,TYPENAME,NULLS,CODEPAGE,TYPESCHEMA FROM SYSCAT.COLUMNS WHERE TABSCHEMA=? AND TABNAME=? ORDER BY COLNO", &binds(), 1024, b).await?;
        let mut columns = Vec::new();
        for row in rows {
            let name = cell(&row, 0)?.trim_end().to_owned();
            let kind = cell(&row, 1)?.trim_end().to_owned();
            if cell(&row, 4)?.trim_end() != "SYSIBM"
                || (matches!(kind.as_str(), "CHAR" | "VARCHAR") && cell(&row, 3)?.trim() == "0")
            {
                return Err(error("db2_type_not_admitted"));
            }
            if !matches!(
                kind.as_str(),
                "SMALLINT"
                    | "INTEGER"
                    | "BIGINT"
                    | "VARCHAR"
                    | "CHAR"
                    | "DATE"
                    | "TIME"
                    | "TIMESTAMP"
                    | "DECIMAL"
            ) {
                return Err(error("db2_type_not_admitted"));
            }
            columns.push(Column {
                name,
                kind,
                nullable: cell(&row, 2)? == "Y",
            });
        }
        if columns.is_empty() {
            return Err(error("db2_table_not_visible"));
        }
        let rows = self.query("SELECT K.COLNAME FROM SYSCAT.KEYCOLUSE K JOIN SYSCAT.TABCONST C ON K.TABSCHEMA=C.TABSCHEMA AND K.TABNAME=C.TABNAME AND K.CONSTNAME=C.CONSTNAME WHERE C.TYPE='P' AND K.TABSCHEMA=? AND K.TABNAME=? ORDER BY K.COLSEQ", &binds(), 64, b).await?;
        let mut keys = Vec::new();
        for row in rows {
            let name = cell(&row, 0)?.trim_end();
            let index = columns
                .iter()
                .position(|c| c.name == name)
                .ok_or_else(|| error("db2_key_column"))?;
            if !matches!(
                columns[index].kind.as_str(),
                "SMALLINT" | "INTEGER" | "BIGINT" | "VARCHAR" | "CHAR"
            ) {
                return Err(error("db2_key_type"));
            }
            keys.push(index);
        }
        Ok((columns, keys))
    }

    async fn rows(
        &mut self,
        plan: &Plan,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
        b: &ReadBudget,
    ) -> Result<Vec<Vec<Option<String>>>, SourceError> {
        let columns = plan
            .columns
            .iter()
            .map(|c| {
                let name = ident(&c.name)?;
                // Text projection avoids driver-side decimal and timestamp truncation.
                Ok(if matches!(c.kind.as_str(), "CHAR" | "VARCHAR") {
                    name
                } else {
                    format!("VARCHAR({name})")
                })
            })
            .collect::<Result<Vec<_>, SourceError>>()?
            .join(",");
        let keys = key_names(plan);
        let mut binds = Vec::new();
        let mut predicates = Vec::new();
        if let Some(lo) = lo {
            predicates.push(bound(&keys, lo, true, false, &mut binds)?);
        }
        if let Some(hi) = hi {
            predicates.push(bound(&keys, hi, false, true, &mut binds)?);
        }
        let where_sql = if predicates.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", predicates.join(" AND "))
        };
        let order = if keys.is_empty() {
            String::new()
        } else {
            format!(
                " ORDER BY {}",
                keys.iter()
                    .map(|k| ident(k))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",")
            )
        };
        self.query(
            &format!(
                "SELECT {columns} FROM {}{where_sql}{order} FETCH FIRST {} ROWS ONLY WITH CS",
                relation(&plan.public.table)?,
                b.max_chunk_rows + 1
            ),
            &binds,
            b.max_chunk_rows,
            b,
        )
        .await
    }
}

fn cell(row: &[Option<String>], i: usize) -> Result<&str, SourceError> {
    row.get(i)
        .and_then(Option::as_deref)
        .ok_or_else(|| error("db2_metadata"))
}
fn validate_durable_plan(
    live: &TableChunkPlan,
    durable: &TableChunkPlan,
) -> Result<(), SourceError> {
    if live.protocol != durable.protocol
        || live.table != durable.table
        || live.key != durable.key
        || live.strategy != durable.strategy
        || durable.chunks.is_empty()
        || durable.chunks.first().is_none_or(|c| c.lo.is_some())
        || durable.chunks.last().is_none_or(|c| c.hi.is_some())
        || durable.chunks.iter().enumerate().any(|(i, c)| {
            c.table != durable.table
                || c.chunk_id as usize != i
                || (i > 0 && (c.lo.is_none() || durable.chunks[i - 1].hi != c.lo))
        })
        || (matches!(live.strategy, ChunkStrategy::SingleRead) && durable.chunks.len() != 1)
    {
        return Err(error("db2_stale_plan"));
    }
    let names = match &durable.strategy {
        ChunkStrategy::Keyset { columns } => columns.as_slice(),
        ChunkStrategy::SingleRead => &[],
        _ => return Err(error("db2_stale_plan")),
    };
    for chunk in &durable.chunks {
        for values in [chunk.lo.as_deref(), chunk.hi.as_deref()]
            .into_iter()
            .flatten()
        {
            bound(names, values, true, false, &mut Vec::new())?;
        }
    }
    Ok(())
}
fn key_names(p: &Plan) -> Vec<String> {
    p.keys.iter().map(|&i| p.columns[i].name.clone()).collect()
}
fn key_values(p: &Plan, row: &[Option<String>]) -> Result<Vec<KeyValue>, SourceError> {
    p.keys
        .iter()
        .map(|&i| {
            let text = cell(row, i)?;
            Ok(
                if matches!(
                    p.columns[i].kind.as_str(),
                    "SMALLINT" | "INTEGER" | "BIGINT"
                ) {
                    KeyValue::Int(text.trim().parse().map_err(|_| error("db2_integer"))?)
                } else {
                    KeyValue::Text(text.into())
                },
            )
        })
        .collect()
}

fn batch(plan: &Plan, rows: &[Vec<Option<String>>]) -> Result<RecordBatch, SourceError> {
    if rows.iter().any(|r| r.len() != plan.columns.len()) {
        return Err(error("db2_row_shape"));
    }
    let mut fields = Vec::new();
    let mut arrays: Vec<ArrayRef> = Vec::new();
    for (i, c) in plan.columns.iter().enumerate() {
        if !c.nullable && rows.iter().any(|row| row[i].is_none()) {
            return Err(error("db2_null_in_required_column"));
        }
        let integer = matches!(c.kind.as_str(), "SMALLINT" | "INTEGER" | "BIGINT");
        // Exact decimal and sub-microsecond temporal values remain lossless text.
        fields.push(Field::new(
            &c.name,
            if integer {
                DataType::Int64
            } else {
                DataType::Utf8
            },
            c.nullable,
        ));
        if integer {
            let values = rows
                .iter()
                .map(|r| {
                    r[i].as_ref()
                        .map(|v| v.trim().parse::<i64>().map_err(|_| error("db2_integer")))
                        .transpose()
                })
                .collect::<Result<Vec<_>, _>>()?;
            arrays.push(Arc::new(Int64Array::from(values)));
        } else {
            arrays.push(Arc::new(StringArray::from(
                rows.iter().map(|r| r[i].as_deref()).collect::<Vec<_>>(),
            )));
        }
    }
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).map_err(|_| error("db2_arrow"))
}

impl SourceEngine for Db2Engine {
    fn family(&self) -> EngineFamily {
        EngineFamily::Db2
    }

    async fn probe(&mut self) -> Result<ProbeReport, SourceError> {
        let rows = self
            .query(
                "SELECT SERVICE_LEVEL,CURRENT USER FROM TABLE(SYSPROC.ENV_GET_INST_INFO()) AS T",
                &[],
                1,
                &ReadBudget::default(),
            )
            .await?;
        let raw = cell(rows.first().ok_or_else(|| error("db2_version"))?, 0)?;
        self.user = cell(rows.first().ok_or_else(|| error("db2_user"))?, 1)?
            .trim_end()
            .into();
        let version =
            EngineVersion::parse(EngineFamily::Db2, raw.strip_prefix("DB2 v").unwrap_or(raw))?;
        if version.major != 11 || version.minor != 5 {
            return Err(error("db2_version_not_admitted"));
        }
        let mut capabilities: BTreeMap<_, _> = [
            Capability::Catalog,
            Capability::ChunkedSnapshot,
            Capability::DigestDiff,
            Capability::Watermark,
        ]
        .into_iter()
        .map(|c| (c, CapabilityState::Available))
        .chain(
            [
                Capability::LogCdc,
                Capability::NativeChanges,
                Capability::PartitionRefresh,
                Capability::ReplicaRead,
            ]
            .into_iter()
            .map(|c| {
                (
                    c,
                    CapabilityState::Unsupported {
                        reason: "not implemented; no ASN writes".into(),
                    },
                )
            }),
        )
        .collect();
        let mut requirements = Vec::new();
        for table in self.scope.clone() {
            let sql = format!("SELECT 1 FROM {} WHERE 1=0", relation(&table)?);
            if self
                .query(&sql, &[], 1, &ReadBudget::default())
                .await
                .is_err()
            {
                requirements.push(Requirement {
                    kind: RequirementKind::Privilege,
                    name: format!("select:{table}"),
                    observed: Some(
                        "read probe failed; verify object existence and SELECT grant".into(),
                    ),
                    required: "SELECT".into(),
                    dba_statement: Some(format!(
                        "GRANT SELECT ON {} TO USER {}",
                        relation(&table)?,
                        ident(&self.user)?
                    )),
                });
            }
        }
        if !requirements.is_empty() {
            for capability in [Capability::ChunkedSnapshot, Capability::DigestDiff] {
                capabilities.insert(
                    capability,
                    CapabilityState::NeedsSetup {
                        requirements: requirements.clone(),
                    },
                );
            }
        }
        Ok(ProbeReport {
            protocol: ENGINE_PROTOCOL.into(),
            family: self.family(),
            version,
            capabilities,
            settings: BTreeMap::from([
                (
                    "consistency".into(),
                    "B; cursor stability; no global snapshot".into(),
                ),
                (
                    "watermark".into(),
                    "observation timestamp, not log position".into(),
                ),
                ("decimal_temporal_mapping".into(), "lossless text".into()),
                (
                    "driver".into(),
                    "customer-provided IBM CLI in isolated worker".into(),
                ),
            ]),
        })
    }

    async fn discover_tables(&mut self, b: &ReadBudget) -> Result<Vec<TableRef>, SourceError> {
        self.query("SELECT TABSCHEMA,TABNAME FROM SYSCAT.TABLES WHERE TYPE='T' AND TABSCHEMA NOT LIKE 'SYS%' ORDER BY TABSCHEMA,TABNAME", &[], 10_000, b).await?.into_iter()
            .map(|r| Ok(TableRef { schema: cell(&r,0)?.trim_end().into(), name: cell(&r,1)?.trim_end().into() })).collect()
    }

    async fn plan_chunks(
        &mut self,
        table: &TableRef,
        b: &ReadBudget,
    ) -> Result<TableChunkPlan, SourceError> {
        validate_budget(b)?;
        self.budget = b.clone();
        self.plans.remove(table);
        let (columns, keys) = self.columns(table, b).await?;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| error("db2_revision"))?;
        let mut p = Plan {
            public: TableChunkPlan {
                protocol: ENGINE_PROTOCOL.into(),
                table: table.clone(),
                key: KeyShape::NoKey,
                strategy: ChunkStrategy::SingleRead,
                estimated_rows: None,
                chunks: Vec::new(),
            },
            columns,
            keys,
            budget: b.clone(),
        };
        let names = key_names(&p);
        if !names.is_empty() {
            p.public.key = if names.len() > 1 {
                KeyShape::CompositePrimaryKey {
                    columns: names.clone(),
                }
            } else if matches!(p.columns[p.keys[0]].kind.as_str(), "VARCHAR" | "CHAR") {
                KeyShape::StringPrimaryKey {
                    column: names[0].clone(),
                    collation: None,
                }
            } else {
                KeyShape::IntegerPrimaryKey {
                    column: names[0].clone(),
                }
            };
            p.public.strategy = ChunkStrategy::Keyset {
                columns: names.clone(),
            };
        }
        let mut lo = None;
        loop {
            if p.public.chunks.len() >= 100_000 {
                return Err(SourceError::budget("db2_plan_limit"));
            }
            let hi = if names.is_empty() {
                None
            } else {
                let mut binds = Vec::new();
                let predicate = match lo.as_deref() {
                    Some(v) => format!(" WHERE {}", bound(&names, v, true, false, &mut binds)?),
                    None => String::new(),
                };
                let projection = names
                    .iter()
                    .map(|k| ident(k))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",");
                let sql = format!(
                    "SELECT {projection} FROM {}{predicate} ORDER BY {projection} OFFSET {} ROWS FETCH FIRST 1 ROW ONLY WITH CS",
                    relation(table)?,
                    b.max_chunk_rows
                );
                let rows = self.query(&sql, &binds, 1, b).await?;
                rows.first()
                    .map(|r| {
                        let mut full = vec![None; p.columns.len()];
                        for (i, &k) in p.keys.iter().enumerate() {
                            full[k] = Some(cell(r, i)?.to_owned());
                        }
                        key_values(&p, &full)
                    })
                    .transpose()?
            };
            if hi.is_some() && hi == lo {
                return Err(error("db2_boundary_not_advancing"));
            }
            p.public.chunks.push(ChunkSpec {
                table: table.clone(),
                plan_revision: self.revision,
                chunk_id: p.public.chunks.len() as u32,
                lo: lo.clone(),
                hi: hi.clone(),
                estimated_rows: None,
            });
            if hi.is_none() {
                break;
            }
            lo = hi;
        }
        let public = p.public.clone();
        self.plans.insert(table.clone(), p);
        Ok(public)
    }

    async fn read_chunk(
        &mut self,
        chunk: &ChunkSpec,
        b: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt, SourceError> {
        validate_budget(b)?;
        self.budget = b.clone();
        let plan = self
            .plans
            .get(&chunk.table)
            .filter(|p| p.public.chunks.contains(chunk))
            .cloned()
            .ok_or_else(|| error("db2_unknown_chunk"))?;
        let low = self.watermark().await?;
        let rows = self
            .rows(&plan, chunk.lo.as_deref(), chunk.hi.as_deref(), b)
            .await?;
        let batch = batch(&plan, &rows)?;
        let mut ipc = Vec::new();
        {
            let mut writer = arrow_ipc::writer::StreamWriter::try_new(&mut ipc, &batch.schema())
                .map_err(|_| error("db2_ipc"))?;
            writer.write(&batch).map_err(|_| error("db2_ipc"))?;
            writer.finish().map_err(|_| error("db2_ipc"))?;
        }
        if ipc.len() as u64 > b.max_chunk_bytes {
            return Err(SourceError::budget("db2_ipc_over_budget"));
        }
        let high = self.watermark().await?;
        sink.push(batch)?;
        self.stats.rows += rows.len() as u64;
        Ok(ChunkReceipt {
            table: chunk.table.clone(),
            plan_revision: chunk.plan_revision,
            chunk_id: chunk.chunk_id,
            rows: rows.len() as u64,
            bytes: ipc.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&ipc)),
            low,
            high,
        })
    }

    async fn watermark(&mut self) -> Result<Option<Watermark>, SourceError> {
        let budget = self.budget.clone();
        let rows = self
            .query(
                "SELECT VARCHAR(CURRENT TIMESTAMP) FROM SYSIBM.SYSDUMMY1",
                &[],
                1,
                &budget,
            )
            .await?;
        Ok(Some(Watermark::Opaque {
            value: format!(
                "db2-observed:{}",
                cell(rows.first().ok_or_else(|| error("db2_watermark"))?, 0)?
            ),
        }))
    }

    async fn digest_range(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest, SourceError> {
        let p = self
            .plans
            .get(table)
            .cloned()
            .ok_or_else(|| error("db2_plan_required"))?;
        let rows = self.rows(&p, lo, hi, &p.budget).await?;
        let mut hashes = rows
            .iter()
            .map(|r| {
                serde_json::to_vec(r)
                    .map(|v| Sha256::digest(v).to_vec())
                    .map_err(|_| error("db2_digest"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        hashes.sort();
        let mut digest = Sha256::new();
        for hash in hashes {
            digest.update(hash);
        }
        Ok(RangeDigest {
            table: table.clone(),
            lo: lo.map(Vec::from),
            hi: hi.map(Vec::from),
            rows: rows.len() as u64,
            digest: format!("{:x}", digest.finalize()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durable_ranges_allow_growth_but_refuse_gaps_and_changed_keys() {
        let table = TableRef {
            schema: "FIXTURE".into(),
            name: "EVENTS".into(),
        };
        let chunk = ChunkSpec {
            table: table.clone(),
            plan_revision: 1,
            chunk_id: 0,
            lo: None,
            hi: None,
            estimated_rows: None,
        };
        let live = TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table,
            key: KeyShape::IntegerPrimaryKey {
                column: "ID".into(),
            },
            strategy: ChunkStrategy::Keyset {
                columns: vec!["ID".into()],
            },
            estimated_rows: None,
            chunks: vec![chunk],
        };
        let mut old = live.clone();
        old.chunks[0].hi = Some(vec![KeyValue::Int(100)]);
        old.chunks.push(ChunkSpec {
            chunk_id: 1,
            lo: old.chunks[0].hi.clone(),
            hi: None,
            ..live.chunks[0].clone()
        });
        assert!(validate_durable_plan(&live, &old).is_ok());
        old.chunks[1].lo = None;
        assert!(validate_durable_plan(&live, &old).is_err());
        old = live.clone();
        old.key = KeyShape::NoKey;
        assert!(validate_durable_plan(&live, &old).is_err());
        old = live.clone();
        old.chunks[0].table.name = "OTHER".into();
        assert!(validate_durable_plan(&live, &old).is_err());
    }

    #[test]
    fn source_compares_composite_bounds_and_values_are_bound() {
        let keys = vec!["odd\"key".into(), "ID".into()];
        let values = vec![KeyValue::Text("x';delete".into()), KeyValue::Int(3)];
        let mut binds = Vec::new();
        let sql = bound(&keys, &values, true, false, &mut binds).unwrap();
        assert_eq!(
            sql,
            "((\"odd\"\"key\" > ?) OR (\"odd\"\"key\" = ? AND \"ID\" >= ?))"
        );
        assert_eq!(binds.len(), 3);
        assert!(!sql.contains("delete"));
        assert!(bound(&keys, &values[..1], false, true, &mut binds).is_err());
        assert!(bound(&keys[..1], &[KeyValue::Null], true, false, &mut binds).is_err());
    }
    #[test]
    fn budget_and_identifiers_fail_closed() {
        assert!(ident("a\"b").unwrap() == "\"a\"\"b\"");
        assert!(ident("a\nb").is_err());
        for b in [
            ReadBudget {
                max_chunk_rows: 0,
                ..ReadBudget::default()
            },
            ReadBudget {
                duty_cycle_percent: 101,
                ..ReadBudget::default()
            },
            ReadBudget {
                max_chunk_bytes: u64::MAX,
                ..ReadBudget::default()
            },
        ] {
            assert!(validate_budget(&b).is_err());
        }
    }
    #[test]
    fn remote_plaintext_is_refused_before_spawning() {
        let cfg = ConnectConfig::new(
            "remote.invalid",
            50000,
            "FIXTURE",
            "reader",
            Zeroizing::new(String::new()),
        );
        assert!(Db2Engine::connect(&mut Command::new("must-not-run"), &cfg).is_err());
    }

    #[test]
    fn typed_batch_preserves_exact_text_and_rejects_bad_integer_and_null() {
        let plan = Plan {
            public: TableChunkPlan {
                protocol: ENGINE_PROTOCOL.into(),
                table: TableRef {
                    schema: "FIXTURE".into(),
                    name: "T".into(),
                },
                key: KeyShape::NoKey,
                strategy: ChunkStrategy::SingleRead,
                estimated_rows: None,
                chunks: Vec::new(),
            },
            columns: vec![
                Column {
                    name: "ID".into(),
                    kind: "BIGINT".into(),
                    nullable: false,
                },
                Column {
                    name: "AMOUNT".into(),
                    kind: "DECIMAL".into(),
                    nullable: true,
                },
                Column {
                    name: "INSTANT".into(),
                    kind: "TIMESTAMP".into(),
                    nullable: true,
                },
            ],
            keys: Vec::new(),
            budget: ReadBudget::default(),
        };
        let row = vec![
            Some(i64::MIN.to_string()),
            Some("-99999999999999999999999.12345678".into()),
            Some("2026-11-05-01.02.03.123456789012".into()),
        ];
        let result = batch(&plan, std::slice::from_ref(&row)).unwrap();
        let text = result
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(text.value(0), row[2].as_deref().unwrap());
        let mut invalid = row;
        invalid[0] = Some("9223372036854775808".into());
        assert!(batch(&plan, &[invalid.clone()]).is_err());
        invalid[0] = None;
        assert!(batch(&plan, &[invalid]).is_err());
        assert_eq!(batch(&plan, &[]).unwrap().num_columns(), 3);
    }
}
