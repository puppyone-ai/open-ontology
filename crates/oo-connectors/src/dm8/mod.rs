//! Opt-in Dameng DM8 reads through a source-only worker running DM's own DPI.
//! C5 is reconciliation, not log CDC. Watermarks are observation times, never
//! a recoverable log position; consistency is B (per-chunk statements, no
//! global snapshot). Every key comparison runs in DM8 under its own collation.
mod types;
mod worker;

use crate::{SourceError, SourceErrorKind, engine::*};
use arrow_array::RecordBatch;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    process::Command,
    time::{Duration, Instant},
};
use types::{Arrow, Column};
use worker::{DriverError, Worker};
use zeroize::Zeroizing;

pub struct ConnectConfig {
    /// Literal loopback inside the worker's network namespace. DM TLS needs a
    /// client key directory and is not offered by this worker yet.
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Zeroizing<String>,
    pub scope: Vec<TableRef>,
}

impl ConnectConfig {
    pub fn new(host: &str, port: u16, user: &str, password: Zeroizing<String>) -> Self {
        Self {
            host: host.into(),
            port,
            user: user.into(),
            password,
            scope: Vec::new(),
        }
    }
}

enum Bind {
    Number(String),
    Text(String),
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

pub struct Dm8Engine {
    worker: Worker,
    plans: BTreeMap<TableRef, Plan>,
    /// Per table, so a C5 run's first plan of a table matches its snapshot's.
    revisions: BTreeMap<TableRef, u32>,
    stats: Stats,
    budget: ReadBudget,
    scope: Vec<TableRef>,
    user: String,
    collation: Option<String>,
}

fn error(fact: &'static str) -> SourceError {
    SourceError::config(fact)
}
fn driver(error: DriverError) -> SourceError {
    match error {
        DriverError::Source(e) => e,
        DriverError::Worker { timeout: true, .. } => SourceError::budget("dm8_statement_timeout"),
        // DM's message text never crosses the worker boundary; its code class does.
        DriverError::Worker { code: 0, .. } => {
            SourceError::new(SourceErrorKind::Protocol, Some("dm8_worker_failure"), None)
        }
        DriverError::Worker { code, .. } => SourceError::new(
            SourceErrorKind::Protocol,
            Some(match code {
                // Measured: -5504 no SELECT on object, -5730 no INI parameter privilege.
                -5504 | -5730 => "dm8_privilege_denied",
                _ => "dm8_database_error",
            }),
            None,
        ),
    }
}

fn ident(value: &str) -> Result<String, SourceError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(error("dm8_identifier"));
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
        return Err(error("dm8_budget"));
    }
    Ok(())
}

// Expanded lexicographic predicates keep every comparison in DM8's collation.
fn bound(
    columns: &[String],
    values: &[KeyValue],
    lower: bool,
    strict: bool,
    binds: &mut Vec<Bind>,
) -> Result<String, SourceError> {
    if columns.is_empty() || columns.len() != values.len() {
        return Err(error("dm8_bound_shape"));
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
            binds.push(match &values[j] {
                KeyValue::Int(v) => Bind::Number(v.to_string()),
                KeyValue::Decimal(v) => Bind::Number(types::decimal_key(v)?),
                KeyValue::Text(v) => Bind::Text(v.clone()),
                _ => return Err(error("dm8_key_type")),
            });
            term.push(format!("{} {op} ?", ident(&columns[j])?));
        }
        terms.push(format!("({})", term.join(" AND ")));
    }
    Ok(format!("({})", terms.join(" OR ")))
}

fn cell(row: &[Option<String>], i: usize) -> Result<&str, SourceError> {
    row.get(i)
        .and_then(Option::as_deref)
        .ok_or_else(|| error("dm8_metadata"))
}

const COLUMNS_SQL: &str = "SELECT COLUMN_NAME,DATA_TYPE,DATA_PRECISION,DATA_SCALE,NULLABLE,DATA_LENGTH,CHAR_USED FROM ALL_TAB_COLUMNS WHERE OWNER=? AND TABLE_NAME=? ORDER BY COLUMN_ID";
const KEYS_SQL: &str = "SELECT CC.COLUMN_NAME FROM ALL_CONSTRAINTS C JOIN ALL_CONS_COLUMNS CC ON C.OWNER=CC.OWNER AND C.CONSTRAINT_NAME=CC.CONSTRAINT_NAME AND C.TABLE_NAME=CC.TABLE_NAME WHERE C.CONSTRAINT_TYPE='P' AND C.OWNER=? AND C.TABLE_NAME=? ORDER BY CC.POSITION";

impl Dm8Engine {
    /// The command must identify a trusted, pinned source-only worker.
    pub fn connect(command: &mut Command, config: &ConnectConfig) -> Result<Self, SourceError> {
        ident(&config.user)?;
        for table in &config.scope {
            relation(table)?;
        }
        if !matches!(config.host.as_str(), "127.0.0.1" | "::1") {
            return Err(error("dm8_tls_not_available_use_loopback_worker"));
        }
        Ok(Self {
            worker: Worker::connect(command, config).map_err(driver)?,
            plans: BTreeMap::new(),
            revisions: BTreeMap::new(),
            stats: Stats {
                sessions: 1,
                ..Stats::default()
            },
            budget: ReadBudget::default(),
            scope: config.scope.clone(),
            user: config.user.clone(),
            collation: None,
        })
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Bind C5 to the table incarnation and complete column/key definitions.
    /// This is a DDL fence, not a recoverable database log position.
    pub async fn table_lineage(&mut self, table: &TableRef) -> Result<String, SourceError> {
        relation(table)?;
        let budget = self.budget.clone();
        let binds = || {
            vec![
                Bind::Text(table.schema.clone()),
                Bind::Text(table.name.clone()),
            ]
        };
        let incarnation = self.query(
            "SELECT OBJECT_ID,TO_CHAR(CREATED,'YYYY-MM-DD HH24:MI:SS.FF6') FROM ALL_OBJECTS WHERE OWNER=? AND OBJECT_NAME=? AND OBJECT_TYPE='TABLE'",
            &binds(), 1, &budget,
        ).await?;
        if incarnation.len() != 1 {
            return Err(error("dm8_table_not_visible"));
        }
        let columns = self.query(COLUMNS_SQL, &binds(), 4096, &budget).await?;
        let keys = self.query(KEYS_SQL, &binds(), 64, &budget).await?;
        let bytes = serde_json::to_vec(&(table, incarnation, columns, keys))
            .map_err(|_| error("dm8_lineage"))?;
        Ok(format!("dm8-catalog:{:x}", Sha256::digest(bytes)))
    }

    /// Keep the baseline partition when source growth moves live boundaries.
    pub fn adopt_keyset_plan(&mut self, durable: &TableChunkPlan) -> Result<(), SourceError> {
        let live = self
            .plans
            .get_mut(&durable.table)
            .ok_or_else(|| error("dm8_plan_required"))?;
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
        let deadline = Duration::from_millis(budget.statement_timeout_ms);
        self.worker.set_timeout(deadline);
        let started = Instant::now();
        let result = (|| {
            let mut rows = Vec::new();
            let mut bytes = 0u64;
            let mut over_budget = false;
            for row in self
                .worker
                .query(sql, binds, (limit + 1) as usize, budget.max_chunk_bytes)
                .map_err(driver)?
            {
                if started.elapsed() > deadline {
                    return Err(SourceError::budget("dm8_statement_timeout"));
                }
                let row = row.map_err(driver)?;
                bytes += row
                    .iter()
                    .map(|v| v.as_ref().map_or(1, |s| s.len() as u64 + 1))
                    .sum::<u64>();
                if rows.len() as u64 >= limit || bytes > budget.max_chunk_bytes {
                    // Drain the bounded overflow and completion so the shared
                    // loader can replan on this same source session.
                    over_budget = true;
                } else {
                    rows.push(row);
                }
            }
            if over_budget {
                Err(SourceError::budget("dm8_chunk_over_budget"))
            } else {
                Ok(rows)
            }
        })();
        let elapsed = started.elapsed();
        let result = if elapsed > deadline {
            Err(SourceError::budget("dm8_statement_timeout"))
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
        let mut columns = Vec::new();
        for row in self.query(COLUMNS_SQL, &binds(), 4096, b).await? {
            let number = |i: usize| {
                row.get(i)
                    .and_then(Option::as_deref)
                    .map(|v| v.trim().parse::<i64>().map_err(|_| error("dm8_metadata")))
                    .transpose()
            };
            columns.push(Column::classify(
                cell(&row, 0)?,
                cell(&row, 1)?,
                number(2)?,
                number(3)?,
                cell(&row, 4)? == "Y",
            )?);
        }
        if columns.is_empty() {
            return Err(error("dm8_table_not_visible"));
        }
        let mut keys = Vec::new();
        for row in self.query(KEYS_SQL, &binds(), 64, b).await? {
            let name = cell(&row, 0)?;
            let index = columns
                .iter()
                .position(|c| c.name == name)
                .ok_or_else(|| error("dm8_key_column"))?;
            if !columns[index].key_admitted() {
                return Err(error("dm8_key_type"));
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
            .map(|c| c.projection(&ident(&c.name)?))
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
                "SELECT {columns} FROM {}{where_sql}{order} FETCH FIRST {} ROWS ONLY",
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
        return Err(error("dm8_stale_plan"));
    }
    let names = match &durable.strategy {
        ChunkStrategy::Keyset { columns } => columns.as_slice(),
        ChunkStrategy::SingleRead => &[],
        _ => return Err(error("dm8_stale_plan")),
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
/// Ranges are cut at 90% of the row budget: durable C5 ranges keep their
/// bounds across cycles, so a range may grow by 10% before a read of it
/// exceeds the hard budget and asks for a rebaseline.
fn planned_rows(b: &ReadBudget) -> u64 {
    (b.max_chunk_rows - b.max_chunk_rows / 10).max(1)
}
fn key_names(p: &Plan) -> Vec<String> {
    p.keys.iter().map(|&i| p.columns[i].name.clone()).collect()
}
fn key_values(p: &Plan, row: &[Option<String>]) -> Result<Vec<KeyValue>, SourceError> {
    p.keys
        .iter()
        .map(|&i| p.columns[i].key_value(cell(row, i)?))
        .collect()
}

fn digest(rows: &[Vec<Option<String>>]) -> Result<String, SourceError> {
    let mut hashes = rows
        .iter()
        .map(|r| {
            serde_json::to_vec(r)
                .map(|v| Sha256::digest(v).to_vec())
                .map_err(|_| error("dm8_digest"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    hashes.sort();
    let mut digest = Sha256::new();
    for hash in hashes {
        digest.update(hash);
    }
    Ok(format!("{:x}", digest.finalize()))
}

impl SourceEngine for Dm8Engine {
    fn family(&self) -> EngineFamily {
        EngineFamily::Dameng
    }

    async fn probe(&mut self) -> Result<ProbeReport, SourceError> {
        let budget = ReadBudget::default();
        let rows = self
            .query(
                "SELECT SVR_VERSION,BUILD_VERSION,DB_VERSION,USER,SF_GET_CASE_SENSITIVE_FLAG(),SF_GET_UNICODE_FLAG() FROM V$INSTANCE",
                &[],
                1,
                &budget,
            )
            .await?;
        let row = rows.first().ok_or_else(|| error("dm8_version"))?;
        let banner = cell(row, 0)?;
        let major = banner
            .rsplit_once(" V")
            .and_then(|(_, v)| v.trim().parse::<u32>().ok())
            .ok_or_else(|| error("dm8_version"))?;
        if major != 8 {
            return Err(error("dm8_version_not_admitted"));
        }
        let version = EngineVersion {
            family: EngineFamily::Dameng,
            major,
            minor: 0,
            patch: 0,
            raw: format!("{banner} {}", cell(row, 1)?),
        };
        self.user = cell(row, 3)?.into();
        let case_sensitive = cell(row, 4)?.to_owned();
        let charset = match cell(row, 5)? {
            "0" => "GB18030",
            "1" => "UTF-8",
            "2" => "EUC-KR",
            _ => return Err(error("dm8_charset")),
        };
        // Optional: plain readers cannot see INI parameters. Never a blocker.
        let parameters = self
            .query(
                "SELECT PARA_NAME,PARA_VALUE FROM V$DM_INI WHERE PARA_NAME IN ('COMPATIBLE_MODE','BLANK_PAD_MODE')",
                &[],
                2,
                &budget,
            )
            .await
            .ok()
            .map(|rows| {
                rows.into_iter()
                    .filter_map(|r| Some((r.first()?.clone()?, r.get(1)?.clone()?)))
                    .collect::<BTreeMap<_, _>>()
            });
        let unmeasured = format!(
            "unmeasured; optional advice: GRANT SELECT ON V$DM_INI TO {}",
            ident(&self.user)?
        );
        let parameter = |name: &str| {
            parameters
                .as_ref()
                .and_then(|p| p.get(name).cloned())
                .unwrap_or_else(|| unmeasured.clone())
        };
        let blank_pad = parameter("BLANK_PAD_MODE");
        let collation = format!(
            "dm8:case_sensitive={case_sensitive};charset={charset};blank_pad_mode={}",
            if blank_pad.starts_with("unmeasured") {
                "unmeasured"
            } else {
                &blank_pad
            }
        );
        self.collation = Some(collation.clone());
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
                        reason: "not implemented; DM LogMiner needs archive/supplemental logging"
                            .into(),
                    },
                )
            }),
        )
        .collect();
        let mut requirements = Vec::new();
        for table in self.scope.clone() {
            let sql = format!("SELECT 1 FROM {} WHERE 1=0", relation(&table)?);
            if self.query(&sql, &[], 1, &budget).await.is_err() {
                requirements.push(Requirement {
                    kind: RequirementKind::Privilege,
                    name: format!("select:{table}"),
                    observed: Some(
                        "read probe failed; verify object existence and SELECT grant".into(),
                    ),
                    required: "SELECT".into(),
                    dba_statement: Some(format!(
                        "GRANT SELECT ON {} TO {}",
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
                ("db_version".into(), cell(row, 2)?.into()),
                ("compatible_mode".into(), parameter("COMPATIBLE_MODE")),
                ("collation".into(), collation),
                (
                    "consistency".into(),
                    "B; per-statement read committed; no global snapshot".into(),
                ),
                (
                    "watermark".into(),
                    "observation timestamp (UTC), not log position".into(),
                ),
                ("type_mapping".into(), types::SUMMARY.into()),
                (
                    "driver".into(),
                    "DM DPI (dmPython) in an isolated source-only worker".into(),
                ),
            ]),
        })
    }

    async fn discover_tables(&mut self, b: &ReadBudget) -> Result<Vec<TableRef>, SourceError> {
        self.query(
            "SELECT OWNER,TABLE_NAME FROM ALL_TABLES WHERE OWNER NOT IN ('SYS','SYSSSO','SYSAUDITOR','CTISYS','SYSJOB') ORDER BY OWNER,TABLE_NAME",
            &[],
            10_000,
            b,
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(TableRef {
                schema: cell(&r, 0)?.into(),
                name: cell(&r, 1)?.into(),
            })
        })
        .collect()
    }

    async fn estimate_bytes(&mut self, table: &TableRef) -> Result<Option<u64>, SourceError> {
        let budget = self.budget.clone();
        let rows = self
            .query(
                "SELECT TABLE_USED_SPACE(?,?)*PAGE() FROM DUAL",
                &[
                    Bind::Text(table.schema.clone()),
                    Bind::Text(table.name.clone()),
                ],
                1,
                &budget,
            )
            .await;
        // Unavailable is None, never zero.
        Ok(rows.ok().and_then(|rows| {
            rows.first()
                .and_then(|r| r.first().cloned().flatten())
                .and_then(|v| v.trim().parse().ok())
        }))
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
        let revision = self.revisions.entry(table.clone()).or_default();
        *revision = revision
            .checked_add(1)
            .ok_or_else(|| error("dm8_revision"))?;
        let revision = *revision;
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
            } else if matches!(p.columns[p.keys[0]].arrow, Arrow::Text) {
                KeyShape::StringPrimaryKey {
                    column: names[0].clone(),
                    collation: self.collation.clone(),
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
                return Err(SourceError::budget("dm8_plan_limit"));
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
                    "SELECT {projection} FROM {}{predicate} ORDER BY {projection} OFFSET {} ROWS FETCH FIRST 1 ROWS ONLY",
                    relation(table)?,
                    planned_rows(b)
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
                return Err(error("dm8_boundary_not_advancing"));
            }
            p.public.chunks.push(ChunkSpec {
                table: table.clone(),
                plan_revision: revision,
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
            .ok_or_else(|| error("dm8_unknown_chunk"))?;
        let low = self.watermark().await?;
        let rows = self
            .rows(&plan, chunk.lo.as_deref(), chunk.hi.as_deref(), b)
            .await?;
        let batch: RecordBatch = types::batch(&plan.columns, &rows)?;
        let mut ipc = Vec::new();
        {
            let mut writer = arrow_ipc::writer::StreamWriter::try_new(&mut ipc, &batch.schema())
                .map_err(|_| error("dm8_ipc"))?;
            writer.write(&batch).map_err(|_| error("dm8_ipc"))?;
            writer.finish().map_err(|_| error("dm8_ipc"))?;
        }
        if ipc.len() as u64 > b.max_chunk_bytes {
            return Err(SourceError::budget("dm8_ipc_over_budget"));
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
                "SELECT TO_CHAR(SYSTIMESTAMP AT TIME ZONE '+00:00','YYYY-MM-DD HH24:MI:SS.FF6') FROM DUAL",
                &[],
                1,
                &budget,
            )
            .await?;
        Ok(Some(Watermark::Opaque {
            value: format!(
                "dm8-observed:{}Z",
                cell(rows.first().ok_or_else(|| error("dm8_watermark"))?, 0)?
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
            .ok_or_else(|| error("dm8_plan_required"))?;
        let rows = self.rows(&p, lo, hi, &p.budget).await?;
        Ok(RangeDigest {
            table: table.clone(),
            lo: lo.map(Vec::from),
            hi: hi.map(Vec::from),
            rows: rows.len() as u64,
            digest: digest(&rows)?,
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
        let keys = vec!["odd\"key".into(), "ID".into(), "AMOUNT".into()];
        let values = vec![
            KeyValue::Text("x';delete".into()),
            KeyValue::Int(3),
            KeyValue::Decimal("-1.50".into()),
        ];
        let mut binds = Vec::new();
        let sql = bound(&keys, &values, true, false, &mut binds).unwrap();
        assert_eq!(
            sql,
            "((\"odd\"\"key\" > ?) OR (\"odd\"\"key\" = ? AND \"ID\" > ?) OR (\"odd\"\"key\" = ? AND \"ID\" = ? AND \"AMOUNT\" >= ?))"
        );
        assert_eq!(binds.len(), 6);
        assert!(!sql.contains("delete"));
        assert!(bound(&keys, &values[..1], false, true, &mut binds).is_err());
        assert!(bound(&keys[..1], &[KeyValue::Null], true, false, &mut binds).is_err());
        let forged = [KeyValue::Decimal("1;DROP".into())];
        assert!(bound(&keys[..1], &forged, true, false, &mut binds).is_err());
    }

    #[test]
    fn planned_ranges_leave_growth_headroom_inside_the_budget() {
        for (max, planned) in [(1, 1), (9, 9), (10, 9), (500, 450), (100_000, 90_000)] {
            let b = ReadBudget {
                max_chunk_rows: max,
                ..ReadBudget::default()
            };
            assert_eq!(planned_rows(&b), planned);
        }
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
            5236,
            "reader",
            Zeroizing::new(String::new()),
        );
        assert!(Dm8Engine::connect(&mut Command::new("must-not-run"), &cfg).is_err());
    }

    #[test]
    fn digest_is_order_independent_and_counts_duplicates_and_nulls() {
        let a = vec![Some("1".to_owned()), None];
        let b = vec![Some("2".to_owned()), Some(String::new())];
        assert_eq!(
            digest(&[a.clone(), b.clone()]).unwrap(),
            digest(&[b.clone(), a.clone()]).unwrap()
        );
        assert_ne!(
            digest(std::slice::from_ref(&a)).unwrap(),
            digest(&[a.clone(), a.clone()]).unwrap()
        );
        let empty = vec![Some("1".to_owned()), Some(String::new())];
        assert_ne!(digest(&[a]).unwrap(), digest(&[empty]).unwrap());
    }
}
