//! Doris and StarRocks speak the MySQL wire protocol but are separate
//! families: neither has an external row change stream in the accepted
//! lines, both expose per-partition visible versions, and aggregate-state
//! types (BITMAP, HLL, QUANTILE_STATE, AGG_STATE) are excluded from loads.

use std::collections::BTreeMap;

use crate::SourceError;
use crate::engine::{
    Capability, CapabilityState, ENGINE_PROTOCOL, EngineFamily, EngineVersion, KeyValue,
    ProbeReport, Requirement, RequirementKind, VersionRange, VersionRule,
};

/// Doris reports `5.7.99` as VERSION(); the product version is in the comment,
/// e.g. `doris version doris-3.1.4-rc02-7f5ba43de6c`.
pub fn parse_doris(comment: &str) -> Result<EngineVersion, SourceError> {
    let lower = comment.to_ascii_lowercase();
    let at = lower
        .find("doris-")
        .ok_or_else(|| SourceError::version("doris_identity"))?;
    let mut version = EngineVersion::parse(EngineFamily::Doris, &comment[at + 6..])?;
    if !VersionRange::new((2, 1, 0), Some((5, 0, 0))).contains(&version) {
        return Err(SourceError::version("doris_version_line"));
    }
    version.raw = comment.into();
    Ok(version)
}

/// StarRocks `current_version()`, e.g. `3.5.21-85fccf8`. VERSION() is the
/// MySQL compatibility string; `@@version_comment` repeats the product
/// version (3.5) or names StarRocks, and never names Doris.
pub fn parse_starrocks(current: &str, comment: &str) -> Result<EngineVersion, SourceError> {
    let lower = comment.to_ascii_lowercase();
    if lower.contains("doris") || !(lower.contains("starrocks") || comment.trim() == current.trim())
    {
        return Err(SourceError::version("starrocks_identity"));
    }
    let mut version = EngineVersion::parse(EngineFamily::Starrocks, current)?;
    if !VersionRange::new((3, 0, 0), Some((5, 0, 0))).contains(&version) {
        return Err(SourceError::version("starrocks_version_line"));
    }
    version.raw = current.into();
    Ok(version)
}

pub fn version_rules() -> Vec<VersionRule> {
    let mut rules = Vec::new();
    let common = [
        (
            Capability::Catalog,
            "DESC for exact types (information_schema reports LARGEINT as bigint unsigned); SHOW PARTITIONS for single-column RANGE partitions",
        ),
        (
            Capability::ChunkedSnapshot,
            "SELECT-only grants and statement guard; SET query_timeout from ReadBudget; no cross-chunk snapshot (consistency B)",
        ),
        (
            Capability::PartitionRefresh,
            "C4: chunk ranges aligned to RANGE partition lower bounds; a changed range digest rereads that partition",
        ),
        (
            Capability::Watermark,
            "Opaque per-partition VisibleVersion vector; not a log position",
        ),
        (
            Capability::LogCdc,
            "Unsupported: no external row change stream",
        ),
    ];
    for (family, lines) in [
        (
            EngineFamily::Doris,
            vec![(2, 1), (3, 0), (3, 1), (4, 0), (4, 1)],
        ),
        (
            EngineFamily::Starrocks,
            vec![
                (3, 0),
                (3, 1),
                (3, 2),
                (3, 3),
                (3, 4),
                (3, 5),
                (4, 0),
                (4, 1),
            ],
        ),
    ] {
        let digest = if family == EngineFamily::Doris {
            "count and LARGEINT sums of xxhash_64 and murmur_hash3_64 over length-framed row text"
        } else {
            "count and LARGEINT sums of xx_hash3_64 and murmur_hash3_32 over length-framed row text"
        };
        for (major, minor) in lines {
            for (capability, note) in common
                .iter()
                .copied()
                .chain([(Capability::DigestDiff, digest)])
            {
                rules.push(VersionRule {
                    family,
                    versions: VersionRange::new((major, minor, 0), Some((major, minor + 1, 0))),
                    capability,
                    note: note.into(),
                });
            }
        }
    }
    rules
}

/// Hash functions of the C5 digest, per family.
pub(crate) fn digest_functions(family: EngineFamily) -> (&'static str, &'static str) {
    if family == EngineFamily::Doris {
        ("xxhash_64", "murmur_hash3_64")
    } else {
        ("xx_hash3_64", "murmur_hash3_32")
    }
}

/// Lower bound of a single-column RANGE partition from SHOW PARTITIONS,
/// e.g. `[types: [DATEV2]; keys: [2024-01-01]; ..types: [DATEV2]; keys: [2024-02-01]; )`.
/// `None` for LIST partitions, multi-column keys or an unparsable form.
pub(crate) fn range_lower(range: &str) -> Option<String> {
    if !range.starts_with('[') || !range.contains("; ..types: [") {
        return None;
    }
    let (_, rest) = range.split_once("keys: [")?;
    let (value, _) = rest.split_once("]; ..")?;
    (!value.contains(", ") && !value.is_empty()).then(|| value.to_string())
}

/// Order partition lower bounds the way the source does. Only integer and
/// fixed-format date/datetime keys qualify; anything else returns `None` and
/// the table is chunked by key only.
pub(crate) fn sort_lowers(kind_is_int: bool, lowers: &mut [(String, KeyValue)]) -> bool {
    if kind_is_int {
        let mut ok = true;
        for (text, value) in lowers.iter_mut() {
            match text.parse::<i128>() {
                Ok(n) => *value = KeyValue::Int(n),
                Err(_) => ok = false,
            }
        }
        if !ok {
            return false;
        }
        lowers.sort_by_key(|(_, v)| match v {
            KeyValue::Int(n) => *n,
            _ => 0,
        });
        return true;
    }
    let shape = |t: &str| {
        let b = t.as_bytes();
        (t.len() == 10 || t.len() >= 19)
            && b.iter().enumerate().all(|(i, c)| match i {
                4 | 7 => *c == b'-',
                10 => *c == b' ',
                13 | 16 => *c == b':',
                19 => *c == b'.',
                _ => c.is_ascii_digit(),
            })
    };
    if !lowers.iter().all(|(t, _)| shape(t)) {
        return false;
    }
    let len = lowers.first().map(|(t, _)| t.len());
    if lowers.iter().any(|(t, _)| Some(t.len()) != len) {
        return false;
    }
    for (text, value) in lowers.iter_mut() {
        *value = KeyValue::Text(text.clone());
    }
    lowers.sort_by(|a, b| a.0.cmp(&b.0));
    true
}

pub(crate) struct Observed {
    pub family: EngineFamily,
    pub version: EngineVersion,
    pub select_granted: bool,
    pub account: String,
    pub database: String,
    pub query_timeout: Option<String>,
}

pub(crate) fn report(o: Observed) -> ProbeReport {
    let mut capabilities = BTreeMap::new();
    let grant = if o.family == EngineFamily::Doris {
        format!("GRANT SELECT_PRIV ON `{}`.* TO {};", o.database, o.account)
    } else {
        format!(
            "GRANT SELECT ON ALL TABLES IN DATABASE `{}` TO USER {};",
            o.database, o.account
        )
    };
    let select = if o.select_granted {
        CapabilityState::Available
    } else {
        CapabilityState::NeedsSetup {
            requirements: vec![Requirement {
                kind: RequirementKind::Privilege,
                name: "SELECT".into(),
                observed: Some("missing".into()),
                required: "SELECT on the source database".into(),
                dba_statement: Some(grant),
            }],
        }
    };
    for capability in [
        Capability::Catalog,
        Capability::ChunkedSnapshot,
        Capability::PartitionRefresh,
        Capability::DigestDiff,
        Capability::Watermark,
    ] {
        capabilities.insert(capability, select.clone());
    }
    let name = if o.family == EngineFamily::Doris {
        "Doris"
    } else {
        "StarRocks"
    };
    for (capability, reason) in [
        (
            Capability::LogCdc,
            format!("{name} exposes no external row change stream in the accepted lines"),
        ),
        (
            Capability::NativeChanges,
            "no change feed; use C4 partition refresh or C5".to_string(),
        ),
    ] {
        capabilities.insert(capability, CapabilityState::Unsupported { reason });
    }
    let mut settings = BTreeMap::new();
    if let Some(v) = o.query_timeout {
        settings.insert("query_timeout".into(), v);
    }
    for (k, v) in [
        ("transport", "mysql_wire_text_protocol"),
        ("consistency", "B: per-chunk reads, no cross-chunk snapshot"),
        (
            "watermark",
            "opaque per-partition VisibleVersion vector; not a log position",
        ),
        (
            "aggregate_types",
            "BITMAP, HLL, QUANTILE_STATE, AGG_STATE and PERCENTILE columns are excluded",
        ),
        (
            "read_only_enforcement",
            "SELECT grants and SELECT/SHOW/DESC/SET query_timeout statement guard",
        ),
    ] {
        settings.insert(k.into(), v.into());
    }
    ProbeReport {
        protocol: ENGINE_PROTOCOL.into(),
        family: o.family,
        version: o.version,
        capabilities,
        settings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_product_versions_not_mysql_prefixes() {
        let d = parse_doris("doris version doris-3.1.4-rc02-7f5ba43de6c").unwrap();
        assert_eq!(
            (d.family, d.major, d.minor, d.patch),
            (EngineFamily::Doris, 3, 1, 4)
        );
        assert_eq!(
            VersionRule::applicable(&version_rules(), d.family, &d).count(),
            6
        );
        assert!(parse_doris("5.7.99").is_err());
        assert!(parse_doris("doris version doris-1.2.8").is_err());
        let s = parse_starrocks("3.5.21-85fccf8", "StarRocks version 3.5.21-85fccf8").unwrap();
        assert_eq!(
            (s.family, s.major, s.minor),
            (EngineFamily::Starrocks, 3, 5)
        );
        assert_eq!(
            VersionRule::applicable(&version_rules(), s.family, &s).count(),
            6
        );
        assert!(parse_starrocks("3.5.21", "doris version doris-3.1.4").is_err());
        assert_eq!(
            parse_starrocks("3.5.21-85fccf8", "3.5.21-85fccf8")
                .unwrap()
                .patch,
            21
        );
        assert!(parse_starrocks("3.5.21-85fccf8", "8.0.33").is_err());
        assert!(parse_starrocks("2.5.1", "StarRocks version 2.5.1").is_err());
    }

    #[test]
    fn range_partitions_are_parsed_and_ordered_by_source_value() {
        assert_eq!(
            range_lower(
                "[types: [DATEV2]; keys: [2024-01-01]; ..types: [DATEV2]; keys: [2024-02-01]; )"
            )
            .as_deref(),
            Some("2024-01-01")
        );
        assert_eq!(range_lower("[types: [INT]; keys: [1]; ]"), None);
        assert_eq!(
            range_lower("[types: [INT, INT]; keys: [1, 2]; ..types: [INT, INT]; keys: [3, 4]; )"),
            None
        );
        let mut ints = vec![
            ("100".to_string(), KeyValue::Null),
            ("-5".into(), KeyValue::Null),
            ("20".into(), KeyValue::Null),
        ];
        assert!(sort_lowers(true, &mut ints));
        assert_eq!(
            ints.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
            ["-5", "20", "100"]
        );
        let mut dates = vec![
            ("2024-02-01".to_string(), KeyValue::Null),
            ("2024-01-01".into(), KeyValue::Null),
        ];
        assert!(sort_lowers(false, &mut dates));
        assert_eq!(dates[0].1, KeyValue::Text("2024-01-01".into()));
        let mut mixed = vec![
            ("2024-02-01".to_string(), KeyValue::Null),
            ("2024-01-01 00:00:00".into(), KeyValue::Null),
        ];
        assert!(!sort_lowers(false, &mut mixed));
        let mut text = vec![("beijing".to_string(), KeyValue::Null)];
        assert!(!sort_lowers(false, &mut text));
    }

    #[test]
    fn missing_select_lists_family_grant() {
        let r = report(Observed {
            family: EngineFamily::Starrocks,
            version: parse_starrocks("3.5.21-85fccf8", "StarRocks version 3.5.21").unwrap(),
            select_granted: false,
            account: "'reader'@'%'".into(),
            database: "fx".into(),
            query_timeout: Some("30".into()),
        });
        assert_eq!(
            r.requirements()[0].dba_statement.as_deref(),
            Some("GRANT SELECT ON ALL TABLES IN DATABASE `fx` TO USER 'reader'@'%';")
        );
    }
}
