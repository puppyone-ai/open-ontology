//! PostgreSQL version dispatch. Every version-specific behavior of the
//! adapter is a row of [`pg_rules`]; the helpers below read the same facts so
//! code paths and `oo doctor` cannot disagree.

use crate::engine::{Capability, EngineFamily, EngineVersion, VersionRange, VersionRule};

/// Oldest major the adapter will talk to. Below this nothing is graded.
pub const MIN_MAJOR: u32 = 12;

/// Majors in the v0.2 verified target (docs/product/05 §1).
pub const TARGET_MAJORS: [u32; 6] = [13, 14, 15, 16, 17, 18];

/// Support level of one major, as written into probe settings.
pub fn support_level(major: u32) -> &'static str {
    match major {
        12 => "experimental",
        13..=18 => "target",
        m if m > 18 => "untested",
        _ => "unsupported",
    }
}

fn rule(from: u32, until: Option<u32>, capability: Capability, note: &str) -> VersionRule {
    VersionRule {
        family: EngineFamily::Postgres,
        versions: VersionRange::new((from, 0, 0), until.map(|u| (u, 0, 0))),
        capability,
        note: note.to_owned(),
    }
}

/// The PostgreSQL dispatch table, oldest rule first.
pub fn pg_rules() -> Vec<VersionRule> {
    use Capability::*;
    vec![
        rule(
            12,
            Some(13),
            Catalog,
            "PostgreSQL 12 is end of life: experimental, outside the v0.2 verified matrix",
        ),
        rule(
            12,
            None,
            ChunkedSnapshot,
            "keyset chunks; bounds are compared by the server under the column collation",
        ),
        rule(
            12,
            Some(14),
            ChunkedSnapshot,
            "no TID Range Scan: a table without a usable key is one bounded SingleRead",
        ),
        rule(
            14,
            None,
            ChunkedSnapshot,
            "TID Range Scan: a table without a usable key is cut into ctid ranges inside one \
             exported snapshot; ctid is never a cross-cycle checkpoint",
        ),
        rule(
            12,
            None,
            Watermark,
            "pg_current_wal_lsn() on a primary, pg_last_wal_replay_lsn() on a standby",
        ),
        rule(
            12,
            None,
            LogCdc,
            "logical slot + publication + pgoutput proto_version 1; an unconsumed slot retains WAL",
        ),
        rule(
            13,
            None,
            LogCdc,
            "publication option publish_via_partition_root",
        ),
        rule(
            14,
            None,
            LogCdc,
            "pgoutput proto_version 2: streaming of large in-progress transactions",
        ),
        rule(
            15,
            None,
            LogCdc,
            "pgoutput proto_version 3: two-phase commit decoding",
        ),
        rule(
            16,
            None,
            LogCdc,
            "pgoutput proto_version 4: parallel apply of streamed transactions",
        ),
        rule(
            12,
            Some(16),
            ReplicaRead,
            "logical decoding is not available on a standby; snapshot reads from a standby only",
        ),
        rule(
            16,
            None,
            ReplicaRead,
            "logical decoding on a standby (needs hot_standby_feedback and wal_level=logical on \
             the primary)",
        ),
        rule(
            17,
            None,
            LogCdc,
            "failover slots: CREATE_REPLICATION_SLOT ... (FAILOVER) and sync_replication_slots",
        ),
        rule(
            18,
            None,
            LogCdc,
            "idle_replication_slot_timeout invalidates an idle slot; a cycle must re-baseline",
        ),
        rule(
            18,
            None,
            LogCdc,
            "VIRTUAL generated columns are not replicated; snapshot reads compute them",
        ),
        rule(
            12,
            None,
            DigestDiff,
            "count plus order-independent sha256 sums of canonical column text with NULL flags",
        ),
    ]
}

/// Notes of every rule for `capability` that applies to `version`.
pub fn notes(version: &EngineVersion, capability: Capability) -> Vec<String> {
    let rules = pg_rules();
    VersionRule::applicable(&rules, EngineFamily::Postgres, version)
        .filter(|r| r.capability == capability)
        .map(|r| r.note.clone())
        .collect()
}

/// Highest pgoutput `proto_version` the server accepts.
pub fn pgoutput_proto_version(major: u32) -> u32 {
    match major {
        0..=13 => 1,
        14 => 2,
        15 => 3,
        _ => 4,
    }
}

/// TID Range Scan (PostgreSQL 14+) makes ctid range chunks cheap.
pub fn tid_range_scan(major: u32) -> bool {
    major >= 14
}

pub fn publish_via_partition_root(major: u32) -> bool {
    major >= 13
}

pub fn logical_decoding_on_standby(major: u32) -> bool {
    major >= 16
}

pub fn failover_slots(major: u32) -> bool {
    major >= 17
}

/// PostgreSQL 18: `idle_replication_slot_timeout` and VIRTUAL generated columns.
pub fn idle_slot_timeout(major: u32) -> bool {
    major >= 18
}

pub fn virtual_generated_columns(major: u32) -> bool {
    major >= 18
}

/// `pg_current_snapshot()` replaced `txid_current_snapshot()` in 13.
pub fn snapshot_function(major: u32) -> &'static str {
    if major >= 13 {
        "pg_catalog.pg_current_snapshot()::text"
    } else {
        "pg_catalog.txid_current_snapshot()::text"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pg(raw: &str) -> EngineVersion {
        EngineVersion::parse(EngineFamily::Postgres, raw).unwrap()
    }

    #[test]
    fn proto_versions_follow_the_matrix() {
        let got: Vec<u32> = (12..=18).map(pgoutput_proto_version).collect();
        assert_eq!(got, [1, 1, 2, 3, 4, 4, 4]);
    }

    #[test]
    fn every_target_major_has_a_watermark_and_digest_rule() {
        for major in 12..=18 {
            let v = pg(&format!("{major}.1"));
            assert_eq!(notes(&v, Capability::Watermark).len(), 1, "{major}");
            assert_eq!(notes(&v, Capability::DigestDiff).len(), 1, "{major}");
        }
    }

    #[test]
    fn chunk_rules_switch_at_fourteen() {
        for (major, ctid) in [(12, false), (13, false), (14, true), (18, true)] {
            let notes = notes(&pg(&format!("{major}.0")), Capability::ChunkedSnapshot);
            assert_eq!(notes.len(), 2, "{major}");
            assert_eq!(notes.iter().any(|n| n.contains("ctid ranges")), ctid);
            assert_eq!(notes.iter().any(|n| n.contains("SingleRead")), !ctid);
            assert_eq!(tid_range_scan(major), ctid);
        }
    }

    #[test]
    fn log_cdc_rules_accumulate_by_major() {
        let counts: Vec<usize> = (12..=18)
            .map(|m| notes(&pg(&format!("{m}.0")), Capability::LogCdc).len())
            .collect();
        // 12: v1; 13: +partition root; 14: +v2; 15: +v3; 16: +v4; 17: +failover;
        // 18: +idle timeout, +virtual columns.
        assert_eq!(counts, [1, 2, 3, 4, 5, 6, 8]);
        let n18 = notes(&pg("18.6"), Capability::LogCdc);
        assert!(
            n18.iter()
                .any(|n| n.contains("idle_replication_slot_timeout"))
        );
        assert!(n18.iter().any(|n| n.contains("VIRTUAL")));
        assert!(
            !notes(&pg("17.11"), Capability::LogCdc)
                .iter()
                .any(|n| n.contains("VIRTUAL"))
        );
    }

    #[test]
    fn standby_decoding_and_helpers_agree() {
        for major in 12..=18 {
            let v = pg(&format!("{major}.0"));
            let standby = notes(&v, Capability::ReplicaRead)
                .iter()
                .any(|n| n.starts_with("logical decoding on a standby"));
            assert_eq!(standby, logical_decoding_on_standby(major), "{major}");
            assert_eq!(failover_slots(major), major >= 17);
            assert_eq!(publish_via_partition_root(major), major >= 13);
            assert_eq!(idle_slot_timeout(major), virtual_generated_columns(major));
        }
    }

    #[test]
    fn twelve_is_experimental() {
        assert_eq!(support_level(12), "experimental");
        assert!(notes(&pg("12.22"), Capability::Catalog)[0].contains("end of life"));
        assert!(notes(&pg("13.23"), Capability::Catalog).is_empty());
        for major in TARGET_MAJORS {
            assert_eq!(support_level(major), "target");
        }
        assert_eq!(support_level(19), "untested");
        assert_eq!(support_level(11), "unsupported");
        assert!(snapshot_function(12).contains("txid_current_snapshot"));
        assert!(snapshot_function(13).contains("pg_current_snapshot"));
    }
}
