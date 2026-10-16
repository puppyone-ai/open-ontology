//! Select actual MySQL 5.7/8.x server syntax without retrying unsupported SQL.

use super::MysqlError as PreviewError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerVersion {
    V57,
    V8(u32),
}

impl ServerVersion {
    pub fn parse(version: &str) -> Result<Self, PreviewError> {
        let core = version.split('-').next().unwrap_or("");
        let parts: Vec<_> = core.split('.').collect();
        if version.to_ascii_lowercase().contains("mariadb")
            || parts.len() != 3
            || parts[2].parse::<u32>().is_err()
        {
            return Err(PreviewError::new("version", "mysql_major"));
        }
        match (parts[0], parts[1].parse::<u32>()) {
            ("5", Ok(7)) => Ok(Self::V57),
            ("8", Ok(minor)) => Ok(Self::V8(minor)),
            _ => Err(PreviewError::new("version", "mysql_major")),
        }
    }

    pub fn retention_query(self) -> &'static str {
        match self {
            Self::V57 => "SELECT CAST(@@GLOBAL.expire_logs_days AS UNSIGNED) * 86400",
            Self::V8(_) => "SELECT @@GLOBAL.binlog_expire_logs_seconds",
        }
    }

    pub fn status_query(self) -> &'static str {
        match self {
            Self::V57 | Self::V8(0..=3) => "SHOW MASTER STATUS",
            Self::V8(_) => "SHOW BINARY LOG STATUS",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ServerVersion;

    #[test]
    fn server_syntax_follows_actual_supported_version() {
        let v57 = ServerVersion::parse("5.7.44-log").unwrap();
        assert_eq!(v57.status_query(), "SHOW MASTER STATUS");
        assert!(v57.retention_query().contains("expire_logs_days"));
        assert_eq!(
            ServerVersion::parse("8.0.41").unwrap().status_query(),
            "SHOW MASTER STATUS"
        );
        assert_eq!(
            ServerVersion::parse("8.4.6").unwrap().status_query(),
            "SHOW BINARY LOG STATUS"
        );
        for wrong in [
            "5.6.51",
            "5.70.44",
            "9.0.1",
            "8.4",
            "8.4.x",
            "5.7.44-MariaDB",
        ] {
            assert!(ServerVersion::parse(wrong).is_err(), "{wrong}");
        }
    }
}
