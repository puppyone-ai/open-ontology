//! ClickHouse is its own family: no binlog, no multi-statement transactions,
//! background merges and asynchronous mutations. Reads use the server's
//! MySQL wire interface under a `readonly` profile, `FINAL` for merging
//! engines, and one keyset range per `_partition_id` for partition refresh.

use std::collections::BTreeMap;

use crate::SourceError;
use crate::engine::{
    Capability, CapabilityState, ENGINE_PROTOCOL, EngineFamily, EngineVersion, ProbeReport,
    Requirement, RequirementKind, VersionRange, VersionRule,
};

/// First line with lightweight DELETE and `_partition_id` pruning that this
/// connector accepts; 26.8 is the exercised LTS.
const MIN: (u32, u32, u32) = (24, 3, 0);

pub fn parse_version(raw: &str) -> Result<EngineVersion, SourceError> {
    if !raw
        .split('.')
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        || raw.split('.').count() < 3
    {
        return Err(SourceError::version("clickhouse_identity"));
    }
    let version = EngineVersion::parse(EngineFamily::Clickhouse, raw)?;
    if !VersionRange::new(MIN, Some((27, 0, 0))).contains(&version) {
        return Err(SourceError::version("clickhouse_version_line"));
    }
    Ok(version)
}

pub fn version_rules() -> Vec<VersionRule> {
    let lines = [
        ((24, 3, 0), Some((25, 0, 0))),
        ((25, 0, 0), Some((26, 0, 0))),
        ((26, 0, 0), Some((27, 0, 0))),
    ];
    lines
        .into_iter()
        .flat_map(|(from, until)| {
            [
                (Capability::Catalog, "system.tables/system.columns; LowCardinality(T) is T; Nullable kept; non-Nullable columns flagged default_for_missing"),
                (Capability::ChunkedSnapshot, "MySQL wire interface, text protocol; FINAL on Replacing/Collapsing/Aggregating/Summing MergeTree; readonly profile forbids SET, so max_execution_time comes from the profile"),
                (Capability::PartitionRefresh, "C4: one keyset range per _partition_id in bytewise source order; a changed range digest rereads that partition; pending mutations refuse the read (DELETES_LAGGED)"),
                (Capability::DigestDiff, "count, wrapping sum and xor of cityHash64(toString(tuple(row))) over FINAL, per range"),
                (Capability::LogCdc, "Unsupported: ClickHouse has no binlog or row change log"),
                (Capability::Watermark, "Unsupported: no log position; partition refresh compares content digests"),
            ]
            .into_iter()
            .map(move |(capability, note)| VersionRule {
                family: EngineFamily::Clickhouse,
                versions: VersionRange::new(from, until),
                capability,
                note: note.into(),
            })
        })
        .collect()
}

/// Merging engines whose stored rows are not the logical rows until merged.
pub(crate) fn needs_final(engine: &str) -> bool {
    let e = engine
        .strip_prefix("Replicated")
        .or_else(|| engine.strip_prefix("Shared"))
        .unwrap_or(engine);
    matches!(
        e,
        "ReplacingMergeTree"
            | "CollapsingMergeTree"
            | "VersionedCollapsingMergeTree"
            | "AggregatingMergeTree"
            | "SummingMergeTree"
            | "CoalescingMergeTree"
    )
}

pub(crate) fn is_merge_tree(engine: &str) -> bool {
    engine.ends_with("MergeTree")
}

/// Plain column names of a sorting key, stopping at the first expression.
pub(crate) fn sorting_columns(sorting_key: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in sorting_key.split(',') {
        let name = part.trim().trim_matches('`');
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            break;
        }
        out.push(name.to_string());
    }
    out
}

pub(crate) struct Observed {
    pub version: EngineVersion,
    pub select_granted: bool,
    pub readonly: Option<String>,
    pub max_execution_time: Option<String>,
    pub mutations_visible: bool,
    pub account: String,
    pub database: String,
}

fn need(
    kind: RequirementKind,
    name: &str,
    observed: Option<String>,
    required: &str,
    dba: String,
) -> CapabilityState {
    CapabilityState::NeedsSetup {
        requirements: vec![Requirement {
            kind,
            name: name.into(),
            observed,
            required: required.into(),
            dba_statement: Some(dba),
        }],
    }
}

pub(crate) fn report(o: Observed) -> ProbeReport {
    let user = format!("`{}`", o.account);
    let mut capabilities = BTreeMap::new();
    let select = if o.select_granted {
        CapabilityState::Available
    } else {
        need(
            RequirementKind::Privilege,
            "SELECT",
            Some("missing".into()),
            "SELECT on the source database",
            format!("GRANT SELECT ON `{}`.* TO {user};", o.database),
        )
    };
    capabilities.insert(Capability::Catalog, select.clone());
    let mut snapshot = select.clone();
    if snapshot == CapabilityState::Available {
        if o.readonly.as_deref().is_none_or(|v| v == "0") {
            snapshot = need(
                RequirementKind::Parameter,
                "readonly",
                o.readonly.clone(),
                "1 (readonly profile)",
                format!("ALTER USER {user} SETTINGS readonly = 1;"),
            );
        } else if o.max_execution_time.as_deref().is_none_or(|v| v == "0") {
            snapshot = need(
                RequirementKind::Parameter,
                "max_execution_time",
                o.max_execution_time.clone(),
                "> 0 seconds, enforced by the profile",
                format!("ALTER USER {user} SETTINGS readonly = 1, max_execution_time = 30;"),
            );
        }
    }
    capabilities.insert(Capability::ChunkedSnapshot, snapshot.clone());
    capabilities.insert(Capability::DigestDiff, snapshot.clone());
    capabilities.insert(
        Capability::PartitionRefresh,
        if snapshot != CapabilityState::Available {
            snapshot.clone()
        } else if o.mutations_visible {
            CapabilityState::Available
        } else {
            need(
                RequirementKind::Privilege,
                "SELECT ON system.mutations",
                Some("missing".into()),
                "pending mutations must be visible so a lagging DELETE is not published",
                format!("GRANT SELECT ON system.mutations TO {user};"),
            )
        },
    );
    for (capability, reason) in [
        (
            Capability::LogCdc,
            "ClickHouse has no binlog or row change log",
        ),
        (
            Capability::NativeChanges,
            "no change feed; use C4 partition refresh or C5",
        ),
        (
            Capability::Watermark,
            "no log position; partition refresh compares content digests",
        ),
    ] {
        capabilities.insert(
            capability,
            CapabilityState::Unsupported {
                reason: reason.into(),
            },
        );
    }
    let mut settings = BTreeMap::new();
    if let Some(v) = o.readonly {
        settings.insert("readonly".into(), v);
    }
    if let Some(v) = o.max_execution_time {
        settings.insert("max_execution_time".into(), v);
    }
    for (k, v) in [
        ("transport", "mysql_wire_text_protocol"),
        ("consistency", "B: per-chunk reads, no cross-chunk snapshot"),
        ("final", "FINAL on merging MergeTree engines"),
        (
            "default_for_missing",
            "non-Nullable columns hold a type default for a missing value; flagged per Arrow field",
        ),
        (
            "async_delete",
            "a pending mutation refuses the read as DELETES_LAGGED; lightweight DELETE masks are applied by the server",
        ),
        (
            "read_only_enforcement",
            "server readonly profile and SELECT/SHOW/DESC statement guard",
        ),
    ] {
        settings.insert(k.into(), v.into());
    }
    ProbeReport {
        protocol: ENGINE_PROTOCOL.into(),
        family: EngineFamily::Clickhouse,
        version: o.version,
        capabilities,
        settings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_rules() {
        let v = parse_version("26.8.21.10").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (26, 8, 21));
        assert_eq!(
            VersionRule::applicable(&version_rules(), v.family, &v).count(),
            6
        );
        for bad in ["23.8.1.1", "27.1.1.1", "8.0.36", "5.7.99-doris"] {
            assert!(parse_version(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn engines_keys_and_requirements() {
        assert!(needs_final("ReplicatedReplacingMergeTree"));
        assert!(!needs_final("MergeTree"));
        assert!(is_merge_tree("SharedMergeTree") && !is_merge_tree("View"));
        assert_eq!(sorting_columns("d, id, toDate(ts), x"), ["d", "id"]);
        assert!(sorting_columns("").is_empty());
        let observed = |mutations, max: &str| Observed {
            version: parse_version("26.8.21.10").unwrap(),
            select_granted: true,
            readonly: Some("1".into()),
            max_execution_time: Some(max.into()),
            mutations_visible: mutations,
            account: "reader".into(),
            database: "fx".into(),
        };
        let r = report(observed(false, "30"));
        assert_eq!(
            r.capabilities[&Capability::ChunkedSnapshot],
            CapabilityState::Available
        );
        let reqs = r.requirements();
        assert_eq!(reqs.len(), 1);
        assert_eq!(
            reqs[0].dba_statement.as_deref(),
            Some("GRANT SELECT ON system.mutations TO `reader`;")
        );
        let r = report(observed(true, "0"));
        assert_eq!(r.requirements()[0].name, "max_execution_time");
        assert!(matches!(
            r.capabilities[&Capability::LogCdc],
            CapabilityState::Unsupported { .. }
        ));
    }
}
