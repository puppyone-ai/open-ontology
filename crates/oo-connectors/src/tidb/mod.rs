//! TiDB snapshot/C5 only. TiCDC and historical AS OF reads are not implied.
use crate::SourceError;
use crate::engine::{
    Capability, EngineFamily, EngineVersion, ProbeReport, VersionRange, VersionRule,
};
use crate::mysql::engine::family::{family_engine, snapshot_dialect, snapshot_report};
family_engine!(TidbEngine, Tidb);

pub fn parse_version(raw: &str) -> Result<EngineVersion, SourceError> {
    let (_, actual) = raw
        .split_once("TiDB-v")
        .ok_or_else(|| SourceError::version("tidb_identity"))?;
    let mut version = EngineVersion::parse(EngineFamily::Tidb, actual)?;
    if !matches!((version.major, version.minor), (7, 5) | (8, 5)) {
        return Err(SourceError::version("tidb_version_line"));
    }
    version.raw = raw.into();
    Ok(version)
}
pub fn version_rules() -> Vec<VersionRule> {
    [7,8].into_iter().flat_map(|major| [
        (Capability::ChunkedSnapshot,"Repeatable-read chunks; READ ONLY unsupported, guarded SELECT-only SQL instead; PROCESS required for load measurement"),
        (Capability::DigestDiff,"C5 source-collation ranges; no MySQL binlog protocol; TiCDC out of scope"),
        (Capability::LogCdc,"Unsupported: TiDB does not expose MySQL replication protocol; TiCDC is a separate integration"),
    ].into_iter().map(move |(capability,note)| VersionRule { family: EngineFamily::Tidb, versions: VersionRange::new((major,5,0),Some((major,6,0))), capability,note:note.into() })).collect()
}
pub(crate) fn dialect() -> crate::mysql::capability::Dialect {
    snapshot_dialect(true)
}
pub(crate) fn report(version: EngineVersion) -> ProbeReport {
    let mut report = snapshot_report(
        version,
        "TiDB has no MySQL replication protocol; TiCDC is out of scope",
    );
    report.settings.insert(
        "read_only_enforcement".into(),
        "connector SQL guard and SELECT grants; server READ ONLY unsupported".into(),
    );
    report.settings.insert(
        "load_observer".into(),
        "local PROCESSLIST with global PROCESS privilege; max_connections=0 means unlimited".into(),
    );
    report
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_product_version_not_mysql_prefix() {
        for (raw, major) in [("5.7.25-TiDB-v7.5.0", 7), ("8.0.11-TiDB-v8.5.0", 8)] {
            let v = parse_version(raw).unwrap();
            assert_eq!(v.major, major);
            assert_eq!(v.family, EngineFamily::Tidb);
            assert!(matches!(
                report(v).capabilities[&Capability::LogCdc],
                crate::engine::CapabilityState::Unsupported { .. }
            ));
        }
        for raw in ["7.5.0", "5.7.25-TiDB-v7.6.0", "11.4.8-MariaDB"] {
            assert!(parse_version(raw).is_err());
        }
    }
}
