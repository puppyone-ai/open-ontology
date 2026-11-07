//! Engine versions and half-open version ranges for dispatch tables.

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::EngineFamily;
use crate::SourceError;

/// Parsed server version. `raw` keeps the server string for evidence;
/// comparison uses `major.minor.patch` only.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EngineVersion {
    pub family: EngineFamily,
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub raw: String,
}

impl EngineVersion {
    /// Parse the leading `major[.minor[.patch]]` of a server version string,
    /// e.g. `5.7.44-log`, `8.4.3`, `PostgreSQL 17.10 on aarch64`,
    /// `Oracle Database 19c ... Version 19.3.0.0.0`.
    pub fn parse(family: EngineFamily, raw: &str) -> Result<Self, SourceError> {
        let start = raw
            .find(|c: char| c.is_ascii_digit())
            .ok_or_else(|| SourceError::version("no version number in server string"))?;
        let digits = &raw[start..];
        let end = digits
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(digits.len());
        let mut parts = digits[..end].split('.').filter(|p| !p.is_empty());
        let mut next = || -> Result<u32, SourceError> {
            parts.next().map_or(Ok(0), |p| {
                p.parse()
                    .map_err(|_| SourceError::version("version part out of range"))
            })
        };
        let (major, minor, patch) = (next()?, next()?, next()?);
        Ok(Self {
            family,
            major,
            minor,
            patch,
            raw: raw.to_owned(),
        })
    }

    fn key(&self) -> (u32, u32, u32) {
        (self.major, self.minor, self.patch)
    }
}

impl PartialOrd for EngineVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        (self.family == other.family).then(|| self.key().cmp(&other.key()))
    }
}

impl fmt::Display for EngineVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// `[from, until)` over `major.minor.patch`; `until: None` is open-ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRange {
    pub from: (u32, u32, u32),
    pub until: Option<(u32, u32, u32)>,
}

impl VersionRange {
    pub const fn new(from: (u32, u32, u32), until: Option<(u32, u32, u32)>) -> Self {
        Self { from, until }
    }

    /// All versions of one major, e.g. `major(8)` = `[8.0.0, 9.0.0)`.
    pub const fn major(major: u32) -> Self {
        Self::new((major, 0, 0), Some((major + 1, 0, 0)))
    }

    pub fn contains(&self, version: &EngineVersion) -> bool {
        let v = version.key();
        v >= self.from && self.until.is_none_or(|u| v < u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(family: EngineFamily, raw: &str) -> EngineVersion {
        EngineVersion::parse(family, raw).unwrap()
    }

    #[test]
    fn parses_server_strings() {
        assert_eq!(v(EngineFamily::Mysql, "5.7.44-log").to_string(), "5.7.44");
        assert_eq!(
            v(
                EngineFamily::Postgres,
                "PostgreSQL 17.10 on aarch64-unknown-linux-gnu"
            )
            .to_string(),
            "17.10.0"
        );
        assert_eq!(v(EngineFamily::Oracle, "19.3.0.0.0").to_string(), "19.3.0");
        assert!(EngineVersion::parse(EngineFamily::Mysql, "unknown").is_err());
    }

    #[test]
    fn ranges_are_half_open() {
        let r = VersionRange::new((8, 4, 0), None);
        assert!(r.contains(&v(EngineFamily::Mysql, "8.4.0")));
        assert!(r.contains(&v(EngineFamily::Mysql, "9.1.0")));
        assert!(!r.contains(&v(EngineFamily::Mysql, "8.0.40")));
        let m8 = VersionRange::major(8);
        assert!(m8.contains(&v(EngineFamily::Mysql, "8.0.40")));
        assert!(!m8.contains(&v(EngineFamily::Mysql, "9.0.1")));
    }

    #[test]
    fn families_do_not_compare() {
        assert!(
            v(EngineFamily::Mysql, "8.0.0")
                .partial_cmp(&v(EngineFamily::Mariadb, "8.0.0"))
                .is_none()
        );
    }
}
