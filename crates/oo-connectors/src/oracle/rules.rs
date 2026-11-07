//! Oracle version dispatch (REQ-ENG-01). Every version difference the engine
//! acts on is one row of [`version_rules`] and one field of [`Dialect`]; the
//! rest of the module reads the dialect, never the version number.
//!
//! Requirement builders return statements a DBA can run as is. They name the
//! reading account and objects, never a credential.

use crate::engine::{
    Capability, EngineFamily, EngineVersion, Requirement, RequirementKind, VersionRange,
    VersionRule,
};

/// LogMiner dictionary strategy. Only [`Dictionary::OnlineCatalog`] is enabled
/// (Plan D8): it reads the live data dictionary and writes nothing (R1).
/// `REDO_LOG_CATALOG` needs `DBMS_LOGMNR_D.BUILD`, which writes the dictionary
/// into the redo stream (R1w), so it is reported and never selected.
pub const DICTIONARY_STRATEGY: &str = "online_catalog";
pub const REDO_LOG_CATALOG_STATE: &str =
    "disabled: R1w, needs DBMS_LOGMNR_D.BUILD (source-side dictionary write)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Multitenant {
    /// 11g: no CDB architecture.
    NonCdbOnly,
    /// 12c-19c: CDB or non-CDB.
    Optional,
    /// 21c+: non-CDB desupported, every database is a PDB in a CDB.
    PdbOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContinuousMine {
    Available,
    /// 12.2-18c: still works, deprecated.
    Deprecated,
    /// 19c+: desupported; the reader maintains its own log file list.
    Desupported,
}

/// Behavior switches derived from the version table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dialect {
    /// `FETCH FIRST n ROWS ONLY` / `OFFSET` (12.1+); else ROWNUM wrapping.
    pub fetch_first: bool,
    /// `STANDARD_HASH` (12.1+); else `ORA_HASH` lanes.
    pub standard_hash: bool,
    /// `ALL_TAB_COLS.COLLATION` data-bound collation (12.2+).
    pub column_collation: bool,
    /// Identity columns exist (12.1+).
    pub identity_columns: bool,
    pub multitenant: Multitenant,
    pub continuous_mine: ContinuousMine,
    /// `LOGMINING` system privilege (12.1+); 11g needs `SELECT ANY TRANSACTION`.
    pub logmining_privilege: bool,
    /// Tablespaces LogMiner must skip in addition to SYSTEM/SYSAUX.
    pub logminer_excluded_tablespaces: &'static [&'static str],
}

impl Dialect {
    pub fn for_version(v: &EngineVersion) -> Self {
        let at_least = |major: u32, minor: u32| (v.major, v.minor) >= (major, minor);
        Self {
            fetch_first: at_least(12, 1),
            standard_hash: at_least(12, 1),
            column_collation: at_least(12, 2),
            identity_columns: at_least(12, 1),
            multitenant: if !at_least(12, 1) {
                Multitenant::NonCdbOnly
            } else if !at_least(21, 0) {
                Multitenant::Optional
            } else {
                Multitenant::PdbOnly
            },
            continuous_mine: if !at_least(12, 2) {
                ContinuousMine::Available
            } else if !at_least(19, 0) {
                ContinuousMine::Deprecated
            } else {
                ContinuousMine::Desupported
            },
            logmining_privilege: at_least(12, 1),
            logminer_excluded_tablespaces: if at_least(23, 0) {
                &["SYSTEM", "SYSAUX", "SYSAUX_TABLES", "SYSAUX_INDEXES"]
            } else {
                &["SYSTEM", "SYSAUX"]
            },
        }
    }

    pub fn continuous_mine_label(&self) -> &'static str {
        match self.continuous_mine {
            ContinuousMine::Available => "available",
            ContinuousMine::Deprecated => "deprecated",
            ContinuousMine::Desupported => "desupported",
        }
    }

    /// How the C1 reader learns which redo files to mine.
    pub fn log_list(&self) -> &'static str {
        match self.continuous_mine {
            ContinuousMine::Desupported => {
                "self_managed: V$ARCHIVED_LOG/V$LOG + DBMS_LOGMNR.ADD_LOGFILE"
            }
            _ => "continuous_mine (self_managed list also supported)",
        }
    }

    pub fn digest_algorithm(&self) -> &'static str {
        if self.standard_hash {
            "oracle-sha256-sum4x60/v2"
        } else {
            "oracle-orahash-sum4x32/v2"
        }
    }
}

type Row = (
    (u32, u32, u32),
    Option<(u32, u32, u32)>,
    Capability,
    &'static str,
);

const RULES: &[Row] = &[
    (
        (11, 0, 0),
        Some((12, 0, 0)),
        Capability::Catalog,
        "non-CDB only; no identity columns; no data-bound column collation",
    ),
    (
        (11, 0, 0),
        Some((12, 1, 0)),
        Capability::ChunkedSnapshot,
        "no FETCH FIRST/OFFSET: keyset pages and boundary probes use ROWNUM wrapping",
    ),
    (
        (11, 0, 0),
        Some((12, 1, 0)),
        Capability::DigestDiff,
        "no STANDARD_HASH: digest uses four seeded ORA_HASH 32-bit lanes",
    ),
    (
        (11, 0, 0),
        Some((12, 1, 0)),
        Capability::LogCdc,
        "LogMiner needs SELECT ANY TRANSACTION and EXECUTE on DBMS_LOGMNR; CONTINUOUS_MINE available",
    ),
    (
        (12, 1, 0),
        None,
        Capability::ChunkedSnapshot,
        "keyset pages use FETCH FIRST n ROWS ONLY; boundary probes use OFFSET",
    ),
    (
        (12, 1, 0),
        None,
        Capability::DigestDiff,
        "digest uses STANDARD_HASH SHA256, four 60-bit lanes summed",
    ),
    (
        (12, 1, 0),
        Some((21, 0, 0)),
        Capability::Catalog,
        "CDB or non-CDB; snapshot reads connect to the PDB service",
    ),
    (
        (12, 1, 0),
        None,
        Capability::LogCdc,
        "LOGMINING system privilege; in a CDB LogMiner runs at CDB$ROOT as a C## common user",
    ),
    (
        (12, 2, 0),
        None,
        Capability::Catalog,
        "ALL_TAB_COLS.COLLATION gives data-bound key collation",
    ),
    (
        (12, 2, 0),
        Some((19, 0, 0)),
        Capability::LogCdc,
        "CONTINUOUS_MINE deprecated",
    ),
    (
        (19, 0, 0),
        None,
        Capability::LogCdc,
        "CONTINUOUS_MINE desupported: reader keeps its own log list from V$ARCHIVED_LOG/V$LOG",
    ),
    (
        (21, 0, 0),
        None,
        Capability::Catalog,
        "non-CDB desupported: every database is a PDB",
    ),
    (
        (23, 0, 0),
        None,
        Capability::LogCdc,
        "new system tablespaces (SYSAUX_TABLES/SYSAUX_INDEXES when present) excluded from mining",
    ),
    (
        (11, 0, 0),
        None,
        Capability::LogCdc,
        "dictionary online_catalog only (R1); redo_log_catalog needs DBMS_LOGMNR_D.BUILD (R1w) and is disabled",
    ),
    (
        (11, 0, 0),
        None,
        Capability::ChunkedSnapshot,
        "AS OF SCN per chunk; ORA-01555 shrinks the page and retries; no-key tables use extent ROWID ranges inside one SCN",
    ),
];

/// The Oracle rows of the engine dispatch table.
pub fn version_rules() -> Vec<VersionRule> {
    RULES
        .iter()
        .map(|(from, until, capability, note)| VersionRule {
            family: EngineFamily::Oracle,
            versions: VersionRange::new(*from, *until),
            capability: *capability,
            note: (*note).to_owned(),
        })
        .collect()
}

fn need(
    kind: RequirementKind,
    name: impl Into<String>,
    observed: Option<String>,
    required: impl Into<String>,
    dba: Option<String>,
) -> Requirement {
    Requirement {
        kind,
        name: name.into(),
        observed,
        required: required.into(),
        dba_statement: dba,
    }
}

/// Quote an Oracle identifier for a DBA statement. Names come from the
/// catalog; a name with a quote is rejected upstream.
pub fn q(name: &str) -> String {
    format!("\"{name}\"")
}

pub fn select_grant(owner: &str, table: &str, user: &str) -> Requirement {
    need(
        RequirementKind::Privilege,
        format!("select:{owner}.{table}"),
        Some("missing".into()),
        "SELECT on the table",
        Some(format!(
            "GRANT SELECT ON {}.{} TO {};",
            q(owner),
            q(table),
            q(user)
        )),
    )
}

pub fn flashback_grant(owner: &str, table: &str, user: &str) -> Requirement {
    need(
        RequirementKind::Privilege,
        format!("flashback:{owner}.{table}"),
        Some("missing".into()),
        "FLASHBACK on the table (AS OF SCN); never FLASHBACK ANY TABLE",
        Some(format!(
            "GRANT FLASHBACK ON {}.{} TO {};",
            q(owner),
            q(table),
            q(user)
        )),
    )
}

pub fn view_grant(view: &str, user: &str, cdb: bool) -> Requirement {
    // V$X is a public synonym of SYS.V_$X; grants go on the V_$ view.
    let target = view.replacen("V$", "V_$", 1);
    need(
        RequirementKind::Privilege,
        format!("select:{view}"),
        Some("not readable".into()),
        format!("SELECT on {view}"),
        Some(format!(
            "GRANT SELECT ON SYS.{target} TO {}{};",
            q(user),
            if cdb { " CONTAINER=ALL" } else { "" }
        )),
    )
}

pub fn archivelog(observed: Option<String>) -> Requirement {
    need(
        RequirementKind::Parameter,
        "log_mode",
        observed,
        "ARCHIVELOG",
        Some(
            "SHUTDOWN IMMEDIATE; STARTUP MOUNT; ALTER DATABASE ARCHIVELOG; ALTER DATABASE OPEN;"
                .into(),
        ),
    )
}

pub fn supplemental_minimal(observed: Option<String>) -> Requirement {
    need(
        RequirementKind::Parameter,
        "supplemental_log_data_min",
        observed,
        "YES (or IMPLICIT)",
        Some("ALTER DATABASE ADD SUPPLEMENTAL LOG DATA;".into()),
    )
}

/// Per-table supplemental logging: key columns for keyed tables, all columns
/// for tables without a key.
pub fn supplemental_table(owner: &str, table: &str, keyed: bool) -> Requirement {
    let group = if keyed { "PRIMARY KEY" } else { "ALL" };
    need(
        RequirementKind::Parameter,
        format!("supplemental_log:{owner}.{table}"),
        Some("no log group".into()),
        format!("{group} COLUMNS supplemental log group"),
        Some(format!(
            "ALTER TABLE {}.{} ADD SUPPLEMENTAL LOG DATA ({group}) COLUMNS;",
            q(owner),
            q(table)
        )),
    )
}

pub fn execute_logmnr(user: &str, cdb: bool) -> Requirement {
    need(
        RequirementKind::Privilege,
        "execute:DBMS_LOGMNR",
        Some("missing".into()),
        "EXECUTE on SYS.DBMS_LOGMNR",
        Some(format!(
            "GRANT EXECUTE ON SYS.DBMS_LOGMNR TO {}{};",
            q(user),
            if cdb { " CONTAINER=ALL" } else { "" }
        )),
    )
}

pub fn logmining(d: &Dialect, user: &str, cdb: bool) -> Requirement {
    if d.logmining_privilege {
        need(
            RequirementKind::Privilege,
            "LOGMINING",
            Some("missing".into()),
            "LOGMINING system privilege",
            Some(format!(
                "GRANT LOGMINING TO {}{};",
                q(user),
                if cdb { " CONTAINER=ALL" } else { "" }
            )),
        )
    } else {
        need(
            RequirementKind::Privilege,
            "SELECT ANY TRANSACTION",
            Some("missing".into()),
            "SELECT ANY TRANSACTION (11g LogMiner prerequisite)",
            Some(format!("GRANT SELECT ANY TRANSACTION TO {};", q(user))),
        )
    }
}

/// LogMiner in a CDB runs at CDB$ROOT under a common user.
pub fn cdb_root_session(observed_container: &str, user: &str) -> Requirement {
    let common = if user.to_ascii_uppercase().starts_with("C##") {
        user.to_owned()
    } else {
        "C##OO_LOGMINER".to_owned()
    };
    need(
        RequirementKind::Feature,
        "logminer_session:CDB$ROOT",
        Some(format!("container={observed_container} user={user}")),
        "C## common user connected to CDB$ROOT",
        Some(format!(
            "CREATE USER {c} IDENTIFIED BY \"<set by DBA>\" CONTAINER=ALL; GRANT CREATE SESSION, SET CONTAINER, LOGMINING TO {c} CONTAINER=ALL;",
            c = q(&common)
        )),
    )
}

pub fn scn_source(user: &str) -> Requirement {
    need(
        RequirementKind::Privilege,
        "select:V$DATABASE",
        Some("not readable".into()),
        "SELECT on V$DATABASE (CURRENT_SCN) or EXECUTE on DBMS_FLASHBACK",
        Some(format!("GRANT SELECT ON SYS.V_$DATABASE TO {};", q(user))),
    )
}

pub fn extents_view(user: &str) -> Requirement {
    need(
        RequirementKind::Privilege,
        "select:DBA_EXTENTS",
        Some("not readable".into()),
        "SELECT on DBA_EXTENTS for ROWID ranges of tables without a key (ALL_EXTENTS does not exist)",
        Some(format!("GRANT SELECT ON SYS.DBA_EXTENTS TO {};", q(user))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(raw: &str) -> EngineVersion {
        EngineVersion::parse(EngineFamily::Oracle, raw).unwrap()
    }

    fn notes(raw: &str, cap: Capability) -> Vec<String> {
        let rules = version_rules();
        let ver = v(raw);
        VersionRule::applicable(&rules, EngineFamily::Oracle, &ver)
            .filter(|r| r.capability == cap)
            .map(|r| r.note.clone())
            .collect()
    }

    #[test]
    fn eleven_g_is_non_cdb_rownum_orahash_continuous_mine() {
        let d = Dialect::for_version(&v("11.2.0.2.0"));
        assert_eq!(d.multitenant, Multitenant::NonCdbOnly);
        assert!(!d.fetch_first && !d.standard_hash && !d.identity_columns);
        assert!(!d.column_collation && !d.logmining_privilege);
        assert_eq!(d.continuous_mine, ContinuousMine::Available);
        assert_eq!(d.digest_algorithm(), "oracle-orahash-sum4x32/v2");
        let n = notes("11.2.0.2.0", Capability::LogCdc).join("|");
        assert!(n.contains("CONTINUOUS_MINE available"));
        assert!(n.contains("online_catalog only"));
        assert!(!n.contains("C## common user"));
        assert!(
            notes("11.2.0.2.0", Capability::ChunkedSnapshot)
                .join("|")
                .contains("ROWNUM")
        );
    }

    #[test]
    fn twelve_to_nineteen_cdb_optional_and_continuous_mine_retires() {
        let d12 = Dialect::for_version(&v("12.1.0.2.0"));
        assert_eq!(d12.multitenant, Multitenant::Optional);
        assert_eq!(d12.continuous_mine, ContinuousMine::Available);
        assert!(d12.fetch_first && !d12.column_collation);
        let d18 = Dialect::for_version(&v("18.3.0.0.0"));
        assert_eq!(d18.continuous_mine, ContinuousMine::Deprecated);
        let d19 = Dialect::for_version(&v("19.25.0.0.0"));
        assert_eq!(d19.multitenant, Multitenant::Optional);
        assert_eq!(d19.continuous_mine, ContinuousMine::Desupported);
        assert!(d19.log_list().starts_with("self_managed"));
        let n = notes("19.3.0.0.0", Capability::LogCdc).join("|");
        assert!(n.contains("desupported") && n.contains("C## common user"));
        assert!(!n.contains("deprecated"));
    }

    #[test]
    fn twenty_one_and_23ai_are_pdb_only_23ai_excludes_new_tablespaces() {
        let d21 = Dialect::for_version(&v("21.3.0.0.0"));
        assert_eq!(d21.multitenant, Multitenant::PdbOnly);
        assert_eq!(d21.logminer_excluded_tablespaces, ["SYSTEM", "SYSAUX"]);
        let d23 = Dialect::for_version(&v("23.26.3.0.0"));
        assert_eq!(d23.multitenant, Multitenant::PdbOnly);
        assert!(d23.logminer_excluded_tablespaces.len() > 2);
        assert!(
            notes("23.26.3.0.0", Capability::Catalog)
                .join("|")
                .contains("non-CDB desupported")
        );
        assert!(
            notes("23.26.3.0.0", Capability::LogCdc)
                .join("|")
                .contains("system tablespaces")
        );
        assert!(
            !notes("21.3.0.0.0", Capability::LogCdc)
                .join("|")
                .contains("system tablespaces")
        );
    }

    #[test]
    fn statements_name_objects_never_credentials() {
        let d = Dialect::for_version(&v("19.3.0.0.0"));
        let reqs = [
            select_grant("APP", "T", "OO_READER"),
            flashback_grant("APP", "T", "OO_READER"),
            view_grant("V$ARCHIVED_LOG", "C##OO", true),
            logmining(&d, "C##OO", true),
            execute_logmnr("C##OO", true),
            cdb_root_session("FREEPDB1", "OO_READER"),
            supplemental_table("APP", "T", false),
        ];
        for r in &reqs {
            let s = r.dba_statement.as_deref().unwrap();
            assert!(s.ends_with(';'), "{s}");
            assert!(!s.to_ascii_lowercase().contains("password"), "{s}");
        }
        assert_eq!(
            reqs[2].dba_statement.as_deref(),
            Some("GRANT SELECT ON SYS.V_$ARCHIVED_LOG TO \"C##OO\" CONTAINER=ALL;")
        );
        assert!(
            reqs[5]
                .dba_statement
                .as_deref()
                .unwrap()
                .contains("\"C##OO_LOGMINER\"")
        );
        assert!(reqs[6].dba_statement.as_deref().unwrap().contains("(ALL)"));
        let eleven = logmining(&Dialect::for_version(&v("11.2.0.2.0")), "OO", false);
        assert_eq!(eleven.name, "SELECT ANY TRANSACTION");
    }
}
