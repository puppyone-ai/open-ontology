//! Experimental SQL Server reader. All table reads share one SNAPSHOT transaction.
//! CT/CDC setup is DBA-owned. Dropping or poisoning the reader closes its socket.

mod arrow_batch;
mod fail;
mod types;

use crate::{SourceError, engine::*};
use arrow_array::RecordBatch;
use arrow_batch::{ColumnSpec, encode_cells};
use arrow_ipc::reader::StreamReader;
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Cursor,
    time::{Duration, Instant},
};
use tiberius::{AuthMethod, Client, Config, EncryptionLevel, Row};
use tokio::{
    net::TcpStream,
    time::{sleep, timeout},
};
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};
use types::{Cell, SqlKind, cell_from_column, kind_for};
use zeroize::Zeroizing;

fn err(code: &'static str) -> SourceError {
    SourceError::protocol(code)
}
fn quote(s: &str) -> Result<String, SourceError> {
    if s.is_empty() || s.encode_utf16().count() > 128 || s.chars().any(char::is_control) {
        return Err(err("sqlserver_identifier"));
    }
    Ok(format!("[{}]", s.replace(']', "]]")))
}
fn literal(s: &str) -> String {
    format!("N'{}'", s.replace('\'', "''"))
}
fn relation(t: &TableRef) -> Result<String, SourceError> {
    Ok(format!("{}.{}", quote(&t.schema)?, quote(&t.name)?))
}
fn text(row: &Row, i: usize) -> Result<String, SourceError> {
    row.try_get::<&str, _>(i)
        .map_err(|_| err("sqlserver_metadata_type"))?
        .map(str::to_owned)
        .ok_or_else(|| err("sqlserver_metadata_null"))
}
fn integer(row: &Row, i: usize) -> Result<i64, SourceError> {
    row.try_get::<i64, _>(i)
        .map_err(|_| err("sqlserver_metadata_type"))?
        .ok_or_else(|| err("sqlserver_metadata_null"))
}
fn validate_budget(b: &ReadBudget) -> Result<(), SourceError> {
    if b.max_chunk_rows == 0
        || b.max_chunk_rows > 100_000
        || b.max_chunk_bytes == 0
        || b.max_chunk_bytes > 32 * 1024 * 1024
        || !(1..=100).contains(&b.duty_cycle_percent)
        || b.statement_timeout_ms == 0
        || b.statement_timeout_ms > 3_600_000
    {
        return Err(err("sqlserver_budget"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeMode {
    Ct,
    Cdc,
}

/// Persist this opaque watermark only after the target has committed its batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Position {
    protocol: String,
    source_epoch: String,
    mode: ChangeMode,
    value: String,
    schema: String,
}
impl Position {
    fn watermark(&self) -> Result<Watermark, SourceError> {
        Ok(Watermark::Opaque {
            value: serde_json::to_string(self).map_err(|_| err("sqlserver_position"))?,
        })
    }
    fn parse(w: &Watermark) -> Result<Self, SourceError> {
        let Watermark::Opaque { value } = w else {
            return Err(err("sqlserver_position"));
        };
        let p: Self = serde_json::from_str(value).map_err(|_| err("sqlserver_position"))?;
        if p.protocol != "oo.sqlserver.position/v1" {
            return Err(err("sqlserver_position"));
        }
        match p.mode {
            ChangeMode::Ct => {
                p.value
                    .parse::<u64>()
                    .map_err(|_| err("sqlserver_position"))?;
            }
            ChangeMode::Cdc => {
                lsn(&p.value)?;
            }
        }
        Ok(p)
    }
}
fn lsn(s: &str) -> Result<String, SourceError> {
    if s.len() != 20 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(err("sqlserver_lsn"));
    }
    Ok(format!("0x{s}"))
}

/// The password has no Debug implementation and is never included in errors.
pub struct ConnectConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Zeroizing<String>,
    pub ca_pem: Vec<u8>,
    pub tables: Vec<TableRef>,
    pub mode: Option<ChangeMode>,
}
#[derive(Clone)]
struct Table {
    name: TableRef,
    columns: Vec<ColumnSpec>,
    keys: Vec<usize>,
    object: i64,
    ct: bool,
    capture: Option<String>,
}
impl Table {
    fn key_names(&self) -> Vec<String> {
        self.keys
            .iter()
            .map(|i| self.columns[*i].name.clone())
            .collect()
    }
    fn projection(&self, alias: &str) -> Result<String, SourceError> {
        self.columns
            .iter()
            .map(|c| {
                let name = format!("{alias}.{}", quote(&c.name)?);
                Ok(match c.sql_type_name.as_str() {
                    "varchar" | "char" | "text" => format!("CAST({name} AS nvarchar(max))"),
                    "money" => format!("CAST({name} AS decimal(19,4))"),
                    "smallmoney" => format!("CAST({name} AS decimal(10,4))"),
                    _ => name,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join(","))
    }
    fn ordering(&self, alias: &str) -> Result<String, SourceError> {
        self.keys
            .iter()
            .map(|i| Ok(format!("{alias}.{}", quote(&self.columns[*i].name)?)))
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join(","))
    }
    fn key_projection(&self, alias: &str) -> Result<String, SourceError> {
        self.keys
            .iter()
            .map(|i| {
                let c = &self.columns[*i];
                let col = format!("{alias}.{}", quote(&c.name)?);
                Ok(if c.kind == SqlKind::Binary {
                    format!("CONVERT(varchar(max),{col},2)")
                } else {
                    format!("CONVERT(nvarchar(max),{col})")
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join(","))
    }
    fn keys_from(&self, r: &Row, start: usize) -> Result<Vec<KeyValue>, SourceError> {
        self.keys
            .iter()
            .enumerate()
            .map(|(j, i)| {
                let s = text(r, start + j)?;
                Ok(match self.columns[*i].kind {
                    SqlKind::Int => KeyValue::Int(s.parse().map_err(|_| err("sqlserver_key"))?),
                    SqlKind::Binary => KeyValue::Bytes(s),
                    SqlKind::Decimal => KeyValue::Decimal(s),
                    _ => KeyValue::Text(s),
                })
            })
            .collect()
    }
    fn bound(&self, values: &[KeyValue], lower: bool, alias: &str) -> Result<String, SourceError> {
        if values.len() != self.keys.len() || values.is_empty() {
            return Err(err("sqlserver_key_width"));
        }
        let terms = self
            .keys
            .iter()
            .zip(values)
            .map(|(i, v)| {
                let c = &self.columns[*i];
                let col = format!("{alias}.{}", quote(&c.name)?);
                let value = match (&c.kind, v) {
                    (SqlKind::Int, KeyValue::Int(n)) => n.to_string(),
                    (SqlKind::Nvarchar, KeyValue::Text(s)) => {
                        if matches!(c.sql_type_name.as_str(), "varchar" | "char") {
                            let collation=quote(c.collation.as_deref().ok_or_else(||err("sqlserver_key_collation"))?)?;
                            let width=c.max_length.filter(|n|*n>0 && *n<=8000).ok_or_else(||err("sqlserver_key_width"))?;
                            format!("CAST({} COLLATE {collation} AS varchar({width})) COLLATE {collation}",literal(s))
                        }else{literal(s)}
                    },
                    (SqlKind::Decimal, KeyValue::Decimal(s)) => {
                        if s.is_empty()
                            || !s
                                .bytes()
                                .all(|b| b.is_ascii_digit() || b == b'.' || b == b'-')
                        {
                            return Err(err("sqlserver_key"));
                        }
                        format!(
                            "CAST({} AS decimal({},{}))",
                            literal(s),
                            c.precision,
                            c.scale
                        )
                    }
                    (SqlKind::Binary, KeyValue::Bytes(s)) => {
                        if s.len() % 2 != 0 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
                            return Err(err("sqlserver_key"));
                        }
                        format!("0x{s}")
                    }
                    _ => return Err(err("sqlserver_key_type")),
                };
                Ok((col, value))
            })
            .collect::<Result<Vec<_>, SourceError>>()?;
        let mut disj = Vec::new();
        for i in 0..terms.len() {
            let mut conj = terms[..i]
                .iter()
                .map(|(c, v)| format!("{c}={v}"))
                .collect::<Vec<_>>();
            let op = if lower {
                if i + 1 == terms.len() { ">=" } else { ">" }
            } else {
                "<"
            };
            conj.push(format!("{}{}{}", terms[i].0, op, terms[i].1));
            disj.push(format!("({})", conj.join(" AND ")));
        }
        Ok(format!("({})", disj.join(" OR ")))
    }
    fn filter(
        &self,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
        alias: &str,
    ) -> Result<String, SourceError> {
        let mut parts = Vec::new();
        if let Some(v) = lo {
            parts.push(self.bound(v, true, alias)?);
        }
        if let Some(v) = hi {
            parts.push(self.bound(v, false, alias)?);
        }
        Ok(if parts.is_empty() {
            "1=1".into()
        } else {
            parts.join(" AND ")
        })
    }
}

pub struct SqlServerEngine {
    client: Option<Client<Compat<TcpStream>>>,
    scope: Vec<TableRef>,
    mode: Option<ChangeMode>,
    budget: ReadBudget,
    tables: BTreeMap<TableRef, Table>,
    plans: BTreeMap<TableRef, TableChunkPlan>,
    epoch: String,
    schema: String,
    anchor: Option<Position>,
    report: ProbeReport,
    read_time: Duration,
    pacing_time: Duration,
    queries: u64,
}
impl SqlServerEngine {
    /// Recheck database and object identity instead of reusing a cached probe.
    pub async fn source_lineage(&mut self) -> Result<String, SourceError> {
        self.rows("ROLLBACK TRANSACTION", 0).await?;
        self.tables.clear();
        self.plans.clear();
        self.anchor = None;
        self.initialize().await?;
        Ok(format!("{}:{}", self.epoch, self.schema))
    }

    pub fn resource_receipt(&self) -> serde_json::Value {
        serde_json::json!({
            "queries": self.queries,
            "read_ns": self.read_time.as_nanos(),
            "pacing_ns": self.pacing_time.as_nanos(),
            "connection_peak": null,
            "connection_peak_basis": "requires independent source observation"
        })
    }

    pub async fn connect(c: ConnectConfig, budget: ReadBudget) -> Result<Self, SourceError> {
        validate_budget(&budget)?;
        if c.ca_pem.is_empty() || c.tables.is_empty() || c.tables.len() > 1000 {
            return Err(err("sqlserver_tls_or_scope"));
        }
        for t in &c.tables {
            relation(t)?;
        }
        let mut cfg = Config::new();
        cfg.host(&c.host);
        cfg.port(c.port);
        cfg.database(&c.database);
        cfg.authentication(AuthMethod::sql_server(&c.user, c.password.as_str()));
        cfg.encryption(EncryptionLevel::Required);
        cfg.trust_cert_ca_bundle(c.ca_pem);
        cfg.readonly(true);
        cfg.application_name("oo-sqlserver-source");
        let deadline = Duration::from_millis(budget.statement_timeout_ms);
        cfg.command_timeout(Some(deadline));
        cfg.handshake_timeout(Some(deadline));
        let client = timeout(deadline, async {
            let socket = TcpStream::connect(cfg.get_addr())
                .await
                .map_err(|_| err("sqlserver_connect"))?;
            socket
                .set_nodelay(true)
                .map_err(|_| err("sqlserver_connect"))?;
            Client::connect(cfg, socket.compat_write())
                .await
                .map_err(|_| err("sqlserver_login_or_tls"))
        })
        .await
        .map_err(|_| err("sqlserver_connect_timeout"))??;
        let mut engine = Self {
            client: Some(client),
            scope: c.tables,
            mode: c.mode,
            budget,
            tables: BTreeMap::new(),
            plans: BTreeMap::new(),
            epoch: String::new(),
            schema: String::new(),
            anchor: None,
            read_time: Duration::ZERO,
            pacing_time: Duration::ZERO,
            queries: 0,
            report: ProbeReport {
                protocol: ENGINE_PROTOCOL.into(),
                family: EngineFamily::Sqlserver,
                version: EngineVersion::parse(EngineFamily::Sqlserver, "0")?,
                capabilities: BTreeMap::new(),
                settings: BTreeMap::new(),
            },
        };
        engine.initialize().await?;
        Ok(engine)
    }
    async fn rows(&mut self, sql: &str, cap: u64) -> Result<Vec<Row>, SourceError> {
        let started = Instant::now();
        let b = self.budget.clone();
        let result = timeout(Duration::from_millis(b.statement_timeout_ms), async {
            let client = self
                .client
                .as_mut()
                .ok_or_else(|| err("sqlserver_session_closed"))?;
            let mut stream = client
                .simple_query(sql)
                .await
                .map_err(|_| err("sqlserver_query"))?;
            let mut result = Vec::new();
            let mut bytes = 0u64;
            while let Some(item) = stream
                .try_next()
                .await
                .map_err(|_| err("sqlserver_stream"))?
            {
                if let Some(row) = item.into_row() {
                    // Bound variable-width TDS payloads before retaining a result row.
                    for i in 0..row.len() {
                        let value = row
                            .get_column_data(i)
                            .map_err(|_| err("sqlserver_decode"))?;
                        bytes = bytes.saturating_add(match value {
                            tiberius::ColumnData::String(Some(s)) => s.len() as u64,
                            tiberius::ColumnData::Binary(Some(s)) => s.len() as u64,
                            _ => 32,
                        });
                    }
                    if result.len() as u64 >= cap || bytes > b.max_chunk_bytes {
                        return Err(SourceError::budget("sqlserver_result_budget"));
                    }
                    result.push(row);
                }
            }
            Ok(result)
        })
        .await
        .map_err(|_| SourceError::budget("sqlserver_deadline"))
        .and_then(|x| x);
        if result.is_err() {
            self.client.take();
        }
        let elapsed = started.elapsed();
        self.read_time += elapsed;
        self.queries += 1;
        let paced = Instant::now();
        sleep(
            elapsed
                .mul_f64(f64::from(100 - b.duty_cycle_percent) / f64::from(b.duty_cycle_percent)),
        )
        .await;
        self.pacing_time += paced.elapsed();
        result
    }
    async fn initialize(&mut self) -> Result<(), SourceError> {
        let rows=self.rows("SELECT CONVERT(nvarchar(128),SERVERPROPERTY('ProductVersion')),CONVERT(nvarchar(128),SERVERPROPERTY('Edition')),CONVERT(bigint,snapshot_isolation_state),CONVERT(bigint,is_cdc_enabled),CONVERT(nvarchar(36),database_guid),CONVERT(nvarchar(36),recovery_fork_guid) FROM sys.databases d JOIN sys.database_recovery_status r ON r.database_id=d.database_id WHERE d.database_id=DB_ID()",1).await?;
        let r = rows
            .first()
            .ok_or_else(|| err("sqlserver_probe_visibility"))?;
        self.report.version = EngineVersion::parse(EngineFamily::Sqlserver, &text(r, 0)?)?;
        if !(15..=17).contains(&self.report.version.major) {
            return Err(err("sqlserver_unvalidated_major"));
        }
        self.report.settings.insert("edition".into(), text(r, 1)?);
        self.epoch = format!("{}:{}", text(r, 4)?, text(r, 5)?);
        self.report
            .settings
            .insert("source_epoch".into(), self.epoch.clone());
        let snapshot = integer(r, 2)? == 1;
        let cdc = integer(r, 3)? == 1;
        self.report
            .settings
            .insert("snapshot_isolation".into(), snapshot.to_string());
        if !snapshot {
            self.report.capabilities.insert(
                Capability::ChunkedSnapshot,
                CapabilityState::NeedsSetup {
                    requirements: vec![Requirement {
                        kind: RequirementKind::Parameter,
                        name: "snapshot_isolation".into(),
                        observed: Some("disabled".into()),
                        required: "ON".into(),
                        dba_statement: Some(
                            "ALTER DATABASE CURRENT SET ALLOW_SNAPSHOT_ISOLATION ON".into(),
                        ),
                    }],
                },
            );
            return Ok(());
        }
        self.rows(
            "SET TRANSACTION ISOLATION LEVEL SNAPSHOT; BEGIN TRANSACTION",
            0,
        )
        .await?;
        // Capture CT before any source-table access establishes the snapshot.
        let current = match self.mode {
            Some(ChangeMode::Ct) => Some(self.current(ChangeMode::Ct).await?),
            Some(ChangeMode::Cdc) if cdc => Some(self.current(ChangeMode::Cdc).await?),
            _ => None,
        };
        for name in self.scope.clone() {
            let t = self.catalog(&name).await?;
            self.tables.insert(name, t);
        }
        let facts = self
            .tables
            .values()
            .map(|t| {
                format!(
                    "{:?}:{:?}:{:?}:{}:{:?}",
                    t.name, t.columns, t.keys, t.object, t.capture
                )
            })
            .collect::<Vec<_>>();
        self.schema = format!("{:x}", Sha256::digest(facts.join("\n")));
        let ct = self.tables.values().all(|t| t.ct);
        let cdc = cdc && self.tables.values().all(|t| t.capture.is_some());
        for cap in [
            Capability::Catalog,
            Capability::ChunkedSnapshot,
            Capability::DigestDiff,
        ] {
            self.report
                .capabilities
                .insert(cap, CapabilityState::Available);
        }
        for (cap, enabled, statement) in [
            (
                Capability::NativeChanges,
                ct,
                "ALTER DATABASE CURRENT SET CHANGE_TRACKING = ON (CHANGE_RETENTION = 7 DAYS, AUTO_CLEANUP = ON); -- Then ALTER TABLE [schema].[table] ENABLE CHANGE_TRACKING",
            ),
            (
                Capability::LogCdc,
                cdc,
                "EXEC sys.sp_cdc_enable_db; -- Then EXEC sys.sp_cdc_enable_table @source_schema=N'schema', @source_name=N'table', @role_name=N'cdc_reader'",
            ),
        ] {
            self.report.capabilities.insert(
                cap,
                if enabled {
                    CapabilityState::Available
                } else {
                    CapabilityState::NeedsSetup {
                        requirements: vec![Requirement {
                            kind: RequirementKind::Feature,
                            name: format!("{cap:?}"),
                            observed: Some("not enabled on all selected tables".into()),
                            required: "DBA enablement and read grants".into(),
                            dba_statement: Some(statement.into()),
                        }],
                    }
                },
            );
        }
        if let Some(mode) = self.mode {
            if !(if mode == ChangeMode::Ct { ct } else { cdc }) {
                return Err(err("sqlserver_change_mode_unavailable"));
            }
            let value = current.ok_or_else(|| err("sqlserver_change_mode_unavailable"))?;
            self.anchor = Some(Position {
                protocol: "oo.sqlserver.position/v1".into(),
                source_epoch: self.epoch.clone(),
                mode,
                value,
                schema: self.schema.clone(),
            });
        }
        self.report.capabilities.insert(
            Capability::Watermark,
            if self.anchor.is_some() {
                CapabilityState::Available
            } else {
                CapabilityState::Unsupported {
                    reason: "select an enabled CT or CDC mode".into(),
                }
            },
        );
        Ok(())
    }
    async fn current(&mut self, mode: ChangeMode) -> Result<String, SourceError> {
        let sql = match mode {
            ChangeMode::Ct => "SELECT CONVERT(nvarchar(30),CHANGE_TRACKING_CURRENT_VERSION())",
            ChangeMode::Cdc => "SELECT CONVERT(varchar(20),sys.fn_cdc_get_max_lsn(),2)",
        };
        let r = self.rows(sql, 1).await?;
        text(r.first().ok_or_else(|| err("sqlserver_watermark"))?, 0)
    }
    async fn catalog(&mut self, name: &TableRef) -> Result<Table, SourceError> {
        let rel = relation(name)?;
        let perm=self.rows(&format!("SELECT CONVERT(bigint,HAS_PERMS_BY_NAME({},'OBJECT','SELECT')),CONVERT(bigint,HAS_PERMS_BY_NAME({},'OBJECT','INSERT')),CONVERT(bigint,HAS_PERMS_BY_NAME({},'OBJECT','UPDATE')),CONVERT(bigint,HAS_PERMS_BY_NAME({},'OBJECT','DELETE')),CONVERT(bigint,HAS_PERMS_BY_NAME({},'OBJECT','ALTER')),CONVERT(bigint,HAS_PERMS_BY_NAME({},'OBJECT','CONTROL'))",literal(&rel),literal(&rel),literal(&rel),literal(&rel),literal(&rel),literal(&rel)),1).await?;
        let p = perm.first().ok_or_else(|| err("sqlserver_permissions"))?;
        if integer(p, 0)? != 1 || (1..6).any(|i| integer(p, i) != Ok(0)) {
            return Err(err("sqlserver_reader_not_readonly"));
        }
        let rows=self.rows(&format!("SELECT c.name,ty.name,CONVERT(bigint,c.precision),CONVERT(bigint,c.scale),CONVERT(bigint,c.is_nullable),COALESCE(c.collation_name,N''),CONVERT(bigint,c.max_length),CONVERT(bigint,c.object_id),CONVERT(bigint,COALESCE(ic.key_ordinal,0)),CONVERT(bigint,CASE WHEN ct.object_id IS NULL THEN 0 ELSE 1 END) FROM sys.columns c JOIN sys.types ty ON ty.user_type_id=c.user_type_id LEFT JOIN sys.indexes ix ON ix.object_id=c.object_id AND ix.is_primary_key=1 LEFT JOIN sys.index_columns ic ON ic.object_id=c.object_id AND ic.index_id=ix.index_id AND ic.column_id=c.column_id LEFT JOIN sys.change_tracking_tables ct ON ct.object_id=c.object_id WHERE c.object_id=OBJECT_ID({}) ORDER BY c.column_id",literal(&rel)),1024).await?;
        if rows.is_empty() {
            return Err(err("sqlserver_catalog_empty"));
        }
        let mut columns = Vec::new();
        let mut keys = Vec::new();
        for r in &rows {
            let kind = kind_for(&text(r, 1)?).map_err(|_| err("sqlserver_type_unsupported"))?;
            let ordinal = integer(r, 8)?;
            if ordinal > 0 {
                if integer(r, 4)? != 0
                    || !matches!(
                        kind,
                        SqlKind::Int | SqlKind::Nvarchar | SqlKind::Decimal | SqlKind::Binary
                    )
                {
                    return Err(err("sqlserver_key_unsupported"));
                }
                keys.push((ordinal, columns.len()));
            }
            columns.push(ColumnSpec {
                name: text(r, 0)?,
                kind,
                nullable: integer(r, 4)? == 1,
                precision: integer(r, 2)? as u8,
                scale: integer(r, 3)? as u8,
                collation: Some(text(r, 5)?).filter(|s| !s.is_empty()),
                is_identity: false,
                sql_type_name: text(r, 1)?,
                max_length: Some(integer(r, 6)? as i32),
            });
        }
        keys.sort();
        let capture = if self.mode == Some(ChangeMode::Cdc) {
            let captures=self.rows(&format!("SELECT capture_instance FROM cdc.change_tables WHERE source_object_id=OBJECT_ID({})",literal(&rel)),2).await?;
            if captures.len() != 1 {
                return Err(err("sqlserver_cdc_capture_ambiguous"));
            }
            Some(text(&captures[0], 0)?)
        } else {
            None
        };
        Ok(Table {
            name: name.clone(),
            columns,
            keys: keys.into_iter().map(|(_, i)| i).collect(),
            object: integer(&rows[0], 7)?,
            ct: integer(&rows[0], 9)? == 1,
            capture,
        })
    }
    fn table(&self, t: &TableRef) -> Result<Table, SourceError> {
        self.tables
            .get(t)
            .cloned()
            .ok_or_else(|| err("sqlserver_out_of_scope"))
    }
    fn decode(
        t: &Table,
        rows: &[Row],
        offset: usize,
    ) -> Result<(RecordBatch, Vec<u8>), SourceError> {
        let cells = rows
            .iter()
            .map(|r| {
                t.columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let data = r
                            .get_column_data(offset + i)
                            .map_err(|_| err("sqlserver_row_width"))?;
                        let value = cell_from_column(&c.sql_type_name, c.precision, c.scale, data)
                            .map_err(|_| err("sqlserver_decode"))?;
                        if value == Cell::Null && !c.nullable {
                            return Err(err("sqlserver_unexpected_null"));
                        }
                        Ok(value)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, SourceError>>()?;
        let package = encode_cells(&t.columns, &cells).map_err(|_| err("sqlserver_arrow"))?;
        canonical_batch(&package.ipc)
    }
}

fn canonical_batch(ipc: &[u8]) -> Result<(RecordBatch, Vec<u8>), SourceError> {
    let mut reader =
        StreamReader::try_new(Cursor::new(ipc), None).map_err(|_| err("sqlserver_arrow"))?;
    let batch = reader
        .next()
        .ok_or_else(|| err("sqlserver_arrow"))?
        .map_err(|_| err("sqlserver_arrow"))?;
    // Hash the exact delivered schema and the pipeline's default IPC options.
    // Decoding rebuilds metadata maps; the preview's wire bytes are not canonical.
    let mut writer = arrow_ipc::writer::StreamWriter::try_new(Vec::new(), &batch.schema())
        .map_err(|_| err("sqlserver_arrow"))?;
    writer.write(&batch).map_err(|_| err("sqlserver_arrow"))?;
    writer.finish().map_err(|_| err("sqlserver_arrow"))?;
    let ipc = writer.into_inner().map_err(|_| err("sqlserver_arrow"))?;
    Ok((batch, ipc))
}

impl SourceEngine for SqlServerEngine {
    fn family(&self) -> EngineFamily {
        EngineFamily::Sqlserver
    }
    async fn probe(&mut self) -> Result<ProbeReport, SourceError> {
        Ok(self.report.clone())
    }
    async fn discover_tables(&mut self, _: &ReadBudget) -> Result<Vec<TableRef>, SourceError> {
        Ok(self.scope.clone())
    }
    async fn watermark(&mut self) -> Result<Option<Watermark>, SourceError> {
        self.anchor.as_ref().map(Position::watermark).transpose()
    }
    async fn plan_chunks(
        &mut self,
        name: &TableRef,
        b: &ReadBudget,
    ) -> Result<TableChunkPlan, SourceError> {
        validate_budget(b)?;
        self.budget = b.clone();
        let t = self.table(name)?;
        let names = t.key_names();
        let key = match names.as_slice() {
            [] => KeyShape::NoKey,
            [c] if t.columns[t.keys[0]].kind == SqlKind::Int => {
                KeyShape::IntegerPrimaryKey { column: c.clone() }
            }
            [c] if t.columns[t.keys[0]].kind == SqlKind::Nvarchar => KeyShape::StringPrimaryKey {
                column: c.clone(),
                collation: t.columns[t.keys[0]].collation.clone(),
            },
            _ => KeyShape::CompositePrimaryKey {
                columns: names.clone(),
            },
        };
        let strategy = if names.is_empty() {
            ChunkStrategy::SingleRead
        } else {
            ChunkStrategy::Keyset { columns: names }
        };
        let mut chunks = Vec::new();
        let mut lo = None;
        loop {
            let hi = if t.keys.is_empty() {
                None
            } else {
                let sql = format!(
                    "SELECT TOP ({}) {} FROM {} s WHERE {} ORDER BY {}",
                    b.max_chunk_rows + 1,
                    t.key_projection("s")?,
                    relation(name)?,
                    t.filter(lo.as_deref(), None, "s")?,
                    t.ordering("s")?
                );
                let rows = self.rows(&sql, b.max_chunk_rows + 1).await?;
                if rows.len() as u64 > b.max_chunk_rows {
                    Some(t.keys_from(rows.last().unwrap(), 0)?)
                } else {
                    None
                }
            };
            chunks.push(ChunkSpec {
                table: name.clone(),
                plan_revision: 1,
                chunk_id: chunks.len() as u32,
                lo: lo.clone(),
                hi: hi.clone(),
                estimated_rows: None,
            });
            if hi.is_none() {
                break;
            }
            if hi == lo || chunks.len() >= 100_000 {
                return Err(SourceError::budget("sqlserver_plan_budget"));
            }
            lo = hi;
        }
        let plan = TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table: name.clone(),
            key,
            strategy,
            estimated_rows: None,
            chunks,
        };
        self.plans.insert(name.clone(), plan.clone());
        Ok(plan)
    }
    async fn read_chunk(
        &mut self,
        c: &ChunkSpec,
        b: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt, SourceError> {
        validate_budget(b)?;
        if !self
            .plans
            .get(&c.table)
            .is_some_and(|p| p.chunks.contains(c))
        {
            return Err(err("sqlserver_unissued_chunk"));
        }
        self.budget = b.clone();
        let t = self.table(&c.table)?;
        let order = if t.keys.is_empty() {
            String::new()
        } else {
            format!(" ORDER BY {}", t.ordering("s")?)
        };
        let sql = format!(
            "SELECT TOP ({}) {} FROM {} s WHERE {}{}",
            b.max_chunk_rows + 1,
            t.projection("s")?,
            relation(&c.table)?,
            t.filter(c.lo.as_deref(), c.hi.as_deref(), "s")?,
            order
        );
        let rows = self.rows(&sql, b.max_chunk_rows + 1).await?;
        if rows.len() as u64 > b.max_chunk_rows {
            return Err(SourceError::budget("sqlserver_chunk_rows"));
        }
        let (batch, ipc) = Self::decode(&t, &rows, 0)?;
        if ipc.len() as u64 > b.max_chunk_bytes {
            return Err(SourceError::budget("sqlserver_chunk_bytes"));
        }
        sink.push(batch)?;
        let watermark = self.watermark().await?;
        Ok(ChunkReceipt {
            table: c.table.clone(),
            plan_revision: c.plan_revision,
            chunk_id: c.chunk_id,
            rows: rows.len() as u64,
            bytes: ipc.len() as u64,
            sha256: format!("{:x}", Sha256::digest(ipc)),
            low: watermark.clone(),
            high: watermark,
        })
    }
    async fn digest_range(
        &mut self,
        name: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest, SourceError> {
        let t = self.table(name)?;
        // Only row digests cross TDS; length-prefixing and NULLs are handled by JSON.
        let cols = t
            .columns
            .iter()
            .map(|c| Ok(format!("s.{} AS {}", quote(&c.name)?, quote(&c.name)?)))
            .collect::<Result<Vec<_>, SourceError>>()?
            .join(",");
        let sql = format!(
            "SELECT TOP ({}) CONVERT(varchar(64),HASHBYTES('SHA2_256',(SELECT {cols} FOR JSON PATH,INCLUDE_NULL_VALUES,WITHOUT_ARRAY_WRAPPER)),2) FROM {} s WHERE {}",
            self.budget.max_chunk_rows + 1,
            relation(name)?,
            t.filter(lo, hi, "s")?
        );
        let rows = self.rows(&sql, self.budget.max_chunk_rows + 1).await?;
        if rows.len() as u64 > self.budget.max_chunk_rows {
            return Err(SourceError::budget("sqlserver_digest_rows"));
        }
        let mut hashes = rows
            .iter()
            .map(|r| text(r, 0))
            .collect::<Result<Vec<_>, _>>()?;
        hashes.sort();
        Ok(RangeDigest {
            table: name.clone(),
            lo: lo.map(<[_]>::to_vec),
            hi: hi.map(<[_]>::to_vec),
            rows: rows.len() as u64,
            digest: format!("{:x}", Sha256::digest(hashes.join(""))),
        })
    }
}

impl ChangeSource for SqlServerEngine {
    async fn earliest_available(&mut self) -> Result<Watermark, SourceError> {
        let mut p = self
            .anchor
            .clone()
            .ok_or_else(|| err("sqlserver_change_mode_unavailable"))?;
        let mut earliest = String::new();
        for t in self.tables.values().cloned().collect::<Vec<_>>() {
            if t.keys.is_empty() {
                return Err(err("sqlserver_changes_require_primary_key"));
            }
            let sql = match p.mode {
                ChangeMode::Ct => format!(
                    "SELECT CONVERT(nvarchar(30),CHANGE_TRACKING_MIN_VALID_VERSION({}))",
                    t.object
                ),
                ChangeMode::Cdc => format!(
                    "SELECT CONVERT(varchar(20),sys.fn_cdc_get_min_lsn({}),2)",
                    literal(
                        t.capture
                            .as_deref()
                            .ok_or_else(|| err("sqlserver_cdc_capture"))?
                    )
                ),
            };
            let r = self.rows(&sql, 1).await?;
            let value = text(r.first().ok_or_else(|| err("sqlserver_retention"))?, 0)?;
            if p.mode == ChangeMode::Ct {
                let v = value
                    .parse::<u64>()
                    .map_err(|_| err("sqlserver_retention"))?;
                if earliest.is_empty()
                    || v > earliest
                        .parse::<u64>()
                        .map_err(|_| err("sqlserver_retention"))?
                {
                    earliest = value;
                }
            } else {
                lsn(&value)?;
                if value == "00000000000000000000" {
                    return Err(err("sqlserver_cdc_retention_unavailable"));
                }
                if value > earliest {
                    earliest = value;
                }
            }
        }
        p.value = earliest;
        p.watermark()
    }
    async fn next_batch(
        &mut self,
        from: &Watermark,
        b: &ReadBudget,
    ) -> Result<Option<ChangeBatch>, SourceError> {
        validate_budget(b)?;
        self.budget = b.clone();
        let prior = Position::parse(from)?;
        // Poll against a fresh snapshot, not the anchor of the preceding call.
        // Issued snapshot chunks cannot be reused after crossing this boundary.
        self.rows("ROLLBACK TRANSACTION", 0).await?;
        self.tables.clear();
        self.plans.clear();
        self.anchor = None;
        self.initialize().await?;
        let high = self
            .anchor
            .clone()
            .ok_or_else(|| err("sqlserver_change_mode_unavailable"))?;
        if prior.source_epoch != high.source_epoch
            || prior.schema != high.schema
            || prior.mode != high.mode
        {
            return Err(err("sqlserver_rebaseline_identity_or_schema"));
        }
        let min = Position::parse(&self.earliest_available().await?)?;
        let valid = match high.mode {
            ChangeMode::Ct => {
                let v = prior
                    .value
                    .parse::<u64>()
                    .map_err(|_| err("sqlserver_position"))?;
                v >= min
                    .value
                    .parse::<u64>()
                    .map_err(|_| err("sqlserver_retention"))?
                    && v <= high
                        .value
                        .parse::<u64>()
                        .map_err(|_| err("sqlserver_position"))?
            }
            ChangeMode::Cdc => prior.value >= min.value && prior.value <= high.value,
        };
        if !valid {
            return Err(err("sqlserver_rebaseline_retention_or_future"));
        }
        if prior.value == high.value {
            return Ok(None);
        }
        let mut events = Vec::new();
        let mut total_bytes = 0u64;
        for t in self.tables.values().cloned().collect::<Vec<_>>() {
            let rel = relation(&t.name)?;
            let sql = match high.mode {
                ChangeMode::Ct => {
                    let join = t
                        .keys
                        .iter()
                        .map(|i| Ok(format!("s.{0}=c.{0}", quote(&t.columns[*i].name)?)))
                        .collect::<Result<Vec<_>, SourceError>>()?
                        .join(" AND ");
                    format!(
                        "SELECT TOP ({}) CONVERT(nvarchar(30),c.SYS_CHANGE_VERSION),c.SYS_CHANGE_OPERATION,{},{} FROM CHANGETABLE(CHANGES {rel},{}) c LEFT JOIN {rel} s ON {join} WHERE c.SYS_CHANGE_VERSION<={} ORDER BY c.SYS_CHANGE_VERSION,{}",
                        b.max_chunk_rows + 1,
                        t.key_projection("c")?,
                        t.projection("s")?,
                        prior.value,
                        high.value,
                        t.ordering("c")?
                    )
                }
                ChangeMode::Cdc => {
                    let capture = t
                        .capture
                        .as_deref()
                        .ok_or_else(|| err("sqlserver_cdc_capture"))?;
                    let function = quote(&format!("fn_cdc_get_all_changes_{capture}"))?;
                    format!(
                        "SELECT TOP ({}) CONVERT(varchar(20),s.__$start_lsn,2),CONVERT(nvarchar(4),s.__$operation),{},{} FROM cdc.{function}(sys.fn_cdc_increment_lsn({}),{},N'all update old') s ORDER BY s.__$start_lsn,s.__$seqval,s.__$operation",
                        b.max_chunk_rows + 1,
                        t.key_projection("s")?,
                        t.projection("s")?,
                        lsn(&prior.value)?,
                        lsn(&high.value)?
                    )
                }
            };
            let rows = self.rows(&sql, b.max_chunk_rows + 1).await?;
            if (events.len() + rows.len()) as u64 > b.max_chunk_rows {
                return Err(SourceError::budget("sqlserver_change_transaction_budget"));
            }
            for r in rows {
                let position = text(&r, 0)?;
                let operation = text(&r, 1)?;
                let key = t.keys_from(&r, 2)?;
                let delete = match high.mode {
                    ChangeMode::Ct => operation == "D",
                    ChangeMode::Cdc => operation == "1" || operation == "3",
                };
                let change = if delete {
                    RowChange::Delete {
                        table: t.name.clone(),
                        key,
                    }
                } else {
                    match (high.mode, operation.as_str()) {
                        (ChangeMode::Ct, "I") | (ChangeMode::Cdc, "2") => RowChange::Insert {
                            table: t.name.clone(),
                            key,
                        },
                        (ChangeMode::Ct, "U") | (ChangeMode::Cdc, "4") => RowChange::Update {
                            table: t.name.clone(),
                            key,
                            old_key: None,
                        },
                        _ => return Err(err("sqlserver_change_operation")),
                    }
                };
                total_bytes = total_bytes.saturating_add(
                    serde_json::to_vec(&change)
                        .map_err(|_| err("sqlserver_change_encode"))?
                        .len() as u64,
                );
                if total_bytes > b.max_chunk_bytes {
                    return Err(SourceError::budget("sqlserver_change_bytes"));
                }
                let image = if delete {
                    // Shared durable targets require one image per event, including
                    // deletes; an empty typed batch carries no invented row values.
                    Some(Self::decode(&t, &[], 0)?.0)
                } else {
                    let (image, ipc) = Self::decode(&t, &[r], 2 + t.keys.len())?;
                    total_bytes = total_bytes.saturating_add(ipc.len() as u64);
                    if total_bytes > b.max_chunk_bytes {
                        return Err(SourceError::budget("sqlserver_change_bytes"));
                    }
                    Some(image)
                };
                let order = if high.mode == ChangeMode::Ct {
                    format!(
                        "{:020}",
                        position
                            .parse::<u64>()
                            .map_err(|_| err("sqlserver_position"))?
                    )
                } else {
                    position
                };
                events.push((order, change, image));
            }
        }
        // Stable sort preserves CDC sequence/operation order within each table.
        events.sort_by(|a, b| a.0.cmp(&b.0));
        let mut changes = Vec::new();
        let mut images = Vec::new();
        for (_, change, image) in events {
            changes.push(change);
            if let Some(image) = image {
                images.push(image);
            }
        }
        Ok(Some(ChangeBatch {
            position: high.watermark()?,
            changes,
            images,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table() -> Table {
        Table {
            name: TableRef {
                schema: "dbo".into(),
                name: "fixture".into(),
            },
            columns: vec![
                ColumnSpec {
                    name: "name".into(),
                    kind: SqlKind::Nvarchar,
                    nullable: false,
                    precision: 0,
                    scale: 0,
                    collation: Some("Latin1_General_CI_AS".into()),
                    is_identity: false,
                    sql_type_name: "nvarchar".into(),
                    max_length: Some(100),
                },
                ColumnSpec {
                    name: "id".into(),
                    kind: SqlKind::Int,
                    nullable: false,
                    precision: 10,
                    scale: 0,
                    collation: None,
                    is_identity: false,
                    sql_type_name: "int".into(),
                    max_length: Some(4),
                },
            ],
            keys: vec![0, 1],
            object: 1,
            ct: true,
            capture: None,
        }
    }
    #[test]
    fn identifiers_and_literals_cannot_escape() {
        assert_eq!(quote("x] DROP TABLE y").unwrap(), "[x]] DROP TABLE y]");
        assert_eq!(literal("a'b"), "N'a''b'");
        assert!(quote("x\0").is_err());
        assert!(quote(&"x".repeat(129)).is_err());
    }
    #[test]
    fn composite_bound_keeps_source_collation() {
        let t = table();
        let sql = t
            .bound(&[KeyValue::Text("a'B".into()), KeyValue::Int(8)], true, "s")
            .unwrap();
        assert_eq!(
            sql,
            "((s.[name]>N'a''B') OR (s.[name]=N'a''B' AND s.[id]>=8))"
        );
        assert!(!sql.contains("COLLATE"));
        assert_eq!(t.ordering("s").unwrap(), "s.[name],s.[id]");
    }
    #[test]
    fn bounds_reject_type_and_width() {
        let t = table();
        assert!(t.bound(&[KeyValue::Null], true, "s").is_err());
        assert!(
            t.bound(&[KeyValue::Int(1), KeyValue::Int(2)], false, "s")
                .is_err()
        );
    }
    #[test]
    fn varchar_bound_keeps_source_type_and_code_page() {
        let mut t = table();
        t.columns[0].sql_type_name = "varchar".into();
        let sql = t
            .bound(&[KeyValue::Text("A".into()), KeyValue::Int(1)], true, "s")
            .unwrap();
        assert!(sql.contains("CAST(N'A' COLLATE [Latin1_General_CI_AS] AS varchar(100)) COLLATE [Latin1_General_CI_AS]"));
        assert!(
            t.projection("s")
                .unwrap()
                .contains("CAST(s.[name] AS nvarchar(max))")
        );
    }
    #[test]
    fn receipt_bytes_match_the_delivered_batch_and_pipeline_options() {
        let t = table();
        for rows in [vec![], vec![vec![Cell::Text("key".into()), Cell::Int(1)]]] {
            let package = encode_cells(&t.columns, &rows).unwrap();
            let (batch, receipt_bytes) = canonical_batch(&package.ipc).unwrap();
            let mut sink =
                arrow_ipc::writer::StreamWriter::try_new(Vec::new(), &batch.schema()).unwrap();
            sink.write(&batch).unwrap();
            sink.finish().unwrap();
            assert_eq!(receipt_bytes, sink.into_inner().unwrap());
        }
    }
    #[test]
    fn position_preserves_epoch_and_mode() {
        let p = Position {
            protocol: "oo.sqlserver.position/v1".into(),
            source_epoch: "database:fork".into(),
            mode: ChangeMode::Ct,
            value: "42".into(),
            schema: "schema-hash".into(),
        };
        assert_eq!(Position::parse(&p.watermark().unwrap()).unwrap(), p);
        let mut bad = p;
        bad.value = "42; DROP DATABASE x".into();
        assert!(Position::parse(&bad.watermark().unwrap()).is_err());
        assert!(lsn("0x00").is_err());
    }
    #[test]
    fn budget_fails_closed() {
        let mut b = ReadBudget::default();
        assert!(validate_budget(&b).is_ok());
        b.duty_cycle_percent = 0;
        assert!(validate_budget(&b).is_err());
        b.duty_cycle_percent = 25;
        b.max_chunk_rows = u64::MAX;
        assert!(validate_budget(&b).is_err());
    }
}
