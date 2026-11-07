//! OceanBase CE MySQL mode only; synthetic OBLogProxy binlog is not native C1.
use crate::SourceError;
use crate::engine::{
    Capability, EngineFamily, EngineVersion, ProbeReport, VersionRange, VersionRule,
};
use crate::mysql::engine::family::{family_engine, snapshot_dialect, snapshot_report};
family_engine!(OceanbaseEngine, Oceanbase);

/// `raw` is OB_VERSION(), not the MySQL compatibility VERSION() string.
pub fn parse_version(raw: &str, comment: &str) -> Result<EngineVersion, SourceError> {
    if !comment.to_ascii_lowercase().contains("oceanbase") {
        return Err(SourceError::version("oceanbase_identity"));
    }
    let version = EngineVersion::parse(EngineFamily::Oceanbase, raw)?;
    if version.major != 4 {
        return Err(SourceError::version("oceanbase_version_line"));
    }
    Ok(version)
}
pub fn version_rules() -> Vec<VersionRule> {
    [
        (Capability::ChunkedSnapshot,"CE 4.x MySQL mode only; ob_query_timeout is microseconds; bounded repeatable-read chunks"),
        (Capability::DigestDiff,"C5 source-collation ranges; synthetic binlog compatibility is not inherited"),
        (Capability::LogCdc,"Unsupported: OBLogProxy/Binlog service requires separate setup and protocol validation"),
    ].into_iter().map(|(capability,note)| VersionRule { family: EngineFamily::Oceanbase,versions:VersionRange::major(4),capability,note:note.into() }).collect()
}
pub(crate) fn dialect() -> crate::mysql::capability::Dialect {
    snapshot_dialect(false)
}
pub(crate) fn report(version: EngineVersion) -> ProbeReport {
    snapshot_report(
        version,
        "OceanBase binlog is synthetic; OBLogProxy/Binlog service setup and separate validation required",
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn does_not_accept_mysql_compatibility_version() {
        let v = parse_version("4.3.5.3", "OceanBase_CE 4.3.5.3").unwrap();
        assert_eq!(v.family, EngineFamily::Oceanbase);
        assert!(parse_version("5.7.25", "OceanBase_CE").is_err());
        assert!(parse_version("4.3.5", "MySQL").is_err());
        assert!(matches!(
            report(v).capabilities[&Capability::LogCdc],
            crate::engine::CapabilityState::Unsupported { .. }
        ));
    }
}
