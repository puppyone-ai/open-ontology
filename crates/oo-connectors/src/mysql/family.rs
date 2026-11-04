//! Shared wire transport and snapshot mechanics, not inherited capabilities.
use super::*;
use crate::engine::{Capability, CapabilityState, Requirement, RequirementKind};
use crate::mysql::{mariadb, oceanbase, tidb};

impl MysqlEngine {
    pub(super) async fn tidb_load(&mut self) -> Result<Vec<Row>> {
        let grants = self.exec("SHOW GRANTS FOR CURRENT_USER()", vec![]).await?;
        if !grants
            .iter()
            .filter_map(|r| row_text(r, 0))
            .any(|g| has_process(&g))
        {
            return Err(SourceError::privilege(
                "tidb_process_required_for_load_budget",
            ));
        }
        self.exec("SELECT 'Threads_connected', COUNT(*) FROM information_schema.PROCESSLIST UNION ALL SELECT 'Threads_running', COUNT(*) FROM information_schema.PROCESSLIST WHERE COMMAND <> 'Sleep'", vec![]).await
    }
    pub(super) async fn tidb_verify_index(
        &mut self,
        sql: &str,
        values: Vec<Value>,
        index: &str,
    ) -> Result<()> {
        let rows = self
            .exec(&format!("EXPLAIN FORMAT='brief' {sql}"), values)
            .await?;
        self.stats.explain_checks += 1;
        let ok = rows.iter().any(|row| {
            let id = row_text(row, 0).unwrap_or_default();
            let access = named(row, "access object")
                .and_then(|i| row_text(row, i))
                .unwrap_or_default();
            (index == "PRIMARY"
                && (id.contains("TableRangeScan")
                    || id.contains("TableFullScan")
                    || id.contains("Point_Get")))
                || access.contains(&format!("index:{index}("))
        });
        if ok {
            Ok(())
        } else {
            Err(SourceError::catalog("chunk_index_unused"))
        }
    }
    pub(crate) fn new_family(
        family: EngineFamily,
        config: MysqlConfig,
        password: Zeroizing<String>,
        options: EngineOptions,
    ) -> Self {
        let mut engine = Self::new(config, password, options);
        engine.family = family;
        engine
    }

    pub(super) fn timeout_setting(&self) -> String {
        let ms = self.options.budget.statement_timeout_ms.clamp(1, 600_000);
        match self.family {
            EngineFamily::Mariadb => format!(
                "SET SESSION max_statement_time = {}.{:03}",
                ms / 1000,
                ms % 1000
            ),
            EngineFamily::Oceanbase => format!("SET SESSION ob_query_timeout = {}", ms * 1000),
            _ => format!("SET SESSION max_execution_time = {ms}"),
        }
    }

    pub(super) async fn family_basics(&mut self) -> Result<()> {
        let sql = if self.family == EngineFamily::Oceanbase {
            "SELECT OB_VERSION(), @@version_comment, @@GLOBAL.max_connections"
        } else {
            "SELECT VERSION(), @@version_comment, @@GLOBAL.max_connections"
        };
        let rows = self.exec(sql, vec![]).await?;
        let row = rows
            .first()
            .ok_or_else(|| SourceError::protocol("version"))?;
        let raw = row_text(row, 0).ok_or_else(|| SourceError::version("version_text"))?;
        let comment = row_text(row, 1).unwrap_or_default();
        let version = match self.family {
            EngineFamily::Mariadb => mariadb::parse_version(&raw)?,
            EngineFamily::Tidb => tidb::parse_version(&raw)?,
            EngineFamily::Oceanbase => oceanbase::parse_version(&raw, &comment)?,
            _ => return Err(SourceError::version("family_dispatch")),
        };
        let dialect = match self.family {
            EngineFamily::Mariadb => mariadb::dialect(),
            EngineFamily::Tidb => tidb::dialect(),
            EngineFamily::Oceanbase => oceanbase::dialect(),
            _ => unreachable!(),
        };
        if self.family == EngineFamily::Oceanbase {
            let mode = self.exec("SELECT @@ob_compatibility_mode", vec![]).await?;
            if mode.first().and_then(|r| row_text(r, 0)).as_deref() != Some("MYSQL") {
                return Err(SourceError::version("oceanbase_mysql_mode_required"));
            }
        }
        self.basics = Some(Basics {
            version,
            dialect,
            watermark: WatermarkMode::None,
            max_connections: row_u64(row, 2).map(|n| {
                if self.family == EngineFamily::Tidb && n == 0 {
                    u64::MAX
                } else {
                    n
                }
            }),
        });
        Ok(())
    }

    pub(super) async fn family_probe(&mut self) -> Result<ProbeReport> {
        self.mark_op();
        self.rest().await;
        let started = Instant::now();
        let result = self.family_observe().await;
        self.finish_op(started);
        result
    }

    async fn family_observe(&mut self) -> Result<ProbeReport> {
        let grants = self.exec("SHOW GRANTS FOR CURRENT_USER()", vec![]).await?;
        let lines: Vec<String> = grants.iter().filter_map(|r| row_text(r, 0)).collect();
        let grants = Grants::parse(&lines, &self.config.mysql_database);
        let account = self.exec("SELECT CURRENT_USER()", vec![]).await?;
        let account = account
            .first()
            .and_then(|r| row_text(r, 0))
            .map(|s| capability::quote_account(&s));
        let mut report = match self.family {
            EngineFamily::Mariadb => {
                mariadb::report(self.basics.as_ref().expect("basics").version.clone())
            }
            EngineFamily::Tidb => {
                tidb::report(self.basics.as_ref().expect("basics").version.clone())
            }
            EngineFamily::Oceanbase => {
                oceanbase::report(self.basics.as_ref().expect("basics").version.clone())
            }
            _ => unreachable!(),
        };
        if self.family == EngineFamily::Tidb && !lines.iter().any(|g| has_process(g)) {
            for capability in [Capability::ChunkedSnapshot, Capability::DigestDiff] {
                report.capabilities.insert(
                    capability,
                    CapabilityState::NeedsSetup {
                        requirements: vec![Requirement {
                            kind: RequirementKind::Privilege,
                            name: "PROCESS".into(),
                            observed: Some("missing".into()),
                            required: "PROCESS to measure all local TiDB sessions before reads"
                                .into(),
                            dba_statement: account
                                .as_ref()
                                .map(|a| format!("GRANT PROCESS ON *.* TO {a};")),
                        }],
                    },
                );
            }
        }
        if !grants.select {
            for capability in [
                Capability::Catalog,
                Capability::ChunkedSnapshot,
                Capability::DigestDiff,
            ] {
                report.capabilities.insert(
                    capability,
                    CapabilityState::NeedsSetup {
                        requirements: vec![Requirement {
                            kind: RequirementKind::Privilege,
                            name: "SELECT".into(),
                            observed: Some("missing schema grant".into()),
                            required: "SELECT on the selected schema".into(),
                            dba_statement: account.as_ref().map(|a| {
                                format!(
                                    "GRANT SELECT ON `{}`.* TO {a};",
                                    self.config.mysql_database
                                )
                            }),
                        }],
                    },
                );
            }
        }
        if self.family == EngineFamily::Mariadb {
            let rows = self.exec("SELECT @@GLOBAL.gtid_binlog_pos, @@GLOBAL.log_bin, @@GLOBAL.binlog_format, @@GLOBAL.binlog_row_image", vec![]).await?;
            if let Some(row) = rows.first() {
                for (i, name) in [
                    "gtid_binlog_pos",
                    "log_bin",
                    "binlog_format",
                    "binlog_row_image",
                ]
                .iter()
                .enumerate()
                {
                    report
                        .settings
                        .insert((*name).into(), row_text(row, i).unwrap_or_default());
                }
            }
            mariadb::parse_gtid(
                report
                    .settings
                    .get("gtid_binlog_pos")
                    .map(String::as_str)
                    .unwrap_or(""),
            )?;
        }
        Ok(report)
    }
}

fn has_process(grant: &str) -> bool {
    let upper = grant.to_ascii_uppercase();
    let Some((privileges, _)) = upper
        .strip_prefix("GRANT ")
        .and_then(|s| s.split_once(" ON *.* TO "))
    else {
        return false;
    };
    privileges
        .split(',')
        .any(|p| matches!(p.trim(), "PROCESS" | "ALL PRIVILEGES"))
}

/// Only common snapshot SQL options. No MySQL version rule is consulted.
pub(crate) fn snapshot_dialect(timeout_hint: bool) -> Dialect {
    Dialect {
        default_auth: "mysql_native_password",
        native_password: capability::NativePassword::Available,
        auth_variable: "",
        binlog_default_on: false,
        status_statement: "",
        retention_variable: "",
        retention_unit_seconds: 1,
        tagged_gtid: false,
        server_select_timeout: timeout_hint,
        stats_cached: false,
        replica_status: "",
        set_persist: false,
    }
}

pub(crate) fn snapshot_report(version: EngineVersion, reason: &str) -> ProbeReport {
    ProbeReport {
        protocol: ENGINE_PROTOCOL.into(),
        family: version.family,
        settings: BTreeMap::from([
            ("server_version".into(), version.raw.clone()),
            (
                "consistency".into(),
                "bounded per-chunk snapshot; no global snapshot or log correction".into(),
            ),
        ]),
        version,
        capabilities: BTreeMap::from([
            (Capability::Catalog, CapabilityState::Available),
            (Capability::ChunkedSnapshot, CapabilityState::Available),
            (Capability::DigestDiff, CapabilityState::Available),
            (
                Capability::Watermark,
                CapabilityState::Unsupported {
                    reason: reason.into(),
                },
            ),
            (
                Capability::LogCdc,
                CapabilityState::Unsupported {
                    reason: reason.into(),
                },
            ),
            (
                Capability::NativeChanges,
                CapabilityState::Unsupported {
                    reason: "native feed not implemented".into(),
                },
            ),
            (
                Capability::PartitionRefresh,
                CapabilityState::Unsupported {
                    reason: "partition refresh not implemented".into(),
                },
            ),
            (
                Capability::ReplicaRead,
                CapabilityState::Unsupported {
                    reason: "replica ordering not verified for this family".into(),
                },
            ),
        ]),
    }
}

macro_rules! family_engine {
    ($name:ident, $family:ident) => {
        /// Independent family adapter over the shared read-only wire substrate.
        pub struct $name(crate::mysql::MysqlEngine);
        impl $name {
            pub fn new(
                config: crate::mysql::MysqlConfig,
                password: zeroize::Zeroizing<String>,
                options: crate::mysql::EngineOptions,
            ) -> Self {
                Self(crate::mysql::MysqlEngine::new_family(
                    crate::engine::EngineFamily::$family,
                    config,
                    password,
                    options,
                ))
            }
            pub async fn close(&mut self) {
                self.0.close().await;
            }
        }
        impl crate::engine::SourceEngine for $name {
            fn family(&self) -> crate::engine::EngineFamily {
                crate::engine::EngineFamily::$family
            }
            async fn probe(&mut self) -> Result<crate::engine::ProbeReport, crate::SourceError> {
                self.0.probe().await
            }
            async fn discover_tables(
                &mut self,
                budget: &crate::engine::ReadBudget,
            ) -> Result<Vec<crate::engine::TableRef>, crate::SourceError> {
                self.0.discover_tables(budget).await
            }
            async fn plan_chunks(
                &mut self,
                table: &crate::engine::TableRef,
                budget: &crate::engine::ReadBudget,
            ) -> Result<crate::engine::TableChunkPlan, crate::SourceError> {
                self.0.plan_chunks(table, budget).await
            }
            async fn read_chunk(
                &mut self,
                chunk: &crate::engine::ChunkSpec,
                budget: &crate::engine::ReadBudget,
                sink: &mut dyn crate::engine::ChunkSink,
            ) -> Result<crate::engine::ChunkReceipt, crate::SourceError> {
                self.0.read_chunk(chunk, budget, sink).await
            }
            async fn watermark(
                &mut self,
            ) -> Result<Option<crate::engine::Watermark>, crate::SourceError> {
                self.0.watermark().await
            }
            async fn digest_range(
                &mut self,
                table: &crate::engine::TableRef,
                lo: Option<&[crate::engine::KeyValue]>,
                hi: Option<&[crate::engine::KeyValue]>,
            ) -> Result<crate::engine::RangeDigest, crate::SourceError> {
                self.0.digest_range(table, lo, hi).await
            }
        }
    };
}
pub(crate) use family_engine;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_requires_global_scope_and_exact_privilege() {
        assert!(has_process("GRANT PROCESS ON *.* TO 'reader'@'%'"));
        assert!(has_process("GRANT SELECT, PROCESS ON *.* TO 'reader'@'%'"));
        assert!(has_process("GRANT ALL PRIVILEGES ON *.* TO 'reader'@'%'"));
        for grant in [
            "GRANT PROCESS ON `db`.* TO 'reader'@'%'",
            "GRANT SELECT ON *.* TO 'PROCESS'@'%'",
            "GRANT PROCESSING ON *.* TO 'reader'@'%'",
        ] {
            assert!(!has_process(grant));
        }
    }
    #[test]
    fn independent_reports_never_offer_cdc_or_replica_reads() {
        for family in [
            EngineFamily::Mariadb,
            EngineFamily::Tidb,
            EngineFamily::Oceanbase,
        ] {
            let version = EngineVersion::parse(family, "4.0.0").unwrap();
            let report = snapshot_report(version, "not inherited");
            assert_eq!(report.family, family);
            assert_eq!(
                report.capabilities[&Capability::DigestDiff],
                CapabilityState::Available
            );
            for capability in [
                Capability::LogCdc,
                Capability::ReplicaRead,
                Capability::Watermark,
            ] {
                assert!(matches!(
                    report.capabilities[&capability],
                    CapabilityState::Unsupported { .. }
                ));
            }
        }
    }
}
