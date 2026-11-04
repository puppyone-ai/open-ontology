//! openGauss is a separate family. Only its MD5-compatible wire path is
//! admitted; PostgreSQL SASL, catalog/version gates and pgoutput are not reused.
//! One engine holds one read-only repeatable-read snapshot. Drop it after a
//! cycle; do not reuse it as a cross-cycle checkpoint.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use arrow_ipc::writer::StreamWriter;
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgIncomingBounds, PgRow, PgSslMode};
use sqlx::{ConnectOptions, Row};
use sqlx_core::sql_str::AssertSqlSafe;

use super::{plan, sqltext};
use crate::engine::*;
use crate::model::{ColumnPrivileges, SourceColumn, classify_type};
use crate::session::{ConnPermit, cell_bool, cell_i64, cell_opt_string, cell_string};
use crate::types::{MapDecision, RunBinding, TypeFacts, TypeGraph, assemble_batch, map_column};
use crate::{PgSourceConfig, SourceError};

pub fn version_rules() -> Vec<VersionRule> {
    [
        (Capability::Catalog, "Separate openGauss 5/6 catalog; MD5 verifier and pg_hba md5 required. SHA256-only and SM3-only authentication need DBA setup: password_encryption_type=1 and reset the reader password."),
        (Capability::ChunkedSnapshot, "Source-compared keyset ranges in one read-only repeatable-read snapshot; no-key tables use a bounded SingleRead. Built-in PG binary types only; unsupported types fail closed."),
        (Capability::DigestDiff, "C5 count plus two order-independent sums of built-in MD5 row hashes; NULL/length framing. Not a cryptographic integrity proof; periodic full reads remain required."),
        (Capability::Watermark, "pg_current_xlog_location / pg_last_xlog_replay_location; not a C1 resume token."),
        (Capability::LogCdc, "Probe wal_level, slots and decoding functions read-only; openGauss decoder is not PostgreSQL pgoutput. No ChangeSource implementation."),
    ].into_iter().map(|(capability, note)| VersionRule {
        family: EngineFamily::Opengauss,
        versions: VersionRange::new((5, 0, 0), Some((7, 0, 0))),
        capability,
        note: note.into(),
    }).collect()
}

fn auth_setup(observed: &str, user: &str) -> CapabilityState {
    let user = quote(user);
    CapabilityState::NeedsSetup {
        requirements: vec![
            Requirement { kind: RequirementKind::Parameter, name: "compatible_password_storage".into(),
                observed: Some(observed.into()), required: "MD5-compatible reader verifier".into(),
                dba_statement: Some("ALTER SYSTEM SET password_encryption_type = 1; SELECT pg_reload_conf();".into()) },
            Requirement { kind: RequirementKind::Feature, name: "reader_password_reset".into(),
                observed: None, required: "Reset the reader password after changing storage mode using a secure DBA client".into(),
                dba_statement: Some(format!("ALTER ROLE {user} PASSWORD '<new-reader-password>';")) },
            Requirement { kind: RequirementKind::Parameter, name: "reader_hba".into(),
                observed: None, required: "Put this rule before conflicting rules, restrict the CIDR, then reload".into(),
                dba_statement: Some(format!("hostssl all {user} <reader-CIDR> md5")) },
        ],
    }
}

fn auth_method(error: &sqlx::Error) -> Option<&'static str> {
    let text = error.to_string();
    if text.contains("unsupported SASL authentication mechanisms")
        || text.contains("reading Authentication")
        || text.contains("unsupported authentication method: Sasl")
    {
        Some("openGauss SHA256 or incompatible authentication challenge")
    } else if text.contains("unknown authentication method: 13") {
        Some("openGauss SM3 challenge (code 13)")
    } else {
        None
    }
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

#[derive(Clone)]
struct Table {
    oid: i64,
    columns: Vec<SourceColumn>,
    graph: TypeGraph,
    keys: Vec<sqltext::KeyCol>,
    shape: KeyShape,
}

#[derive(Clone)]
struct Planned {
    table: Table,
    plan: TableChunkPlan,
}

pub struct OpenGaussEngine {
    config: PgSourceConfig,
    conn: Option<PgConnection>,
    permit: Option<ConnPermit>,
    plans: BTreeMap<TableRef, Planned>,
    revision: u32,
}

impl OpenGaussEngine {
    pub fn new(config: PgSourceConfig) -> Self {
        Self {
            config,
            conn: None,
            permit: None,
            plans: BTreeMap::new(),
            revision: 0,
        }
    }

    fn selected(&self, table: &TableRef) -> Result<(), SourceError> {
        if self
            .config
            .selection()
            .tables()
            .iter()
            .any(|t| t.schema() == table.schema && t.table() == table.name)
        {
            Ok(())
        } else {
            Err(SourceError::config("unselected_table"))
        }
    }

    async fn open(&mut self) -> Result<(), sqlx::Error> {
        if self.conn.is_some() {
            return Ok(());
        }
        let mut permits = crate::session::try_reserve(self.config.source_id().as_str(), 1)
            .map_err(|_| sqlx::Error::Protocol("source connection budget".into()))?;
        let options = PgConnectOptions::from_explicit()
            .host(self.config.endpoint().host())
            .port(self.config.endpoint().port())
            .database(self.config.endpoint().database())
            .username(self.config.credentials().username())
            .password(self.config.credentials().password())
            .ssl_mode(PgSslMode::VerifyFull)
            .ssl_root_cert_from_pem(self.config.tls().ca_pem().to_vec())
            .application_name("oo-opengauss")
            .extra_float_digits(2i8)
            .disable_statement_logging();
        let mut conn =
            tokio::time::timeout(self.config.budget().connect_timeout(), options.connect())
                .await
                .map_err(|_| sqlx::Error::Protocol("connect timeout".into()))??;
        // Bound metadata and row messages before accepting any server query.
        conn.set_incoming_bounds(Some(PgIncomingBounds::try_new(
            32 * 1024 * 1024 + 8192,
            32 * 1024 * 1024,
            32 * 1024 * 1024,
            1600,
        )?))?;
        for sql in [
            "SET default_transaction_read_only = on",
            "SET search_path = pg_catalog",
            "SET timezone = 'UTC'",
            "SET datestyle = 'ISO, YMD'",
            "SET bytea_output = 'hex'",
        ] {
            sqlx::query(AssertSqlSafe(sql.to_owned()))
                .execute(&mut conn)
                .await?;
        }
        sqlx::query("SELECT set_config('statement_timeout',$1,false)")
            .bind(
                self.config
                    .budget()
                    .statement_timeout()
                    .as_millis()
                    .to_string(),
            )
            .execute(&mut conn)
            .await?;
        let raw: String = sqlx::query("SELECT version()")
            .fetch_one(&mut conn)
            .await?
            .try_get(0)?;
        let version = EngineVersion::parse(EngineFamily::Opengauss, &raw)
            .map_err(|_| sqlx::Error::Protocol("unsupported openGauss version".into()))?;
        if !raw.to_ascii_lowercase().contains("opengauss") || !(5..=6).contains(&version.major) {
            return Err(sqlx::Error::Protocol(
                "unsupported openGauss version".into(),
            ));
        }
        sqlx::query("START TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut conn)
            .await?;
        self.permit = permits.pop();
        self.conn = Some(conn);
        Ok(())
    }

    async fn query(
        &mut self,
        sql: String,
        params: Vec<String>,
        cap: usize,
    ) -> Result<Vec<PgRow>, SourceError> {
        self.open().await.map_err(crate::error::map_connect)?;
        let mut q = sqlx::query(AssertSqlSafe(sql));
        for p in params {
            q = q.bind(p);
        }
        let result = tokio::time::timeout(self.config.budget().statement_timeout(), async {
            let mut stream = q.fetch(self.conn.as_mut().unwrap());
            let mut rows = Vec::new();
            while let Some(row) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
                if rows.len() == cap {
                    return Err(SourceError::budget("row_limit"));
                }
                rows.push(row.map_err(crate::error::map_sqlx)?);
            }
            Ok(rows)
        })
        .await
        .map_err(|_| SourceError::budget("client_timeout"))
        .and_then(|r| r);
        if result.is_err() {
            // A timed-out stream or aborted transaction must never be reused.
            self.conn.take();
            self.permit.take();
            self.plans.clear();
        }
        result
    }

    async fn guard_budget(&mut self, budget: &ReadBudget) -> Result<(), SourceError> {
        super::validate_budget(budget)?;
        let timeout = Duration::from_millis(budget.statement_timeout_ms)
            .min(self.config.budget().statement_timeout());
        self.query(
            "SELECT set_config('statement_timeout',$1,false)".into(),
            vec![timeout.as_millis().to_string()],
            1,
        )
        .await?;
        let rows=self.query("SELECT count(*)::bigint,current_setting('max_connections')::bigint FROM pg_stat_activity".into(),vec![],1).await?;
        if cell_i64(&rows[0], 0)? + 5 >= cell_i64(&rows[0], 1)? {
            return Err(SourceError::budget("connection_reserve"));
        }
        Ok(())
    }

    async fn metadata(&mut self, table: &TableRef) -> Result<Table, SourceError> {
        self.selected(table)?;
        let rows = self.query(
            "SELECT c.oid::bigint,c.relkind::text,EXISTS(SELECT 1 FROM pg_rlspolicy WHERE polrelid=c.oid),has_table_privilege(c.oid,'SELECT'), \
             EXISTS(SELECT 1 FROM pg_inherits WHERE inhparent=c.oid) \
             FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
             WHERE n.nspname=$1 AND c.relname=$2".into(),
            vec![table.schema.clone(),table.name.clone()], 1).await?;
        let r = rows
            .first()
            .ok_or_else(|| SourceError::catalog("table_missing"))?;
        if cell_string(r, 1)? != "r" || cell_bool(r, 4)? {
            return Err(SourceError::catalog("unsupported_relation"));
        }
        if cell_bool(r, 2)? || !cell_bool(r, 3)? {
            return Err(SourceError::privilege("table_select_or_rls"));
        }
        let oid = cell_i64(r, 0)?;
        let rows = self.query(
            "SELECT a.attnum::bigint,a.attname::text,a.atttypid::bigint,a.atttypmod::bigint,a.attnotnull, \
             t.typname::text,n.nspname::text,t.typtype::text,t.typlen::bigint,t.typelem::bigint,t.typbasetype::bigint, \
             t.typtypmod::bigint,t.typsend::bigint,p.proname::text,pn.nspname::text, \
             a.attcollation::bigint,co.collname::text,format_type(a.atttypid,a.atttypmod),t.typcategory::text \
             FROM pg_attribute a JOIN pg_type t ON t.oid=a.atttypid JOIN pg_namespace n ON n.oid=t.typnamespace \
             LEFT JOIN pg_proc p ON p.oid=t.typsend LEFT JOIN pg_namespace pn ON pn.oid=p.pronamespace \
             LEFT JOIN pg_collation co ON co.oid=a.attcollation \
             WHERE a.attrelid=$1::bigint::oid AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum".into(),
            vec![oid.to_string()], 1600).await?;
        let mut columns = Vec::new();
        let mut facts = Vec::new();
        for r in &rows {
            let name = cell_string(r, 1)?;
            let typname = cell_string(r, 5)?;
            let typtype = cell_string(r, 7)?;
            let typmod = cell_i64(r, 3)? as i32;
            facts.push(TypeFacts {
                oid: cell_i64(r, 2)?,
                name: typname.clone(),
                schema: cell_string(r, 6)?,
                typtype: typtype.clone(),
                typlen: cell_i64(r, 8)? as i32,
                typelem: cell_i64(r, 9)?,
                typbasetype: cell_i64(r, 10)?,
                typtypmod: cell_i64(r, 11)? as i32,
                typsend_oid: cell_i64(r, 12)?,
                typsend_name: cell_opt_string(r, 13)?.unwrap_or_default(),
                typsend_schema: cell_opt_string(r, 14)?.unwrap_or_default(),
            });
            columns.push(SourceColumn {
                attnum: cell_i64(r, 0)? as i32,
                name_ident: name.clone(),
                name,
                type_oid: cell_i64(r, 2)?,
                type_schema: cell_string(r, 6)?,
                type_name: typname.clone(),
                typtype: typtype.clone(),
                typmod,
                base_type_oid: None,
                numeric_precision: None,
                numeric_scale: None,
                not_null: cell_bool(r, 4)?,
                collation_oid: cell_i64(r, 15)?,
                collation_name: cell_opt_string(r, 16)?,
                identity: String::new(),
                generated: String::new(),
                default_expr: None,
                comment: None,
                enum_labels: Vec::new(),
                domain_checks: Vec::new(),
                type_support: classify_type(&typtype, &typname, typmod),
                current_privileges: ColumnPrivileges {
                    select: Some(true),
                    insert: None,
                    update: None,
                },
            });
        }
        if columns.is_empty() {
            return Err(SourceError::catalog("empty_columns"));
        }
        // indkey subscripts work on the fork's older catalog; no LATERAL,
        // WITH ORDINALITY or PostgreSQL-only indnkeyatts is assumed.
        let indices = self.query(
            "SELECT ic.relname::text,i.indisprimary,(i.indpred IS NULL AND i.indexprs IS NULL), \
             i.indkey::text,i.indcollation::text FROM pg_index i JOIN pg_class ic ON ic.oid=i.indexrelid \
             JOIN pg_am am ON am.oid=ic.relam WHERE i.indrelid=$1::bigint::oid AND i.indisunique \
             AND i.indisvalid AND i.indisready AND am.amname='btree' ORDER BY i.indisprimary DESC,ic.relname".into(),
            vec![oid.to_string()], 64).await?;
        let mut candidates = Vec::new();
        for r in indices {
            let nums: Vec<i32> = cell_string(&r, 3)?
                .split_whitespace()
                .map(|v| v.parse().map_err(|_| SourceError::catalog("index_key")))
                .collect::<Result<_, _>>()?;
            let collations: Vec<i64> = cell_string(&r, 4)?
                .split_whitespace()
                .map(|v| {
                    v.parse()
                        .map_err(|_| SourceError::catalog("index_collation"))
                })
                .collect::<Result<_, _>>()?;
            let mut candidate = plan::IndexCandidate {
                name: cell_string(&r, 0)?,
                primary: cell_bool(&r, 1)?,
                plain: cell_bool(&r, 2)?,
                columns: Vec::new(),
            };
            for (pos, num) in nums.iter().enumerate() {
                let Some(index) = columns.iter().position(|c| c.attnum == *num) else {
                    candidate.plain = false;
                    continue;
                };
                let c = &columns[index];
                if collations.get(pos) != Some(&c.collation_oid) {
                    candidate.plain = false;
                }
                candidate.columns.push(plan::IndexCol {
                    name: c.name.clone(),
                    not_null: c.not_null,
                    cast_type: cell_string(&rows[index], 17)?,
                    base_oid: c.type_oid,
                    category: cell_string(&rows[index], 18)?,
                    collation: c.collation_name.clone(),
                });
            }
            candidates.push(candidate);
        }
        let (shape, index) = plan::choose_key(&candidates);
        let mut keys = Vec::new();
        for c in index.into_iter().flat_map(|i| i.columns) {
            if !sqltext::valid_cast_type(&c.cast_type) {
                return Err(SourceError::catalog("cast_type"));
            }
            keys.push(sqltext::KeyCol {
                quoted: quote(&c.name),
                name: c.name,
                cast_type: c.cast_type,
                class: sqltext::KeyClass::from_oid(c.base_oid),
                collation: c.collation,
            });
        }
        Ok(Table {
            oid,
            columns,
            graph: TypeGraph::new(facts),
            keys,
            shape,
        })
    }

    pub async fn catalog(&mut self, table: &TableRef) -> Result<Vec<SourceColumn>, SourceError> {
        Ok(self.metadata(table).await?.columns)
    }

    /// Four-layer C5 policy is shared; digests remain family-specific.
    pub async fn inspect_c5(
        &mut self,
        table: &TableRef,
        previous: &[RangeDigest],
        cycle: &super::reconcile::C5Cycle,
    ) -> Result<super::reconcile::C5Inspection, SourceError> {
        let plan = self
            .plans
            .get(table)
            .ok_or_else(|| SourceError::protocol("unknown_plan"))?
            .plan
            .clone();
        super::reconcile::inspect(self, &plan, previous, cycle).await
    }

    /// Keep this engine alive while an independent reader imports the snapshot.
    pub async fn export_snapshot(&mut self) -> Result<String, SourceError> {
        let rows = self
            .query("SELECT pg_export_snapshot()::text".into(), vec![], 1)
            .await?;
        let snapshot = cell_string(&rows[0], 0)?;
        if !super::valid_snapshot(&snapshot) {
            return Err(SourceError::protocol("snapshot"));
        }
        Ok(snapshot)
    }

    fn relation(table: &TableRef) -> String {
        format!("{}.{}", quote(&table.schema), quote(&table.name))
    }

    async fn planned(&mut self, table: &TableRef) -> Result<Planned, SourceError> {
        self.selected(table)?;
        let p = self
            .plans
            .get(table)
            .ok_or_else(|| SourceError::protocol("unknown_plan"))?
            .clone();
        let current = self.metadata(table).await?;
        if current.oid != p.table.oid
            || current.columns != p.table.columns
            || current.keys != p.table.keys
        {
            return Err(SourceError::catalog("schema_drift"));
        }
        Ok(p)
    }

    fn predicate(
        table: &Table,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<(String, Vec<String>), SourceError> {
        let mut values = Vec::new();
        for bound in [lo, hi].into_iter().flatten() {
            if table.keys.is_empty() || bound.len() != table.keys.len() {
                return Err(SourceError::protocol("bound_width"));
            }
            for value in bound {
                values.push(sqltext::bind_text(value)?);
            }
        }
        Ok((
            sqltext::range_where(
                &table.keys,
                sqltext::CompareForm::Row,
                lo.is_some(),
                hi.is_some(),
            )
            .0,
            values,
        ))
    }
}

impl SourceEngine for OpenGaussEngine {
    fn family(&self) -> EngineFamily {
        EngineFamily::Opengauss
    }

    async fn probe(&mut self) -> Result<ProbeReport, SourceError> {
        if let Err(error) = self.open().await {
            let Some(method) = auth_method(&error) else {
                return Err(crate::error::map_connect(error));
            };
            return Ok(ProbeReport {
                protocol: ENGINE_PROTOCOL.into(),
                family: self.family(),
                version: EngineVersion {
                    family: self.family(),
                    major: 0,
                    minor: 0,
                    patch: 0,
                    raw: "unmeasured: authentication refused".into(),
                },
                capabilities: version_rules()
                    .iter()
                    .map(|r| {
                        (
                            r.capability,
                            auth_setup(method, self.config.credentials().username()),
                        )
                    })
                    .collect(),
                settings: BTreeMap::from([("version_observed".into(), "false".into())]),
            });
        }
        let rows = self.query(
            "SELECT version(),current_setting('password_encryption_type'),current_setting('wal_level'), \
             current_setting('max_replication_slots'), \
             EXISTS(SELECT 1 FROM pg_proc WHERE proname='pg_create_logical_replication_slot'), \
             EXISTS(SELECT 1 FROM pg_proc WHERE proname='pg_logical_slot_get_changes')".into(),vec![],1).await?;
        let r = &rows[0];
        let raw = cell_string(r, 0)?;
        if !raw.to_ascii_lowercase().contains("opengauss") {
            return Err(SourceError::version("not_opengauss"));
        }
        let version = EngineVersion::parse(self.family(), &raw)?;
        let mut capabilities = BTreeMap::new();
        for rule in version_rules() {
            capabilities.insert(
                rule.capability,
                if rule.versions.contains(&version) {
                    CapabilityState::Available
                } else {
                    CapabilityState::Unsupported {
                        reason: "Only openGauss 5.x/6.x are admitted".into(),
                    }
                },
            );
        }
        capabilities.insert(Capability::LogCdc,CapabilityState::Unsupported {
            reason: format!("openGauss decoding requires a separate consumer, not pgoutput; observed wal_level={}, max_replication_slots={}, create_slot={}, get_changes={}",
                cell_string(r,2)?,cell_string(r,3)?,cell_bool(r,4)?,cell_bool(r,5)?),
        });
        let mut settings = BTreeMap::from([
            ("password_encryption_type".into(), cell_string(r, 1)?),
            ("wal_level".into(), cell_string(r, 2)?),
            ("max_replication_slots".into(), cell_string(r, 3)?),
            ("support_level".into(), "experimental".into()),
            (
                "snapshot_scope".into(),
                "one engine instance per cycle".into(),
            ),
            (
                "authentication".into(),
                "existing reader authenticated via PG driver".into(),
            ),
        ]);
        for table in self.config.selection().tables().to_vec() {
            let table = TableRef {
                schema: table.schema().into(),
                name: table.table().into(),
            };
            // A missing SELECT is a setup report, not a fabricated empty catalog.
            let grants = self
                .query(
                    "SELECT has_schema_privilege($1,'USAGE'),has_table_privilege($2,'SELECT')"
                        .into(),
                    vec![table.schema.clone(), Self::relation(&table)],
                    1,
                )
                .await?;
            if !cell_bool(&grants[0], 0)? || !cell_bool(&grants[0], 1)? {
                let state = CapabilityState::NeedsSetup {
                    requirements: vec![Requirement {
                        kind: RequirementKind::Privilege,
                        name: format!("select:{table}"),
                        observed: Some("denied".into()),
                        required: "USAGE on schema and SELECT on table".into(),
                        dba_statement: Some(format!(
                            "GRANT USAGE ON SCHEMA {} TO {}; GRANT SELECT ON {} TO {};",
                            quote(&table.schema),
                            quote(self.config.credentials().username()),
                            Self::relation(&table),
                            quote(self.config.credentials().username())
                        )),
                    }],
                };
                for cap in [
                    Capability::Catalog,
                    Capability::ChunkedSnapshot,
                    Capability::DigestDiff,
                ] {
                    capabilities.insert(cap, state.clone());
                }
            } else {
                let metadata = self.metadata(&table).await?;
                settings.insert(
                    format!("columns:{table}"),
                    metadata.columns.len().to_string(),
                );
            }
        }
        Ok(ProbeReport {
            protocol: ENGINE_PROTOCOL.into(),
            family: self.family(),
            version,
            capabilities,
            settings,
        })
    }

    async fn plan_chunks(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<TableChunkPlan, SourceError> {
        super::validate_budget(budget)?;
        self.guard_budget(budget).await?;
        let start = Instant::now();
        let metadata = self.metadata(table).await?;
        let mut bounds = Vec::new();
        let mut lo: Option<Vec<KeyValue>> = None;
        if !metadata.keys.is_empty() {
            loop {
                let sql = sqltext::boundary_probe(
                    &Self::relation(table),
                    &metadata.keys,
                    sqltext::CompareForm::Row,
                    lo.is_some(),
                );
                let mut params = match &lo {
                    Some(v) => v
                        .iter()
                        .map(sqltext::bind_text)
                        .collect::<Result<Vec<_>, _>>()?,
                    None => vec![],
                };
                params.push(plan::target_rows(budget.max_chunk_rows).to_string());
                // SQLx binds strings; the OFFSET operand needs an explicit cast.
                let sql = sql.replace(
                    &format!("OFFSET ${}", params.len()),
                    &format!("OFFSET ${}::bigint", params.len()),
                );
                let rows = self.query(sql, params, 1).await?;
                let Some(row) = rows.first() else {
                    break;
                };
                let values = metadata
                    .keys
                    .iter()
                    .enumerate()
                    .map(|(i, k)| k.class.value(cell_opt_string(row, i)?))
                    .collect::<Result<Vec<_>, _>>()?;
                if lo.as_ref() == Some(&values) {
                    return Err(SourceError::protocol("key_progress"));
                }
                lo = Some(values.clone());
                bounds.push(values);
                if bounds.len() >= 100_000 {
                    return Err(SourceError::budget("chunk_count"));
                }
            }
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| SourceError::budget("plan_revision"))?;
        let plan = TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table: table.clone(),
            key: metadata.shape.clone(),
            strategy: if metadata.keys.is_empty() {
                ChunkStrategy::SingleRead
            } else {
                ChunkStrategy::Keyset {
                    columns: metadata.keys.iter().map(|k| k.name.clone()).collect(),
                }
            },
            estimated_rows: None,
            chunks: plan::keyset_chunks(table, self.revision, bounds, None)?,
        };
        self.plans.insert(
            table.clone(),
            Planned {
                table: metadata,
                plan: plan.clone(),
            },
        );
        super::duty(start, budget.duty_cycle_percent).await;
        Ok(plan)
    }

    async fn read_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt, SourceError> {
        super::validate_budget(budget)?;
        self.guard_budget(budget).await?;
        let start = Instant::now();
        let p = self.planned(&chunk.table).await?;
        if !p.plan.chunks.contains(chunk) {
            return Err(SourceError::protocol("stale_chunk"));
        }
        let low = self.watermark().await?;
        let keys: Vec<i32> = p
            .table
            .columns
            .iter()
            .filter(|c| p.table.keys.iter().any(|k| k.name == c.name))
            .map(|c| c.attnum)
            .collect();
        let plans = p
            .table
            .columns
            .iter()
            .map(|c| map_column(c, &p.table.graph, keys.contains(&c.attnum)))
            .collect::<Vec<_>>();
        if plans
            .iter()
            .any(|p| p.mapping.decision != MapDecision::Lossless)
        {
            return Err(SourceError::protocol("unsupported_column"));
        }
        let (predicate, params) =
            Self::predicate(&p.table, chunk.lo.as_deref(), chunk.hi.as_deref())?;
        let cols = p
            .table
            .columns
            .iter()
            .map(|c| quote(&c.name))
            .collect::<Vec<_>>();
        let sql = sqltext::chunk_select(
            &Self::relation(&chunk.table),
            &cols,
            &predicate,
            &p.table.keys,
        );
        let mut q = sqlx::query(AssertSqlSafe(sql));
        for value in params {
            q = q.bind(value);
        }
        let binding = RunBinding {
            source_id: self.config.source_id().as_str().into(),
            schema: chunk.table.schema.clone(),
            table: chunk.table.name.clone(),
            run_generation: u64::from(chunk.plan_revision),
            input_digest: Sha256::digest(b"opengauss/v1").into(),
        };
        let deadline = Duration::from_millis(budget.statement_timeout_ms)
            .min(self.config.budget().statement_timeout());
        let result = tokio::time::timeout(deadline, async {
            let mut stream = q.fetch(self.conn.as_mut().unwrap());
            let mut page = Vec::new();
            let mut rows = 0u64;
            let mut payload = 0u64;
            let mut writer = None;
            while let Some(row) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
                let row = row.map_err(crate::error::map_sqlx)?;
                rows += 1;
                if rows > budget.max_chunk_rows {
                    return Err(SourceError::budget("chunk_rows_replan"));
                }
                let (cells, bytes) = crate::bounded_read::decode_row(&plans, &row)?;
                payload = payload.saturating_add(bytes);
                if payload > budget.max_chunk_bytes {
                    return Err(SourceError::budget("chunk_bytes_replan"));
                }
                page.push(cells);
                if page.len() == 1024 {
                    flush(
                        &binding,
                        &plans,
                        &keys,
                        &mut page,
                        &mut writer,
                        budget.max_chunk_bytes,
                        sink,
                    )?;
                }
            }
            if !page.is_empty() || rows == 0 {
                flush(
                    &binding,
                    &plans,
                    &keys,
                    &mut page,
                    &mut writer,
                    budget.max_chunk_bytes,
                    sink,
                )?;
            }
            let mut writer = writer.ok_or_else(|| SourceError::protocol("arrow_ipc"))?;
            writer
                .finish()
                .map_err(|_| SourceError::budget("chunk_bytes_replan"))?;
            let bytes = writer
                .into_inner()
                .map_err(|_| SourceError::protocol("arrow_ipc"))?;
            if bytes.len() as u64 > budget.max_chunk_bytes {
                return Err(SourceError::budget("chunk_bytes_replan"));
            }
            Ok((rows, bytes))
        })
        .await
        .map_err(|_| SourceError::budget("client_timeout"))
        .and_then(|r| r);
        let (rows, bytes) = match result {
            Ok(r) => r,
            Err(e) => {
                self.conn.take();
                self.permit.take();
                self.plans.clear();
                return Err(e);
            }
        };
        let receipt = ChunkReceipt {
            table: chunk.table.clone(),
            plan_revision: chunk.plan_revision,
            chunk_id: chunk.chunk_id,
            rows,
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            low,
            high: self.watermark().await?,
        };
        super::duty(start, budget.duty_cycle_percent).await;
        Ok(receipt)
    }

    async fn watermark(&mut self) -> Result<Option<Watermark>, SourceError> {
        let rows=self.query("SELECT CASE WHEN pg_is_in_recovery() THEN pg_last_xlog_replay_location()::text ELSE pg_current_xlog_location()::text END".into(),vec![],1).await?;
        cell_opt_string(&rows[0], 0)?
            .map(|v| super::lsn::parse_lsn(&v).map(|lsn| Watermark::PgLsn { lsn }))
            .transpose()
    }

    async fn digest_range(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest, SourceError> {
        let start = Instant::now();
        let p = self.planned(table).await?;
        let (predicate, params) = Self::predicate(&p.table, lo, hi)?;
        let cols = p
            .table
            .columns
            .iter()
            .map(|c| quote(&c.name))
            .collect::<Vec<_>>();
        let sql = format!(
            "SELECT count(*)::bigint,COALESCE(sum(('x'||substr(d,1,16))::bit(64)::bigint::numeric),0)::text, \
            COALESCE(sum(('x'||substr(d,17,16))::bit(64)::bigint::numeric),0)::text \
            FROM (SELECT md5({}) AS d FROM {}{}) oo_rows",
            sqltext::row_text(&cols),
            Self::relation(table),
            predicate
        );
        let rows = self.query(sql, params, 1).await?;
        let r = &rows[0];
        let result = RangeDigest {
            table: table.clone(),
            lo: lo.map(<[_]>::to_vec),
            hi: hi.map(<[_]>::to_vec),
            rows: cell_i64(r, 0)? as u64,
            digest: format!(
                "opengauss-text-md5-sum128/v1:{}:{}",
                cell_string(r, 1)?,
                cell_string(r, 2)?
            ),
        };
        super::duty(start, 25).await;
        Ok(result)
    }
}

fn flush(
    binding: &RunBinding,
    plans: &[crate::types::ColumnPlan],
    keys: &[i32],
    page: &mut Vec<Vec<crate::types::DecodedCell>>,
    writer: &mut Option<StreamWriter<Vec<u8>>>,
    max_bytes: u64,
    sink: &mut dyn ChunkSink,
) -> Result<(), SourceError> {
    let typed = assemble_batch(binding, plans.to_vec(), keys, std::mem::take(page))?;
    if !typed.rejects.is_empty() {
        return Err(SourceError::protocol("rejected_cell"));
    }
    let batch = typed.record_batch()?;
    if writer.is_none() {
        *writer = Some(
            StreamWriter::try_new(Vec::new(), &batch.schema())
                .map_err(|_| SourceError::protocol("arrow_ipc"))?,
        );
    }
    let writer = writer.as_mut().unwrap();
    writer
        .write(&batch)
        .map_err(|_| SourceError::protocol("arrow_ipc"))?;
    if writer.get_ref().len() as u64 > max_bytes {
        return Err(SourceError::budget("chunk_bytes_replan"));
    }
    sink.push(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dispatch_and_auth_setup_are_family_specific() {
        for major in [5, 6] {
            let v =
                EngineVersion::parse(EngineFamily::Opengauss, &format!("openGauss {major}.0.0"))
                    .unwrap();
            assert_eq!(
                VersionRule::applicable(&version_rules(), EngineFamily::Opengauss, &v).count(),
                5
            );
        }
        for error in [
            "unsupported SASL authentication mechanisms",
            "unknown authentication method: 13",
        ] {
            assert!(auth_method(&sqlx::Error::Protocol(error.into())).is_some());
        }
        assert!(auth_method(&sqlx::Error::Protocol("connection reset".into())).is_none());
        let CapabilityState::NeedsSetup { requirements } = auth_setup("sha256", "reader") else {
            panic!()
        };
        assert_eq!(requirements.len(), 3);
        assert!(
            requirements[0]
                .dba_statement
                .as_ref()
                .unwrap()
                .contains("password_encryption_type = 1")
        );
        assert_eq!(quote("a\"b"), "\"a\"\"b\"");
    }
}
