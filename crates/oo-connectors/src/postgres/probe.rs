//! Capability grading from observed facts. The queries that collect
//! [`ProbeFacts`] are in `mod.rs`; grading is pure and unit tested.

use std::collections::BTreeMap;

use crate::engine::{
    Capability, CapabilityState, ENGINE_PROTOCOL, EngineFamily, EngineVersion, ProbeReport,
    Requirement, RequirementKind,
};

use super::version;

/// One selected table as the probing principal sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableFacts {
    /// `quote_ident(schema) || '.' || quote_ident(name)`.
    pub qualified: String,
    pub exists: bool,
    pub schema_usage: bool,
    pub select: bool,
    pub published: bool,
    pub has_primary_key: bool,
    /// `relreplident`: `d`efault, `n`othing, `f`ull, `i`ndex.
    pub replica_identity: String,
    pub partitioned: bool,
}

/// Everything grading needs. Values are settings, never secrets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeFacts {
    pub version: EngineVersion,
    /// `quote_ident(current_user)`.
    pub user: String,
    pub in_recovery: bool,
    pub wal_level: String,
    pub max_replication_slots: i64,
    pub max_wal_senders: i64,
    pub rolreplication: bool,
    pub rolsuper: bool,
    pub read_all_stats: bool,
    pub hot_standby_feedback: Option<String>,
    pub sync_replication_slots: Option<String>,
    pub idle_replication_slot_timeout: Option<String>,
    pub max_connections: i64,
    pub block_size: i64,
    pub publications: i64,
    pub tables: Vec<TableFacts>,
}

fn need(
    kind: RequirementKind,
    name: impl Into<String>,
    observed: Option<String>,
    required: &str,
    statement: Option<String>,
) -> Requirement {
    Requirement {
        kind,
        name: name.into(),
        observed,
        required: required.to_owned(),
        dba_statement: statement,
    }
}

fn state(requirements: Vec<Requirement>) -> CapabilityState {
    if requirements.is_empty() {
        CapabilityState::Available
    } else {
        CapabilityState::NeedsSetup { requirements }
    }
}

fn protected_read_requirements(facts: &ProbeFacts) -> Vec<Requirement> {
    let mut out = select_requirements(facts);
    if !facts.read_all_stats && !facts.rolsuper {
        out.push(need(
            RequirementKind::Privilege,
            "pg_read_all_stats",
            Some("active sessions of other roles are hidden".into()),
            "visibility of all active sessions for source protection",
            Some(format!("GRANT pg_read_all_stats TO {};", facts.user)),
        ));
    }
    out
}

fn select_requirements(facts: &ProbeFacts) -> Vec<Requirement> {
    let mut out = Vec::new();
    for t in &facts.tables {
        let schema = schema_ident(&t.qualified);
        if !t.exists {
            out.push(need(
                RequirementKind::Feature,
                format!("table:{}", t.qualified),
                Some("missing or not visible".into()),
                "selected table exists",
                None,
            ));
            continue;
        }
        if !t.schema_usage {
            out.push(need(
                RequirementKind::Privilege,
                format!("usage:{schema}"),
                Some("no USAGE".into()),
                "USAGE on the schema",
                Some(format!("GRANT USAGE ON SCHEMA {schema} TO {};", facts.user)),
            ));
        }
        if !t.select {
            out.push(need(
                RequirementKind::Privilege,
                format!("select:{}", t.qualified),
                Some("no SELECT".into()),
                "SELECT on the table",
                Some(format!(
                    "GRANT SELECT ON {} TO {};",
                    t.qualified, facts.user
                )),
            ));
        }
    }
    out
}

fn schema_ident(qualified: &str) -> &str {
    let mut quoted = false;
    for (i, ch) in qualified.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '.' if !quoted => return &qualified[..i],
            _ => {}
        }
    }
    qualified
}

fn log_cdc(facts: &ProbeFacts) -> CapabilityState {
    let major = facts.version.major;
    if facts.in_recovery && !version::logical_decoding_on_standby(major) {
        return CapabilityState::Unsupported {
            reason: "logical decoding on a standby needs PostgreSQL 16 or later; connect to \
                     the primary for C1 or use C5"
                .into(),
        };
    }
    let mut out = Vec::new();
    if facts.wal_level != "logical" {
        out.push(need(
            RequirementKind::Parameter,
            "wal_level",
            Some(facts.wal_level.clone()),
            "logical",
            Some(
                "ALTER SYSTEM SET wal_level = 'logical'; -- restart the server; on a standby \
                 set it on the primary"
                    .into(),
            ),
        ));
    }
    if facts.max_replication_slots < 1 {
        out.push(need(
            RequirementKind::Parameter,
            "max_replication_slots",
            Some(facts.max_replication_slots.to_string()),
            ">= 1 free slot",
            Some("ALTER SYSTEM SET max_replication_slots = 10; -- restart the server".into()),
        ));
    }
    if facts.max_wal_senders < 1 {
        out.push(need(
            RequirementKind::Parameter,
            "max_wal_senders",
            Some(facts.max_wal_senders.to_string()),
            ">= 1 free sender",
            Some("ALTER SYSTEM SET max_wal_senders = 10; -- restart the server".into()),
        ));
    }
    if !facts.rolreplication && !facts.rolsuper {
        out.push(need(
            RequirementKind::Privilege,
            "replication",
            Some("NOREPLICATION".into()),
            "REPLICATION role attribute (managed services: their replication role, e.g. \
             rds_replication)",
            Some(format!("ALTER ROLE {} WITH REPLICATION;", facts.user)),
        ));
    }
    let unpublished: Vec<&TableFacts> = facts
        .tables
        .iter()
        .filter(|t| t.exists && !t.published)
        .collect();
    if !unpublished.is_empty() {
        let list: Vec<&str> = unpublished.iter().map(|t| t.qualified.as_str()).collect();
        let root =
            version::publish_via_partition_root(major) && unpublished.iter().any(|t| t.partitioned);
        let with = if root {
            " WITH (publish_via_partition_root = true)"
        } else {
            ""
        };
        out.push(need(
            RequirementKind::Feature,
            "publication",
            Some(format!(
                "{} publication(s); {} selected table(s) not published",
                facts.publications,
                list.len()
            )),
            "a publication that includes every selected table",
            Some(format!(
                "CREATE PUBLICATION oo_publication FOR TABLE {}{with};",
                list.join(", ")
            )),
        ));
    }
    for t in &facts.tables {
        if t.exists
            && (t.replica_identity == "n" || (!t.has_primary_key && t.replica_identity == "d"))
        {
            out.push(need(
                RequirementKind::Feature,
                format!("replica_identity:{}", t.qualified),
                Some("DEFAULT without a primary key".into()),
                "a replica identity, so updates and deletes are published",
                Some(format!(
                    "ALTER TABLE {} REPLICA IDENTITY FULL;",
                    t.qualified
                )),
            ));
        }
    }
    if facts.in_recovery && facts.hot_standby_feedback.as_deref() != Some("on") {
        out.push(need(
            RequirementKind::Parameter,
            "hot_standby_feedback",
            facts.hot_standby_feedback.clone(),
            "on (logical decoding on a standby)",
            Some("ALTER SYSTEM SET hot_standby_feedback = on; SELECT pg_reload_conf();".into()),
        ));
    }
    if version::idle_slot_timeout(major)
        && let Some(value) = &facts.idle_replication_slot_timeout
        && value != "0"
    {
        out.push(need(
            RequirementKind::Retention,
            "idle_replication_slot_timeout",
            Some(value.clone()),
            "0 (disabled) or longer than the longest interval between sync cycles",
            Some(
                "ALTER SYSTEM SET idle_replication_slot_timeout = 0; SELECT pg_reload_conf();"
                    .into(),
            ),
        ));
    }
    out.extend(select_requirements(facts));
    state(out)
}

/// Grade capabilities and fill settings.
pub fn grade(facts: &ProbeFacts) -> ProbeReport {
    let major = facts.version.major;
    let mut caps = BTreeMap::new();
    if major < version::MIN_MAJOR {
        let reason = format!(
            "PostgreSQL {major} is below the supported floor {}",
            version::MIN_MAJOR
        );
        for cap in [
            Capability::Catalog,
            Capability::ChunkedSnapshot,
            Capability::Watermark,
            Capability::LogCdc,
            Capability::NativeChanges,
            Capability::PartitionRefresh,
            Capability::DigestDiff,
            Capability::ReplicaRead,
        ] {
            caps.insert(
                cap,
                CapabilityState::Unsupported {
                    reason: reason.clone(),
                },
            );
        }
    } else {
        caps.insert(
            Capability::Catalog,
            state(protected_read_requirements(facts)),
        );
        caps.insert(
            Capability::ChunkedSnapshot,
            state(protected_read_requirements(facts)),
        );
        caps.insert(
            Capability::DigestDiff,
            state(protected_read_requirements(facts)),
        );
        caps.insert(Capability::Watermark, CapabilityState::Available);
        caps.insert(Capability::LogCdc, log_cdc(facts));
        caps.insert(
            Capability::NativeChanges,
            CapabilityState::Unsupported {
                reason: "PostgreSQL has no native change table; use LogCdc (C1) or DigestDiff \
                         (C5)"
                    .into(),
            },
        );
        caps.insert(
            Capability::PartitionRefresh,
            CapabilityState::Unsupported {
                reason: "not offered for PostgreSQL in v0.2".into(),
            },
        );
        caps.insert(
            Capability::ReplicaRead,
            if facts.in_recovery {
                CapabilityState::Available
            } else {
                CapabilityState::Unsupported {
                    reason: "connected to a primary; point the source at a standby to read a \
                             replica"
                        .into(),
                }
            },
        );
    }
    let mut settings = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        settings.insert(k.to_owned(), v);
    };
    put("server_version", facts.version.raw.clone());
    put(
        "version_rules",
        version::pg_rules()
            .iter()
            .filter(|r| r.versions.contains(&facts.version))
            .map(|r| r.note.as_str())
            .collect::<Vec<_>>()
            .join("; "),
    );
    put("support_level", version::support_level(major).to_owned());
    put("in_recovery", facts.in_recovery.to_string());
    put("wal_level", facts.wal_level.clone());
    put(
        "max_replication_slots",
        facts.max_replication_slots.to_string(),
    );
    put("max_wal_senders", facts.max_wal_senders.to_string());
    put("max_connections", facts.max_connections.to_string());
    put("block_size", facts.block_size.to_string());
    put(
        "pgoutput_proto_version",
        version::pgoutput_proto_version(major).to_string(),
    );
    put(
        "no_key_strategy",
        if version::tid_range_scan(major) {
            "ctid_range_in_one_snapshot"
        } else {
            "single_read"
        }
        .to_owned(),
    );
    put(
        "watermark_function",
        if facts.in_recovery {
            "pg_last_wal_replay_lsn()"
        } else {
            "pg_current_wal_lsn()"
        }
        .to_owned(),
    );
    put(
        "activity_visibility",
        if facts.read_all_stats || facts.rolsuper {
            "all_sessions".to_owned()
        } else {
            format!(
                "own_role_only; GRANT pg_read_all_stats TO {}; lets the active-session pause see \
                 every session",
                facts.user
            )
        },
    );
    if let Some(v) = &facts.sync_replication_slots {
        put("sync_replication_slots", v.clone());
    }
    if let Some(v) = &facts.idle_replication_slot_timeout {
        put("idle_replication_slot_timeout", v.clone());
    }
    if let Some(v) = &facts.hot_standby_feedback {
        put("hot_standby_feedback", v.clone());
    }
    put(
        "connector.log_cdc",
        "graded for the instance; the C1 reader lands in S2".to_owned(),
    );
    ProbeReport {
        protocol: ENGINE_PROTOCOL.to_owned(),
        family: EngineFamily::Postgres,
        version: facts.version.clone(),
        capabilities: caps,
        settings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(raw: &str) -> ProbeFacts {
        ProbeFacts {
            version: EngineVersion::parse(EngineFamily::Postgres, raw).unwrap(),
            user: "oo_reader".into(),
            in_recovery: false,
            wal_level: "logical".into(),
            max_replication_slots: 10,
            max_wal_senders: 10,
            rolreplication: true,
            rolsuper: false,
            read_all_stats: true,
            hot_standby_feedback: Some("off".into()),
            sync_replication_slots: None,
            idle_replication_slot_timeout: None,
            max_connections: 100,
            block_size: 8192,
            publications: 1,
            tables: vec![TableFacts {
                qualified: "app.orders".into(),
                exists: true,
                schema_usage: true,
                select: true,
                published: true,
                has_primary_key: true,
                replica_identity: "d".into(),
                partitioned: false,
            }],
        }
    }

    fn names(report: &ProbeReport) -> Vec<String> {
        report
            .requirements()
            .iter()
            .map(|r| r.name.clone())
            .collect()
    }

    #[test]
    fn ready_source_is_available() {
        let r = grade(&facts("17.11"));
        for cap in [
            Capability::Catalog,
            Capability::ChunkedSnapshot,
            Capability::Watermark,
            Capability::LogCdc,
            Capability::DigestDiff,
        ] {
            assert_eq!(r.capabilities[&cap], CapabilityState::Available, "{cap:?}");
        }
        assert!(matches!(
            r.capabilities[&Capability::ReplicaRead],
            CapabilityState::Unsupported { .. }
        ));
        assert_eq!(r.settings["support_level"], "target");
        assert_eq!(r.settings["pgoutput_proto_version"], "4");
        assert_eq!(r.settings["no_key_strategy"], "ctid_range_in_one_snapshot");
    }

    #[test]
    fn missing_setup_yields_dba_statements() {
        let mut f = facts("13.23");
        f.wal_level = "replica".into();
        f.rolreplication = false;
        f.tables[0].select = false;
        f.tables[0].published = false;
        f.tables[0].has_primary_key = false;
        f.tables.push(TableFacts {
            qualified: "app.parts".into(),
            exists: true,
            schema_usage: false,
            select: true,
            published: false,
            has_primary_key: true,
            replica_identity: "d".into(),
            partitioned: true,
        });
        let r = grade(&f);
        assert_eq!(
            names(&r),
            [
                "select:app.orders",
                "usage:app",
                "wal_level",
                "replication",
                "publication",
                "replica_identity:app.orders",
            ]
        );
        let reqs = r.requirements();
        let by = |n: &str| reqs.iter().find(|r| r.name == n).unwrap();
        assert_eq!(
            by("replication").dba_statement.as_deref(),
            Some("ALTER ROLE oo_reader WITH REPLICATION;")
        );
        assert_eq!(
            by("publication").dba_statement.as_deref(),
            Some(
                "CREATE PUBLICATION oo_publication FOR TABLE app.orders, app.parts WITH \
                 (publish_via_partition_root = true);"
            )
        );
        assert_eq!(
            by("select:app.orders").dba_statement.as_deref(),
            Some("GRANT SELECT ON app.orders TO oo_reader;")
        );
        assert!(matches!(
            r.capabilities[&Capability::ChunkedSnapshot],
            CapabilityState::NeedsSetup { .. }
        ));
        assert_eq!(r.settings["no_key_strategy"], "single_read");
        for req in reqs {
            assert!(
                !req.dba_statement
                    .as_deref()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains("password")
            );
        }
    }

    #[test]
    fn standby_rules() {
        let mut f = facts("15.19");
        f.in_recovery = true;
        let r = grade(&f);
        assert!(matches!(
            r.capabilities[&Capability::LogCdc],
            CapabilityState::Unsupported { .. }
        ));
        assert_eq!(
            r.capabilities[&Capability::ReplicaRead],
            CapabilityState::Available
        );
        assert_eq!(r.settings["watermark_function"], "pg_last_wal_replay_lsn()");
        let mut f = facts("16.15");
        f.in_recovery = true;
        assert_eq!(names(&grade(&f)), ["hot_standby_feedback"]);
    }

    #[test]
    fn pg18_idle_slot_timeout_and_pg12_experimental() {
        let mut f = facts("18.6");
        f.idle_replication_slot_timeout = Some("1d".into());
        assert_eq!(names(&grade(&f)), ["idle_replication_slot_timeout"]);
        f.idle_replication_slot_timeout = Some("0".into());
        assert!(names(&grade(&f)).is_empty());
        let r = grade(&facts("12.22"));
        assert_eq!(r.settings["support_level"], "experimental");
        assert_eq!(r.settings["pgoutput_proto_version"], "1");
        let old = grade(&facts("11.22"));
        assert!(
            old.capabilities
                .values()
                .all(|s| matches!(s, CapabilityState::Unsupported { .. }))
        );
    }

    #[test]
    fn activity_visibility_hint() {
        let mut f = facts("17.11");
        f.read_all_stats = false;
        assert!(grade(&f).settings["activity_visibility"].contains("GRANT pg_read_all_stats"));
        assert!(names(&grade(&f)).contains(&"pg_read_all_stats".into()));
    }

    #[test]
    fn quoted_schema_and_replica_identity_nothing_are_not_misreported() {
        assert_eq!(schema_ident("\"a.b\".\"t\""), "\"a.b\"");
        assert_eq!(schema_ident("\"a\"\".b\".t"), "\"a\"\".b\"");
        let mut f = facts("17.11");
        f.tables[0].qualified = "\"a.b\".t".into();
        f.tables[0].schema_usage = false;
        f.tables[0].replica_identity = "n".into();
        let r = grade(&f);
        assert!(names(&r).contains(&"replica_identity:\"a.b\".t".into()));
        assert!(
            r.requirements().iter().any(|r| r.dba_statement.as_deref()
                == Some("GRANT USAGE ON SCHEMA \"a.b\" TO oo_reader;"))
        );
    }
}

pub(super) async fn observe(s: &mut crate::PgSession) -> Result<ProbeReport, crate::SourceError> {
    use crate::session::{self, cell_bool, cell_i64, cell_opt_string, cell_string};
    let r = session::fetch_control(s, sqlx::query(
        "SELECT quote_ident(current_user), pg_is_in_recovery(), current_setting('wal_level'), \
         current_setting('max_replication_slots')::int8, current_setting('max_wal_senders')::int8, \
         rolreplication, rolsuper, pg_has_role(current_user,'pg_read_all_stats','MEMBER'), \
         current_setting('hot_standby_feedback',true), current_setting('sync_replication_slots',true), \
         current_setting('idle_replication_slot_timeout',true), current_setting('max_connections')::int8, \
         current_setting('block_size')::int8, (SELECT count(*) FROM pg_publication) \
         FROM pg_roles WHERE rolname=current_user"
    )).await?;
    let mut f = ProbeFacts {
        version: EngineVersion::parse(EngineFamily::Postgres, &s.version_raw)?,
        user: cell_string(&r, 0)?,
        in_recovery: cell_bool(&r, 1)?,
        wal_level: cell_string(&r, 2)?,
        max_replication_slots: cell_i64(&r, 3)?,
        max_wal_senders: cell_i64(&r, 4)?,
        rolreplication: cell_bool(&r, 5)?,
        rolsuper: cell_bool(&r, 6)?,
        read_all_stats: cell_bool(&r, 7)?,
        hot_standby_feedback: cell_opt_string(&r, 8)?,
        sync_replication_slots: cell_opt_string(&r, 9)?,
        idle_replication_slot_timeout: cell_opt_string(&r, 10)?,
        max_connections: cell_i64(&r, 11)?,
        block_size: cell_i64(&r, 12)?,
        publications: cell_i64(&r, 13)?,
        tables: Vec::new(),
    };
    for t in s.selection.clone().tables() {
        let r = session::fetch_control(s, sqlx::query(
            "SELECT quote_ident($1)||'.'||quote_ident($2), c.oid IS NOT NULL, \
             COALESCE(has_schema_privilege(n.oid,'USAGE'),false), COALESCE(has_table_privilege(c.oid,'SELECT'),false), \
             EXISTS(SELECT 1 FROM pg_publication_tables p WHERE p.schemaname=$1 AND p.tablename=$2), \
             EXISTS(SELECT 1 FROM pg_index i WHERE i.indrelid=c.oid AND i.indisprimary), \
             COALESCE(c.relreplident::text,'n'), COALESCE(c.relkind='p',false) \
             FROM (SELECT 1) dummy LEFT JOIN pg_namespace n ON n.nspname=$1 \
             LEFT JOIN pg_class c ON c.relnamespace=n.oid AND c.relname=$2"
        ).bind(t.schema()).bind(t.table())).await?;
        f.tables.push(TableFacts {
            qualified: cell_string(&r, 0)?,
            exists: cell_bool(&r, 1)?,
            schema_usage: cell_bool(&r, 2)?,
            select: cell_bool(&r, 3)?,
            published: cell_bool(&r, 4)?,
            has_primary_key: cell_bool(&r, 5)?,
            replica_identity: cell_string(&r, 6)?,
            partitioned: cell_bool(&r, 7)?,
        });
    }
    Ok(grade(&f))
}
