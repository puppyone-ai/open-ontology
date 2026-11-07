//! Product identity is separate from Kingbase's PostgreSQL compatibility version.
use crate::SourceError;
use crate::engine::{EngineFamily, EngineVersion};

pub(super) fn identity(
    raw: &str,
    mode: &str,
    compatibility_major: i64,
) -> Result<EngineVersion, SourceError> {
    // Only the owner's V9R1C10 PG-mode image has been exercised. Do not infer
    // Oracle-mode or later-release compatibility from server_version_num.
    if raw != "KingbaseES V009R001C010" || mode != "pg" || compatibility_major != 12 {
        return Err(SourceError::version("kingbase_release_or_mode_untested"));
    }
    Ok(EngineVersion {
        family: EngineFamily::Kingbase,
        major: 9,
        minor: 1,
        patch: 10,
        raw: raw.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_version_is_not_the_postgres_compatibility_version() {
        let version = identity("KingbaseES V009R001C010", "pg", 12).unwrap();
        assert_eq!((version.major, version.minor, version.patch), (9, 1, 10));
        assert_eq!(version.family, EngineFamily::Kingbase);
        for (raw, mode, pg) in [
            ("PostgreSQL 12.1", "pg", 12),
            ("KingbaseES V009R001C010", "oracle", 12),
            ("KingbaseES V009R001C011", "pg", 12),
            ("KingbaseES V009R001C010", "pg", 17),
        ] {
            assert!(identity(raw, mode, pg).is_err());
        }
    }
}
