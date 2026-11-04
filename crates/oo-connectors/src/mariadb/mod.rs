//! MariaDB is not MySQL. JSON aliases remain LONGTEXT, not binary JSON.
//! Shared keyset/Arrow/C5 code replaces the preview's fixture-specific reader.
use crate::SourceError;
use crate::engine::{
    Capability, EngineFamily, EngineVersion, ProbeReport, VersionRange, VersionRule,
};
use crate::mysql::engine::family::{family_engine, snapshot_dialect, snapshot_report};

family_engine!(MariadbEngine, Mariadb);

pub fn parse_version(raw: &str) -> Result<EngineVersion, SourceError> {
    if !raw.to_ascii_lowercase().contains("mariadb") {
        return Err(SourceError::version("mariadb_identity"));
    }
    let core = raw.strip_prefix("5.5.5-").unwrap_or(raw);
    let mut version = EngineVersion::parse(EngineFamily::Mariadb, core)?;
    if !matches!((version.major, version.minor), (10, 6 | 11) | (11, 4 | 8)) {
        return Err(SourceError::version("mariadb_version_line"));
    }
    version.raw = raw.into();
    Ok(version)
}

/// MariaDB GTID is domain-server-sequence, never a MySQL UUID interval set.
pub fn parse_gtid(raw: &str) -> Result<Vec<(u32, u32, u64)>, SourceError> {
    if raw.is_empty() {
        return Ok(vec![]);
    }
    let mut domains = std::collections::BTreeSet::new();
    raw.split(',')
        .map(|gtid| {
            let parts: Vec<_> = gtid.split('-').collect();
            let bad = || SourceError::protocol("mariadb_gtid");
            if parts.len() != 3
                || parts
                    .iter()
                    .any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err(bad());
            }
            let domain = parts[0].parse::<u32>().map_err(|_| bad())?;
            let server = parts[1].parse::<u32>().map_err(|_| bad())?;
            let sequence = parts[2].parse::<u64>().map_err(|_| bad())?;
            if !domains.insert(domain) || sequence == 0 {
                return Err(bad());
            }
            Ok((domain, server, sequence))
        })
        .collect()
}

pub fn version_rules() -> Vec<VersionRule> {
    [(10,6), (10,11), (11,4), (11,8)].into_iter().flat_map(|(major,minor)| {
        [
            (Capability::Catalog, "JSON is LONGTEXT with JSON_VALID check; retain text without inferring binary JSON"),
            (Capability::Catalog, "mysql_native_password supported; unix_socket requires local socket; ed25519 not enabled in this driver"),
            (Capability::Watermark, "GTID uses domain-server-sequence, not MySQL UUID sets; SHOW BINLOG STATUS aliases SHOW MASTER STATUS"),
            (Capability::ChunkedSnapshot, "InnoDB read-only repeatable-read chunks; max_statement_time in seconds"),
            (Capability::DigestDiff, "C5 uses source-collation key ranges and count plus framed MD5 sums; not CDC"),
        ].into_iter().map(move |(capability,note)| VersionRule {
            family: EngineFamily::Mariadb, versions: VersionRange::new((major,minor,0),Some((major,minor+1,0))), capability, note: note.into(),
        })
    }).collect()
}

pub(crate) fn dialect() -> crate::mysql::capability::Dialect {
    let mut d = snapshot_dialect(false);
    d.status_statement = "SHOW BINLOG STATUS";
    d.replica_status = "SHOW SLAVE STATUS";
    d.retention_variable = "expire_logs_days";
    d.retention_unit_seconds = 86400;
    d
}
pub(crate) fn report(version: EngineVersion) -> ProbeReport {
    let mut report = snapshot_report(
        version,
        "MariaDB binlog transport not yet validated; MySQL C1 compatibility is not inherited",
    );
    report
        .settings
        .insert("json_storage".into(), "LONGTEXT; retained as text".into());
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_identity_and_gtids_are_independent() {
        for raw in [
            "10.6.24-MariaDB",
            "10.11.14-MariaDB",
            "11.4.8-MariaDB",
            "5.5.5-11.8.3-MariaDB",
        ] {
            let version = parse_version(raw).unwrap();
            assert_eq!(version.family, EngineFamily::Mariadb);
            assert_eq!(
                VersionRule::applicable(&version_rules(), version.family, &version).count(),
                5
            );
        }
        for raw in [
            "8.4.7",
            "5.7.25-TiDB-v7.5.0",
            "10.5.30-MariaDB",
            "11.5.0-MariaDB",
        ] {
            assert!(parse_version(raw).is_err());
        }
        assert_eq!(
            parse_gtid("0-731-4,7-8-9").unwrap(),
            vec![(0, 731, 4), (7, 8, 9)]
        );
        for raw in [
            "uuid:1-4",
            "0-1-0",
            "0-1-2,0-2-3",
            "1-2",
            "+1-2-3",
            "1-2-3-4",
        ] {
            assert!(parse_gtid(raw).is_err());
        }
    }
}

#[cfg(any(test, feature = "mysql-binlog"))]
pub(crate) fn gtid_follows(previous: &[(u32, u32, u64)], current: &[(u32, u32, u64)]) -> bool {
    previous.iter().all(|(domain, server, seq)| {
        current
            .iter()
            .any(|(d, s, n)| d == domain && s == server && n >= seq)
    })
}

#[cfg(feature = "mysql-binlog")]
pub struct MariadbChanges(crate::mysql::MysqlChanges);

#[cfg(feature = "mysql-binlog")]
impl MariadbEngine {
    /// Primary-only ROW/FULL binlog in legacy-slave mode. Resume is file/position,
    /// never a MySQL GTID set. Purge, GTID regression or replica topology fails closed.
    pub async fn changes(
        &mut self,
        tables: &[crate::engine::TableRef],
        server_id: u32,
    ) -> Result<MariadbChanges, SourceError> {
        Ok(MariadbChanges(self.0.changes(tables, server_id).await?))
    }
}
#[cfg(feature = "mysql-binlog")]
impl MariadbChanges {
    pub async fn close(&mut self) {
        self.0.close().await;
    }
    pub async fn retention(
        &mut self,
        cycle_seconds: u64,
        recovery_seconds: u64,
    ) -> Result<crate::mysql::BinlogRetention, SourceError> {
        self.0.retention(cycle_seconds, recovery_seconds).await
    }
}
#[cfg(feature = "mysql-binlog")]
impl crate::engine::ChangeSource for MariadbChanges {
    async fn earliest_available(&mut self) -> Result<crate::engine::Watermark, SourceError> {
        self.0.earliest_available().await
    }
    async fn next_batch(
        &mut self,
        from: &crate::engine::Watermark,
        budget: &crate::engine::ReadBudget,
    ) -> Result<Option<crate::engine::ChangeBatch>, SourceError> {
        self.0.next_batch(from, budget).await
    }
}

pub(crate) fn grade_log(
    report: &mut ProbeReport,
    grants: &[String],
    account: Option<&str>,
    replica: Option<bool>,
) {
    use crate::engine::{CapabilityState, Requirement, RequirementKind};
    let mut requirements = vec![];
    let has = |privilege: &str| {
        grants.iter().any(|line| {
            let line = line.to_ascii_uppercase();
            line.strip_prefix("GRANT ")
                .and_then(|s| s.split_once(" ON *.* TO "))
                .is_some_and(|(p, _)| {
                    p.split(',')
                        .any(|p| matches!(p.trim(), "ALL PRIVILEGES") || p.trim() == privilege)
                })
        })
    };
    for privilege in ["BINLOG MONITOR", "REPLICA MONITOR"] {
        if !has(privilege)
            && !(privilege == "BINLOG MONITOR" && has("REPLICATION CLIENT"))
            && !(privilege == "REPLICA MONITOR" && has("SLAVE MONITOR"))
        {
            requirements.push(Requirement {
                kind: RequirementKind::Privilege,
                name: privilege.into(),
                observed: Some("missing".into()),
                required: privilege.into(),
                dba_statement: account.map(|a| format!("GRANT {privilege} ON *.* TO {a};")),
            });
        }
    }
    if report.settings.get("log_bin").map(String::as_str) != Some("1") {
        requirements.push(Requirement {
            kind: RequirementKind::Parameter,
            name: "log_bin".into(),
            observed: report.settings.get("log_bin").cloned(),
            required: "ON".into(),
            dba_statement: Some(
                "-- DBA: enable log_bin in my.cnf and restart; rollback by disabling log_bin"
                    .into(),
            ),
        });
    }
    let watermark = if replica == Some(true) {
        CapabilityState::Unsupported {
            reason: "MariaDB replica commit ordering is not verified; primary only".into(),
        }
    } else if requirements.is_empty() && replica == Some(false) {
        CapabilityState::Available
    } else {
        if requirements.is_empty() {
            requirements.push(Requirement {
                kind: RequirementKind::Privilege,
                name: "replica_topology".into(),
                observed: None,
                required: "successful SHOW SLAVE STATUS on primary".into(),
                dba_statement: None,
            });
        }
        CapabilityState::NeedsSetup {
            requirements: requirements.clone(),
        }
    };
    report
        .capabilities
        .insert(Capability::Watermark, watermark.clone());
    if !cfg!(feature = "mysql-binlog") {
        report.capabilities.insert(
            Capability::LogCdc,
            CapabilityState::Unsupported {
                reason:
                    "build requires opt-in mysql-binlog feature; default artifact remains gated"
                        .into(),
            },
        );
        return;
    }
    if matches!(watermark, CapabilityState::Unsupported { .. }) {
        report.capabilities.insert(Capability::LogCdc, watermark);
        return;
    }
    if !has("REPLICATION SLAVE") {
        requirements.push(Requirement {
            kind: RequirementKind::Privilege,
            name: "REPLICATION SLAVE".into(),
            observed: Some("missing".into()),
            required: "REPLICATION SLAVE".into(),
            dba_statement: account.map(|a| format!("GRANT REPLICATION SLAVE ON *.* TO {a};")),
        });
    }
    for (name, expected) in [("binlog_format", "ROW"), ("binlog_row_image", "FULL")] {
        if report.settings.get(name).map(String::as_str) != Some(expected) {
            requirements.push(Requirement {kind:RequirementKind::Parameter,name:name.into(),observed:report.settings.get(name).cloned(),required:expected.into(),dba_statement:Some(format!("SET GLOBAL {name} = '{expected}'; -- persist in my.cnf; restore the previous value to roll back"))});
        }
    }
    report.settings.insert(
        "cdc_resume".into(),
        "file-position; MariaDB GTID monotonic lineage guard; legacy-slave annotation no-op only"
            .into(),
    );
    report.settings.insert(
        "retention".into(),
        "configured retention does not prove availability; call retention() before resume".into(),
    );
    report.capabilities.insert(
        Capability::LogCdc,
        if requirements.is_empty() {
            CapabilityState::Available
        } else {
            CapabilityState::NeedsSetup { requirements }
        },
    );
}

/// Legacy-slave annotation comments are truncated to the original event size.
#[cfg(feature = "mysql-binlog")]
pub(crate) fn annotation_noop(sql: &str) -> bool {
    sql.starts_with("# DUMMY EVENT")
        && "# DUMMY EVENT REPLACING EVENT TYPE 160 THAT SLAVE CANNOT HANDLE.".starts_with(sql)
}

#[cfg(test)]
mod log_tests {
    use super::*;
    #[test]
    fn domain_server_sequence_cannot_regress_or_change_server() {
        assert!(gtid_follows(&[(0, 1, 3)], &[(0, 1, 4), (2, 3, 1)]));
        assert!(!gtid_follows(&[(0, 1, 3)], &[(0, 1, 2)]));
        assert!(!gtid_follows(&[(0, 1, 3)], &[(0, 2, 4)]));
        assert!(!gtid_follows(&[(0, 1, 3)], &[]));
    }
    #[cfg(feature = "mysql-binlog")]
    #[test]
    fn only_known_truncated_annotation_comments_are_noops() {
        for sql in [
            "# DUMMY EVENT REPLACING EVENT",
            "# DUMMY EVENT REPLACING EVENT TYPE 160",
        ] {
            assert!(annotation_noop(sql));
        }
        for sql in [
            "#",
            "# DUMMY EVENT; DELETE FROM t",
            "# DUMMY EVENT REPLACING EVENT TYPE 162",
            "DELETE FROM t",
            "# arbitrary comment",
        ] {
            assert!(!annotation_noop(sql));
        }
    }
    #[test]
    fn grants_and_topology_gate_log_capabilities() {
        use crate::engine::CapabilityState;
        let mut r = report(parse_version("11.4.8-MariaDB").unwrap());
        for (key, value) in [
            ("log_bin", "1"),
            ("binlog_format", "ROW"),
            ("binlog_row_image", "FULL"),
        ] {
            r.settings.insert(key.into(), value.into());
        }
        let grants =
            vec!["GRANT BINLOG MONITOR, SLAVE MONITOR, REPLICATION SLAVE ON *.* TO 'r'@'%'".into()];
        grade_log(&mut r, &grants, Some("'r'@'%'"), Some(false));
        assert_eq!(
            r.capabilities[&Capability::Watermark],
            CapabilityState::Available
        );
        if cfg!(feature = "mysql-binlog") {
            assert_eq!(
                r.capabilities[&Capability::LogCdc],
                CapabilityState::Available
            );
        }
        grade_log(&mut r, &grants, Some("'r'@'%'"), Some(true));
        assert!(matches!(
            r.capabilities[&Capability::Watermark],
            CapabilityState::Unsupported { .. }
        ));
        grade_log(&mut r, &[], Some("'r'@'%'"), None);
        assert!(matches!(
            r.capabilities[&Capability::Watermark],
            CapabilityState::NeedsSetup { .. }
        ));
    }
}
