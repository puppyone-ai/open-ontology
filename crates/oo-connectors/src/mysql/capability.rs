//! MySQL version dispatch table and capability grading for `probe()`.
//!
//! Every version difference the connector acts on is one row of [`RULES`].
//! [`Dialect::for_version`] folds the rows that apply to a server into the
//! statements and variables to use, and [`version_rules`] exports the same
//! rows as engine [`VersionRule`]s for `oo doctor`. Grading is a pure
//! function of the observed settings so it can be tested without a server.
//! Every `dba_statement` is something a DBA can run as is; none carries a
//! credential.

use std::collections::BTreeMap;

use crate::SourceError;
use crate::engine::{
    Capability, CapabilityState, ENGINE_PROTOCOL, EngineFamily, EngineVersion, ProbeReport,
    Requirement, RequirementKind, VersionRange, VersionRule,
};

/// Binlog retention the connector asks for: one week covers the longest
/// cycle interval plus recovery in the reference deployment.
pub const RETENTION_SECONDS: u64 = 7 * 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativePassword {
    /// Loaded and usable.
    Available,
    /// Still shipped, not loaded unless `mysql_native_password=ON` (8.4).
    DisabledByDefault,
    /// Removed from the server (9.0+).
    Removed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facet {
    DefaultAuth(&'static str),
    Native(NativePassword),
    /// Variable that names the default authentication plugin.
    AuthVariable(&'static str),
    BinlogDefaultOn(bool),
    /// Statement that prints the current binlog file and position.
    StatusStatement(&'static str),
    /// Retention variable and its unit in seconds.
    Retention(&'static str, u64),
    TaggedGtid,
    /// `max_execution_time` applies to read-only SELECT.
    ServerSelectTimeout,
    ReplicaStatus(&'static str),
    /// `information_schema.TABLES.TABLE_ROWS` is cached; estimates use EXPLAIN.
    StatsCached,
    DigestDiff,
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub versions: VersionRange,
    pub capability: Capability,
    pub facet: Facet,
    pub note: &'static str,
}

const fn r(
    from: (u32, u32, u32),
    until: Option<(u32, u32, u32)>,
    capability: Capability,
    facet: Facet,
    note: &'static str,
) -> Rule {
    Rule {
        versions: VersionRange::new(from, until),
        capability,
        facet,
        note,
    }
}

const V57: (u32, u32, u32) = (5, 7, 0);
const V80: (u32, u32, u32) = (8, 0, 0);
const V84: (u32, u32, u32) = (8, 4, 0);
const V90: (u32, u32, u32) = (9, 0, 0);

/// The MySQL dispatch table. Ranges are `[from, until)`.
pub const RULES: &[Rule] = &[
    r(
        V57,
        Some(V80),
        Capability::Catalog,
        Facet::DefaultAuth("mysql_native_password"),
        "default authentication plugin is mysql_native_password",
    ),
    r(
        V80,
        None,
        Capability::Catalog,
        Facet::DefaultAuth("caching_sha2_password"),
        "default authentication plugin is caching_sha2_password (TLS or RSA key exchange)",
    ),
    r(
        V57,
        Some(V84),
        Capability::Catalog,
        Facet::Native(NativePassword::Available),
        "mysql_native_password is loaded",
    ),
    r(
        V84,
        Some(V90),
        Capability::Catalog,
        Facet::Native(NativePassword::DisabledByDefault),
        "mysql_native_password is disabled by default (--mysql-native-password=ON loads it)",
    ),
    r(
        V90,
        None,
        Capability::Catalog,
        Facet::Native(NativePassword::Removed),
        "mysql_native_password is removed; caching_sha2_password only",
    ),
    r(
        V57,
        Some(V84),
        Capability::Catalog,
        Facet::AuthVariable("default_authentication_plugin"),
        "default plugin is read from @@default_authentication_plugin",
    ),
    r(
        V84,
        None,
        Capability::Catalog,
        Facet::AuthVariable("authentication_policy"),
        "default_authentication_plugin is removed; read @@authentication_policy",
    ),
    r(
        V57,
        Some(V80),
        Capability::LogCdc,
        Facet::BinlogDefaultOn(false),
        "binary log is off by default; log_bin and server_id need a restart",
    ),
    r(
        V80,
        None,
        Capability::LogCdc,
        Facet::BinlogDefaultOn(true),
        "binary log is on by default (ROW, FULL)",
    ),
    r(
        V57,
        Some((8, 2, 0)),
        Capability::Watermark,
        Facet::StatusStatement("SHOW MASTER STATUS"),
        "binlog position from SHOW MASTER STATUS",
    ),
    r(
        (8, 2, 0),
        None,
        Capability::Watermark,
        Facet::StatusStatement("SHOW BINARY LOG STATUS"),
        "SHOW BINARY LOG STATUS replaces SHOW MASTER STATUS (removed in 8.4)",
    ),
    r(
        (8, 3, 0),
        None,
        Capability::Watermark,
        Facet::TaggedGtid,
        "gtid_executed may contain tagged GTIDs UUID:TAG:N",
    ),
    r(
        V57,
        Some(V80),
        Capability::LogCdc,
        Facet::Retention("expire_logs_days", 86_400),
        "binlog retention is expire_logs_days (days)",
    ),
    r(
        V80,
        None,
        Capability::LogCdc,
        Facet::Retention("binlog_expire_logs_seconds", 1),
        "binlog retention is binlog_expire_logs_seconds; expire_logs_days is deprecated in 8.0 and removed in 8.4",
    ),
    r(
        (5, 7, 8),
        None,
        Capability::ChunkedSnapshot,
        Facet::ServerSelectTimeout,
        "max_execution_time bounds read-only SELECT on the server",
    ),
    r(
        V80,
        None,
        Capability::ChunkedSnapshot,
        Facet::StatsCached,
        "information_schema TABLE_ROWS is cached (information_schema_stats_expiry); row estimates use EXPLAIN",
    ),
    r(
        V57,
        Some((8, 0, 22)),
        Capability::ReplicaRead,
        Facet::ReplicaStatus("SHOW SLAVE STATUS"),
        "replica state from SHOW SLAVE STATUS",
    ),
    r(
        (8, 0, 22),
        None,
        Capability::ReplicaRead,
        Facet::ReplicaStatus("SHOW REPLICA STATUS"),
        "SHOW REPLICA STATUS replaces SHOW SLAVE STATUS",
    ),
    r(
        V57,
        None,
        Capability::DigestDiff,
        Facet::DigestDiff,
        "range digest is COUNT plus BIT_XOR over MD5 halves of CONCAT_WS with ISNULL flags",
    ),
];

/// Engine dispatch rows, in table order.
pub fn version_rules() -> Vec<VersionRule> {
    RULES
        .iter()
        .map(|rule| VersionRule {
            family: EngineFamily::Mysql,
            versions: rule.versions.clone(),
            capability: rule.capability,
            note: rule.note.to_owned(),
        })
        .collect()
}

/// Statements and variables for one server version, folded from [`RULES`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialect {
    pub default_auth: &'static str,
    pub native_password: NativePassword,
    pub auth_variable: &'static str,
    pub binlog_default_on: bool,
    pub status_statement: &'static str,
    pub retention_variable: &'static str,
    pub retention_unit_seconds: u64,
    pub tagged_gtid: bool,
    pub server_select_timeout: bool,
    pub stats_cached: bool,
    pub replica_status: &'static str,
    /// `SET PERSIST` exists (8.0+); 5.7 needs `SET GLOBAL` plus my.cnf.
    pub set_persist: bool,
}

impl Dialect {
    /// MySQL 5.7.0 and later. MariaDB and older servers are refused.
    pub fn for_version(version: &EngineVersion) -> Result<Self, SourceError> {
        if version.family != EngineFamily::Mysql
            || ["mariadb", "tidb", "oceanbase"]
                .iter()
                .any(|family| version.raw.to_ascii_lowercase().contains(family))
            || !matches!((version.major, version.minor), (5, 7) | (8, _) | (9, _))
        {
            return Err(SourceError::version("mysql_version_floor"));
        }
        let mut d = Self {
            default_auth: "",
            native_password: NativePassword::Available,
            auth_variable: "",
            binlog_default_on: false,
            status_statement: "",
            retention_variable: "",
            retention_unit_seconds: 1,
            tagged_gtid: false,
            server_select_timeout: false,
            stats_cached: false,
            replica_status: "",
            set_persist: version.major >= 8,
        };
        for rule in RULES.iter().filter(|r| r.versions.contains(version)) {
            match rule.facet {
                Facet::DefaultAuth(p) => d.default_auth = p,
                Facet::Native(n) => d.native_password = n,
                Facet::AuthVariable(v) => d.auth_variable = v,
                Facet::BinlogDefaultOn(on) => d.binlog_default_on = on,
                Facet::StatusStatement(s) => d.status_statement = s,
                Facet::Retention(v, unit) => {
                    d.retention_variable = v;
                    d.retention_unit_seconds = unit;
                }
                Facet::TaggedGtid => d.tagged_gtid = true,
                Facet::ServerSelectTimeout => d.server_select_timeout = true,
                Facet::StatsCached => d.stats_cached = true,
                Facet::ReplicaStatus(s) => d.replica_status = s,
                Facet::DigestDiff => {}
            }
        }
        if d.default_auth.is_empty() || d.status_statement.is_empty() {
            return Err(SourceError::version("mysql_version_rules"));
        }
        Ok(d)
    }

    /// Statement that makes a global setting survive a restart.
    fn set_global(&self, name: &str, value: &str) -> String {
        if self.set_persist {
            format!("SET PERSIST {name} = {value};")
        } else {
            format!("SET GLOBAL {name} = {value}; -- and add {name}={value} to my.cnf [mysqld]")
        }
    }
}

/// Privileges seen in `SHOW GRANTS FOR CURRENT_USER()`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grants {
    pub select: bool,
    pub replication_client: bool,
    pub replication_slave: bool,
}

impl Grants {
    /// Parse grant lines for `database`. Only global (`*.*`) or that
    /// database's (`db`.*) grants count.
    pub fn parse(lines: &[String], database: &str) -> Self {
        let mut g = Self::default();
        let db_scope = format!("`{database}`.*");
        for line in lines {
            let upper = line.to_ascii_uppercase();
            let Some(rest) = upper.strip_prefix("GRANT ") else {
                continue;
            };
            let Some((privs, _)) = rest.split_once(" ON ") else {
                continue;
            };
            let scope = line
                .split_once(" ON ")
                .and_then(|(_, s)| s.split_once(" TO "))
                .map(|(scope, _)| scope);
            let global = scope == Some("*.*");
            let on_db = scope == Some(db_scope.as_str());
            let all = privs.contains("ALL PRIVILEGES");
            let has = |p: &str| all || privs.split(',').any(|x| x.trim() == p);
            if (global || on_db) && has("SELECT") {
                g.select = true;
            }
            if global && has("REPLICATION CLIENT") {
                g.replication_client = true;
            }
            if global && has("REPLICATION SLAVE") {
                g.replication_slave = true;
            }
        }
        g
    }
}

/// Settings read by the probe. Values are settings, never secrets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observed {
    pub database: String,
    /// `'user'@'host'` from `CURRENT_USER()`, quoted for a GRANT.
    pub account: String,
    pub log_bin: bool,
    pub binlog_format: Option<String>,
    pub binlog_row_image: Option<String>,
    pub gtid_mode: Option<String>,
    pub server_id: Option<u64>,
    /// Retention in the dialect's variable, converted to seconds.
    pub retention_seconds: Option<u64>,
    pub default_auth_setting: Option<String>,
    /// Plugin of the connected account, when the server shows it.
    pub account_plugin: Option<String>,
    /// `mysql_native_password` is loaded and active.
    pub native_plugin_active: Option<bool>,
    pub grants: Grants,
    pub max_connections: Option<u64>,
    pub threads_connected: Option<u64>,
    pub max_execution_time_ms: Option<u64>,
    pub is_replica: bool,
    pub replica_preserve_commit_order: Option<bool>,
}

/// Quote `user@host` from `CURRENT_USER()` for a GRANT statement.
pub fn quote_account(current_user: &str) -> String {
    let (user, host) = current_user.rsplit_once('@').unwrap_or((current_user, "%"));
    let q = |s: &str| s.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{}'@'{}'", q(user), q(host))
}

fn req(
    kind: RequirementKind,
    name: &str,
    observed: Option<String>,
    required: &str,
    dba: Option<String>,
) -> Requirement {
    Requirement {
        kind,
        name: name.to_owned(),
        observed,
        required: required.to_owned(),
        dba_statement: dba,
    }
}

fn state(requirements: Vec<Requirement>) -> CapabilityState {
    if requirements.is_empty() {
        CapabilityState::Available
    } else {
        CapabilityState::NeedsSetup { requirements }
    }
}

/// Grade every capability from observed settings.
pub fn grade(version: &EngineVersion, dialect: &Dialect, o: &Observed) -> ProbeReport {
    let account = if o.account.is_empty() {
        "'<reader>'@'%'".to_owned()
    } else {
        o.account.clone()
    };
    let select = (!o.grants.select).then(|| {
        req(
            RequirementKind::Privilege,
            "select",
            Some("missing".into()),
            &format!("SELECT on `{}`.*", o.database),
            Some(format!("GRANT SELECT ON `{}`.* TO {account};", o.database)),
        )
    });
    let repl_client = (!o.grants.replication_client).then(|| {
        req(
            RequirementKind::Privilege,
            "replication_client",
            Some("missing".into()),
            "REPLICATION CLIENT on *.*",
            Some(format!("GRANT REPLICATION CLIENT ON *.* TO {account};")),
        )
    });
    let repl_slave = (!o.grants.replication_slave).then(|| {
        req(
            RequirementKind::Privilege,
            "replication_slave",
            Some("missing".into()),
            "REPLICATION SLAVE on *.*",
            Some(format!("GRANT REPLICATION SLAVE ON *.* TO {account};")),
        )
    });
    let log_bin = (!o.log_bin).then(|| {
        req(
            RequirementKind::Parameter,
            "log_bin",
            Some("OFF".into()),
            "ON",
            Some(if dialect.binlog_default_on {
                "-- log_bin is read-only: remove skip-log-bin / disable-log-bin from my.cnf [mysqld] and restart".into()
            } else {
                "-- log_bin is read-only: add log_bin=binlog and server_id=<unique non-zero> to my.cnf [mysqld] and restart".into()
            }),
        )
    });
    let gtid_transitional = o.gtid_mode.as_deref().filter(|m| {
        m.eq_ignore_ascii_case("OFF_PERMISSIVE") || m.eq_ignore_ascii_case("ON_PERMISSIVE")
    });
    let gtid = gtid_transitional.map(|m| {
        let mut statements = Vec::new();
        statements.push(dialect.set_global("enforce_gtid_consistency", "ON"));
        if m.eq_ignore_ascii_case("OFF_PERMISSIVE") {
            statements.push(dialect.set_global("gtid_mode", "ON_PERMISSIVE"));
        }
        statements.push(dialect.set_global("gtid_mode", "ON"));
        req(
            RequirementKind::Parameter,
            "gtid_mode",
            Some(m.to_owned()),
            "ON or OFF (not a transitional mode)",
            Some(statements.join("\n")),
        )
    });

    let mut caps = BTreeMap::new();
    caps.insert(Capability::Catalog, state(select.iter().cloned().collect()));
    caps.insert(
        Capability::ChunkedSnapshot,
        state(select.iter().cloned().collect()),
    );
    caps.insert(
        Capability::DigestDiff,
        state(select.iter().cloned().collect()),
    );

    // Both modes inspect replica channels before issuing a safe watermark.
    let gtid_on = o
        .gtid_mode
        .as_deref()
        .is_some_and(|m| m.eq_ignore_ascii_case("ON"));
    let mut wm = Vec::new();
    let replica_order =
        (o.is_replica && o.replica_preserve_commit_order != Some(true)).then(|| {
            let name = if (version.major, version.minor, version.patch) >= (8, 0, 26) {
                "replica_preserve_commit_order"
            } else {
                "slave_preserve_commit_order"
            };
            req(
                RequirementKind::Parameter,
                name,
                o.replica_preserve_commit_order
                    .map(|on| if on { "ON" } else { "OFF" }.into()),
                "ON for read-only replica watermarks",
                Some(dialect.set_global(name, "ON")),
            )
        });
    wm.extend(log_bin.clone());
    wm.extend(gtid.clone());
    wm.extend(replica_order.clone());
    if o.log_bin {
        wm.extend(repl_client.clone());
    }
    caps.insert(Capability::Watermark, state(wm));

    let mut cdc = Vec::new();
    cdc.extend(replica_order);
    cdc.extend(log_bin);
    if o.server_id == Some(0) {
        cdc.push(req(
            RequirementKind::Parameter,
            "server_id",
            Some("0".into()),
            "non-zero",
            Some(format!(
                "-- Example ID 731: confirm uniqueness in the replication topology before running.\n{}",
                dialect.set_global("server_id", "731")
            )),
        ));
    }
    if o.log_bin
        && !o
            .binlog_format
            .as_deref()
            .is_some_and(|f| f.eq_ignore_ascii_case("ROW"))
    {
        cdc.push(req(
            RequirementKind::Parameter,
            "binlog_format",
            o.binlog_format.clone(),
            "ROW",
            Some(dialect.set_global("binlog_format", "'ROW'")),
        ));
    }
    if o.log_bin
        && !o
            .binlog_row_image
            .as_deref()
            .is_some_and(|f| f.eq_ignore_ascii_case("FULL"))
    {
        cdc.push(req(
            RequirementKind::Parameter,
            "binlog_row_image",
            o.binlog_row_image.clone(),
            "FULL",
            Some(dialect.set_global("binlog_row_image", "'FULL'")),
        ));
    }
    cdc.extend(gtid);
    cdc.extend(repl_client);
    cdc.extend(repl_slave);
    // 0 means "never purge", which satisfies any retention.
    if o.log_bin
        && let Some(seconds) = o.retention_seconds
        && seconds != 0
        && seconds < RETENTION_SECONDS
    {
        let value = (RETENTION_SECONDS / dialect.retention_unit_seconds).to_string();
        cdc.push(req(
            RequirementKind::Retention,
            dialect.retention_variable,
            Some(format!("{seconds}s")),
            &format!("≥ {RETENTION_SECONDS}s (longest cycle interval + recovery)"),
            Some(dialect.set_global(dialect.retention_variable, &value)),
        ));
    }
    caps.insert(Capability::LogCdc, state(cdc));
    caps.insert(
        Capability::NativeChanges,
        CapabilityState::Unsupported {
            reason: "MySQL has no native change table; changes come from the binary log (C1)"
                .into(),
        },
    );
    caps.insert(
        Capability::PartitionRefresh,
        CapabilityState::Unsupported {
            reason: "partition refresh is not offered for MySQL in this release".into(),
        },
    );
    caps.insert(
        Capability::ReplicaRead,
        CapabilityState::Unsupported {
            reason: format!(
                "replica reads are not graded by this probe; replica state comes from {}",
                dialect.replica_status
            ),
        },
    );

    // An account on mysql_native_password cannot reconnect once the plugin
    // is disabled (8.4) or removed (9.x); grade it on Catalog.
    if o.account_plugin.as_deref() == Some("mysql_native_password")
        && (dialect.native_password != NativePassword::Available
            || o.native_plugin_active == Some(false))
    {
        let need = req(
            RequirementKind::Feature,
            "authentication_plugin",
            Some("mysql_native_password".into()),
            "caching_sha2_password",
            Some(format!(
                "ALTER USER {account} IDENTIFIED WITH caching_sha2_password BY RANDOM PASSWORD;"
            )),
        );
        let entry = caps
            .entry(Capability::Catalog)
            .or_insert(CapabilityState::Available);
        match entry {
            CapabilityState::NeedsSetup { requirements } => requirements.push(need),
            other => {
                *other = CapabilityState::NeedsSetup {
                    requirements: vec![need],
                }
            }
        }
    }

    let mut settings = BTreeMap::new();
    let mut set = |k: &str, v: Option<String>| {
        if let Some(v) = v {
            settings.insert(k.to_owned(), v);
        }
    };
    set("log_bin", Some(if o.log_bin { "ON" } else { "OFF" }.into()));
    set("binlog_format", o.binlog_format.clone());
    set("binlog_row_image", o.binlog_row_image.clone());
    set("gtid_mode", o.gtid_mode.clone());
    set("server_id", o.server_id.map(|v| v.to_string()));
    set(
        "binlog_retention_seconds",
        o.retention_seconds.map(|v| v.to_string()),
    );
    set(
        "retention_variable",
        Some(dialect.retention_variable.into()),
    );
    set("status_statement", Some(dialect.status_statement.into()));
    set(
        "watermark_kind",
        Some(
            if !o.log_bin {
                "none"
            } else if gtid_on {
                "gtid_set"
            } else {
                "file_pos"
            }
            .into(),
        ),
    );
    set("tagged_gtid", Some(dialect.tagged_gtid.to_string()));
    set("default_auth_plugin", Some(dialect.default_auth.into()));
    set(
        &format!("@@{}", dialect.auth_variable),
        o.default_auth_setting.clone(),
    );
    set("account_auth_plugin", o.account_plugin.clone());
    set(
        "native_password",
        Some(
            match (dialect.native_password, o.native_plugin_active) {
                (NativePassword::Removed, _) => "removed",
                (_, Some(true)) => "active",
                (_, Some(false)) => "inactive",
                (NativePassword::Available, None) => "available",
                (NativePassword::DisabledByDefault, None) => "disabled_by_default",
            }
            .into(),
        ),
    );
    set("max_connections", o.max_connections.map(|v| v.to_string()));
    set(
        "threads_connected",
        o.threads_connected.map(|v| v.to_string()),
    );
    set(
        "server_select_timeout",
        Some(dialect.server_select_timeout.to_string()),
    );
    set(
        "information_schema_stats_cached",
        Some(dialect.stats_cached.to_string()),
    );
    set("row_estimate_source", Some("explain".into()));
    ProbeReport {
        protocol: ENGINE_PROTOCOL.into(),
        family: EngineFamily::Mysql,
        version: version.clone(),
        capabilities: caps,
        settings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(raw: &str) -> EngineVersion {
        EngineVersion::parse(EngineFamily::Mysql, raw).unwrap()
    }

    fn ready(database: &str) -> Observed {
        Observed {
            database: database.into(),
            account: quote_account("oo_reader@%"),
            log_bin: true,
            binlog_format: Some("ROW".into()),
            binlog_row_image: Some("FULL".into()),
            gtid_mode: Some("ON".into()),
            server_id: Some(1),
            retention_seconds: Some(RETENTION_SECONDS),
            grants: Grants {
                select: true,
                replication_client: true,
                replication_slave: true,
            },
            ..Observed::default()
        }
    }

    #[test]
    fn dispatch_covers_the_four_lines() {
        let d57 = Dialect::for_version(&v("5.7.44-log")).unwrap();
        assert_eq!(d57.status_statement, "SHOW MASTER STATUS");
        assert_eq!(d57.retention_variable, "expire_logs_days");
        assert_eq!(d57.retention_unit_seconds, 86_400);
        assert_eq!(d57.default_auth, "mysql_native_password");
        assert!(!d57.binlog_default_on && !d57.tagged_gtid && !d57.set_persist);
        assert_eq!(d57.replica_status, "SHOW SLAVE STATUS");

        let d80 = Dialect::for_version(&v("8.0.44")).unwrap();
        assert_eq!(d80.status_statement, "SHOW MASTER STATUS");
        assert_eq!(d80.retention_variable, "binlog_expire_logs_seconds");
        assert_eq!(d80.default_auth, "caching_sha2_password");
        assert_eq!(d80.auth_variable, "default_authentication_plugin");
        assert_eq!(d80.native_password, NativePassword::Available);
        assert!(d80.binlog_default_on && d80.stats_cached && !d80.tagged_gtid);
        assert_eq!(d80.replica_status, "SHOW REPLICA STATUS");

        let d84 = Dialect::for_version(&v("8.4.7")).unwrap();
        assert_eq!(d84.status_statement, "SHOW BINARY LOG STATUS");
        assert_eq!(d84.auth_variable, "authentication_policy");
        assert_eq!(d84.native_password, NativePassword::DisabledByDefault);
        assert!(d84.tagged_gtid);

        let d9 = Dialect::for_version(&v("9.5.0")).unwrap();
        assert_eq!(d9.status_statement, "SHOW BINARY LOG STATUS");
        assert_eq!(d9.native_password, NativePassword::Removed);
        assert_eq!(d9.retention_variable, "binlog_expire_logs_seconds");

        assert_eq!(
            Dialect::for_version(&v("8.2.0")).unwrap().status_statement,
            "SHOW BINARY LOG STATUS"
        );
        assert!(
            !Dialect::for_version(&v("5.7.7"))
                .unwrap()
                .server_select_timeout
        );
        assert!(
            Dialect::for_version(&v("5.7.8"))
                .unwrap()
                .server_select_timeout
        );
    }

    #[test]
    fn refuses_mariadb_and_old_servers() {
        assert!(Dialect::for_version(&v("5.6.51")).is_err());
        assert!(Dialect::for_version(&v("10.11.8-MariaDB")).is_err());
        assert!(Dialect::for_version(&v("5.7.25-TiDB-v8.0")).is_err());
        assert!(Dialect::for_version(&v("10.0.0")).is_err());
        let maria = EngineVersion::parse(EngineFamily::Mariadb, "11.4.2").unwrap();
        assert!(Dialect::for_version(&maria).is_err());
    }

    #[test]
    fn exported_rules_keep_table_order_and_family() {
        let rules = version_rules();
        assert_eq!(rules.len(), RULES.len());
        let v84 = v("8.4.7");
        let applicable: Vec<_> = VersionRule::applicable(&rules, EngineFamily::Mysql, &v84)
            .map(|r| r.note.as_str())
            .collect();
        assert!(
            applicable
                .iter()
                .any(|n| n.contains("SHOW BINARY LOG STATUS"))
        );
        assert!(
            !applicable
                .iter()
                .any(|n| n.contains("expire_logs_days (days)"))
        );
    }

    #[test]
    fn a_ready_source_has_every_snapshot_and_log_capability() {
        let ver = v("8.0.44");
        let d = Dialect::for_version(&ver).unwrap();
        let report = grade(&ver, &d, &ready("oo_fixture"));
        for cap in [
            Capability::Catalog,
            Capability::ChunkedSnapshot,
            Capability::Watermark,
            Capability::LogCdc,
            Capability::DigestDiff,
        ] {
            assert_eq!(
                report.capabilities[&cap],
                CapabilityState::Available,
                "{cap:?}"
            );
        }
        assert!(report.requirements().is_empty());
        assert_eq!(report.settings["watermark_kind"], "gtid_set");
    }

    #[test]
    fn mysql57_defaults_produce_runnable_dba_statements() {
        let ver = v("5.7.44");
        let d = Dialect::for_version(&ver).unwrap();
        let o = Observed {
            database: "oo_fixture".into(),
            account: quote_account("oo_reader@%"),
            log_bin: false,
            gtid_mode: Some("OFF".into()),
            server_id: Some(0),
            retention_seconds: Some(0),
            grants: Grants {
                select: true,
                ..Grants::default()
            },
            ..Observed::default()
        };
        let report = grade(&ver, &d, &o);
        let names: Vec<_> = report
            .requirements()
            .iter()
            .map(|r| r.name.clone())
            .collect();
        assert_eq!(
            names,
            [
                "log_bin",
                "server_id",
                "replication_client",
                "replication_slave"
            ]
        );
        let all: Vec<_> = report
            .requirements()
            .iter()
            .filter_map(|r| r.dba_statement.clone())
            .collect();
        assert!(
            all.iter()
                .any(|s| s == "GRANT REPLICATION CLIENT ON *.* TO 'oo_reader'@'%';")
        );
        assert!(all.iter().any(|s| s.contains("log_bin=binlog")));
        assert!(
            all.iter()
                .any(|s| s.contains("SET GLOBAL server_id = 731;"))
        );
        assert!(!all.iter().any(|s| s.contains("= <")));
        assert_eq!(report.settings["watermark_kind"], "none");
        assert_eq!(
            report.capabilities[&Capability::Catalog],
            CapabilityState::Available
        );
    }

    #[test]
    fn row_format_retention_and_transitional_gtid_are_requirements() {
        let ver = v("8.4.7");
        let d = Dialect::for_version(&ver).unwrap();
        let mut o = ready("oo_fixture");
        o.binlog_format = Some("MIXED".into());
        o.binlog_row_image = Some("MINIMAL".into());
        o.gtid_mode = Some("ON_PERMISSIVE".into());
        o.retention_seconds = Some(86_400);
        let report = grade(&ver, &d, &o);
        let reqs = report.requirements();
        let by = |n: &str| {
            reqs.iter()
                .find(|r| r.name == n)
                .unwrap()
                .dba_statement
                .clone()
                .unwrap()
        };
        assert_eq!(by("binlog_format"), "SET PERSIST binlog_format = 'ROW';");
        assert_eq!(
            by("binlog_row_image"),
            "SET PERSIST binlog_row_image = 'FULL';"
        );
        assert_eq!(
            by("gtid_mode"),
            "SET PERSIST enforce_gtid_consistency = ON;\nSET PERSIST gtid_mode = ON;"
        );
        let mut off = o.clone();
        off.gtid_mode = Some("OFF_PERMISSIVE".into());
        let off_report = grade(&ver, &d, &off);
        assert!(
            off_report
                .requirements()
                .iter()
                .find(|r| r.name == "gtid_mode")
                .unwrap()
                .dba_statement
                .as_ref()
                .unwrap()
                .contains("gtid_mode = ON_PERMISSIVE;")
        );
        assert_eq!(
            by("binlog_expire_logs_seconds"),
            "SET PERSIST binlog_expire_logs_seconds = 604800;"
        );
        assert!(matches!(
            report.capabilities[&Capability::Watermark],
            CapabilityState::NeedsSetup { .. }
        ));

        let v57 = v("5.7.44");
        let d57 = Dialect::for_version(&v57).unwrap();
        let mut o = ready("oo_fixture");
        o.retention_seconds = Some(86_400);
        let report = grade(&v57, &d57, &o);
        assert_eq!(
            report.requirements()[0].dba_statement.as_deref(),
            Some(
                "SET GLOBAL expire_logs_days = 7; -- and add expire_logs_days=7 to my.cnf [mysqld]"
            )
        );
    }

    #[test]
    fn file_position_watermark_needs_replication_client() {
        let ver = v("9.5.0");
        let d = Dialect::for_version(&ver).unwrap();
        let mut o = ready("oo_fixture");
        o.gtid_mode = Some("OFF".into());
        o.grants.replication_client = false;
        let report = grade(&ver, &d, &o);
        assert_eq!(report.settings["watermark_kind"], "file_pos");
        match &report.capabilities[&Capability::Watermark] {
            CapabilityState::NeedsSetup { requirements } => {
                assert_eq!(requirements[0].name, "replication_client")
            }
            other => panic!("{other:?}"),
        }
        o.gtid_mode = Some("ON".into());
        let report = grade(&ver, &d, &o);
        assert!(matches!(
            &report.capabilities[&Capability::Watermark],
            CapabilityState::NeedsSetup { requirements }
                if requirements.iter().any(|r| r.name == "replication_client")
        ));
    }

    #[test]
    fn replica_commit_order_is_a_versioned_requirement() {
        for raw in ["5.7.44", "8.0.25", "8.0.44", "8.4.7", "9.5.0"] {
            let ver = v(raw);
            let dialect = Dialect::for_version(&ver).unwrap();
            let mut observed = ready("fixture");
            observed.is_replica = true;
            for preserve in [None, Some(false)] {
                observed.replica_preserve_commit_order = preserve;
                let report = grade(&ver, &dialect, &observed);
                for capability in [Capability::Watermark, Capability::LogCdc] {
                    assert!(matches!(
                        &report.capabilities[&capability],
                        CapabilityState::NeedsSetup { requirements }
                            if requirements.iter().any(|r| r.name.ends_with("preserve_commit_order"))
                    ));
                }
            }
            observed.replica_preserve_commit_order = Some(true);
            let report = grade(&ver, &dialect, &observed);
            assert_eq!(
                report.capabilities[&Capability::Watermark],
                CapabilityState::Available
            );
        }
    }

    #[test]
    fn native_password_accounts_are_flagged_from_84() {
        for (raw, flagged) in [("8.0.44", false), ("8.4.7", true), ("9.5.0", true)] {
            let ver = v(raw);
            let d = Dialect::for_version(&ver).unwrap();
            let mut o = ready("oo_fixture");
            o.account_plugin = Some("mysql_native_password".into());
            let report = grade(&ver, &d, &o);
            let has = report
                .requirements()
                .iter()
                .any(|r| r.name == "authentication_plugin");
            assert_eq!(has, flagged, "{raw}");
        }
    }

    #[test]
    fn no_dba_statement_carries_a_credential() {
        let ver = v("8.4.7");
        let d = Dialect::for_version(&ver).unwrap();
        let o = Observed {
            database: "oo_fixture".into(),
            account: quote_account("oo_reader@%"),
            account_plugin: Some("mysql_native_password".into()),
            ..Observed::default()
        };
        for r in grade(&ver, &d, &o).requirements() {
            let s = r
                .dba_statement
                .clone()
                .unwrap_or_default()
                .to_ascii_uppercase();
            assert!(!s.contains("IDENTIFIED BY '"), "{s}");
            assert!(!s.contains("PASSWORD '"), "{s}");
        }
    }

    #[test]
    fn grants_are_scoped_to_global_or_the_database() {
        let lines = [
            "GRANT USAGE ON *.* TO `oo_reader`@`%`".to_string(),
            "GRANT SELECT ON `other`.* TO `oo_reader`@`%`".to_string(),
        ];
        assert_eq!(Grants::parse(&lines, "oo_fixture"), Grants::default());
        let lines = [
            "GRANT REPLICATION SLAVE, REPLICATION CLIENT ON *.* TO `oo_reader`@`%`".to_string(),
            "GRANT SELECT ON `oo_fixture`.* TO `oo_reader`@`%`".to_string(),
        ];
        assert_eq!(
            Grants::parse(&lines, "oo_fixture"),
            Grants {
                select: true,
                replication_client: true,
                replication_slave: true
            }
        );
        let all =
            ["GRANT ALL PRIVILEGES ON *.* TO 'root'@'localhost' WITH GRANT OPTION".to_string()];
        assert!(Grants::parse(&all, "x").replication_slave);
    }
}
