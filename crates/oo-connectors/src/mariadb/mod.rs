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
