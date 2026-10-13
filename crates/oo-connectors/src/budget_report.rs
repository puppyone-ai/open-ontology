//! Versioned budget evidence for one connected session.
//!
//! [`BUDGET_PROTOCOL`] is a separate document. [`crate::PROTOCOL`] stays
//! `oo.source.pgsql/v1` and does not grow budget fields. Limits are the
//! session [`PgBudget`] and the settings that session applied. This module
//! does not invent a price, a credit, or a server row cap.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::PgBudget;
use crate::error::SourceError;
use crate::ident::hex_bytes;
use crate::model::{AccessReport, BlockReason, FactStatus, RoleAttributes, VersionFact};
use crate::session::{self, PgSession};
use crate::sql;

/// Frozen protocol name for the budget evidence document.
pub const BUDGET_PROTOCOL: &str = "oo.source.pgsql.budget/v1";

/// In-process driver kind. This is not a sidecar.
pub const DRIVER_KIND: &str = "in_process";

/// Patched sqlx PostgreSQL driver name.
pub const DRIVER_NAME: &str = "sqlx-postgres";

/// sqlx version this crate links.
pub const DRIVER_VERSION: &str = "0.9.0";

/// sqlx-core TLS feature this crate enables.
pub const DRIVER_FEATURES: &str = "_tls-rustls-ring-no-roots";

const FOREIGN_PRODUCTS: &[&str] = &[
    "kingbase",
    "opengauss",
    "polardb",
    "mysql",
    "mariadb",
    "oracle",
    "vastbase",
    "highgo",
];

/// SHA-256 of `name`, `version`, and `features` joined by NUL bytes.
pub fn driver_digest() -> String {
    let mut hasher = Sha256::new();
    hasher.update(DRIVER_NAME.as_bytes());
    hasher.update([0]);
    hasher.update(DRIVER_VERSION.as_bytes());
    hasher.update([0]);
    hasher.update(DRIVER_FEATURES.as_bytes());
    hex_bytes(&hasher.finalize())
}

/// Access report and budget evidence from one `inspect_budget` call.
///
/// `access_bytes` and `evidence_bytes` are the exact documents. Each is
/// compact JSON plus one trailing newline. Callers hash and forward those
/// bytes. Re-serializing the structs is a different document.
pub struct BudgetInspection {
    /// `oo.source.pgsql/v1` report from the same session.
    pub access_report: AccessReport,
    /// `oo.source.pgsql.budget/v1` evidence.
    pub evidence: BudgetEvidence,
    /// Exact access-report bytes.
    pub access_bytes: Vec<u8>,
    /// Exact evidence bytes.
    pub evidence_bytes: Vec<u8>,
}

/// Budget evidence. Field order is the serialized order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetEvidence {
    /// [`BUDGET_PROTOCOL`].
    pub protocol: String,
    /// `oo.config/v1` when the access report carries that boundary.
    pub core_contract: String,
    /// Trusted source id.
    pub source_id: String,
    /// Secret reference. Not the secret value.
    pub secret_ref: String,
    /// Hash of the companion access-report bytes.
    pub companion: Companion,
    /// Principal copied from this session.
    pub identity: BudgetIdentity,
    /// TLS facts copied from the access report.
    pub tls: BudgetTls,
    /// Version facts copied from the access report.
    pub version: BudgetVersion,
    /// Computed driver identity.
    pub driver: DriverIdentity,
    /// Always `none`. A local-pg label is a retag.
    pub shadow: String,
    /// `postgres-native` or `unknown`.
    pub compat_mode: String,
    /// `known` or `unknown`.
    pub compat_status: String,
    /// `postgres-17-native` or `unknown`.
    pub target_id: String,
    /// Copied from the access report.
    pub registration_allowed: bool,
    /// Copied from the access report.
    pub block_reasons: Vec<BlockReason>,
    /// Privilege facts for the selected tables.
    pub permission: BudgetPermission,
    /// Guards this session applied, and the values `pg_settings` showed.
    pub settings: ObservedSettings,
    /// Rows, Bytes, and WallClock, in that order.
    pub bounds: Vec<BudgetBound>,
    /// OLTP accounting. Estimates stay unknown. Billing stays none.
    pub cost_model: OltpCostModel,
    /// Fail-closed facts derived from this document.
    pub blocks: Vec<BudgetBlock>,
}

/// Companion access report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Companion {
    /// `oo.source.pgsql/v1`.
    pub protocol: String,
    /// Lowercase hex SHA-256 of the exact access bytes.
    pub sha256: String,
}

/// Session principal recorded for admission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetIdentity {
    /// `known` or `unknown`.
    pub status: String,
    /// `current_user`.
    pub current_user: String,
    /// `session_user`.
    pub session_user: String,
    /// `current_database`.
    pub database: String,
    /// `session_user` equals `current_user`.
    pub aligned: bool,
}

/// TLS facts. Positive admission needs an observed verify-full session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetTls {
    /// Driver mode.
    pub mode: String,
    /// Trust anchor class.
    pub trust: String,
    /// Hostname verification flag.
    pub hostname_verification: bool,
    /// `pg_stat_ssl` observation.
    pub server_observation: String,
    /// Negotiated version, or `unknown`.
    pub negotiated_version: String,
}

/// Server version facts used for native compatibility.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetVersion {
    /// `server_version_num`.
    pub server_version_num: i64,
    /// `server_version`.
    pub server_version: String,
    /// Major.
    pub major: i64,
    /// Minor.
    pub minor: i64,
    /// `tested`, `untested`, `below_minimum`, or `unknown`.
    pub admission: String,
}

/// Driver identity. `digest` is [`driver_digest`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverIdentity {
    /// [`DRIVER_KIND`].
    pub kind: String,
    /// [`DRIVER_NAME`].
    pub name: String,
    /// [`DRIVER_VERSION`].
    pub version: String,
    /// [`DRIVER_FEATURES`].
    pub features: String,
    /// [`driver_digest`].
    pub digest: String,
}

/// One selected table's privilege answers. `None` is SQL NULL, not false.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectPrivileges {
    /// Schema name.
    pub schema: String,
    /// Table name.
    pub table: String,
    /// Database `CONNECT`.
    pub connect: Option<bool>,
    /// Database `TEMP`.
    pub temp: Option<bool>,
    /// Database `CREATE`.
    pub database_create: Option<bool>,
    /// Schema `USAGE`.
    pub usage: Option<bool>,
    /// Schema `CREATE`.
    pub schema_create: Option<bool>,
    /// Table `SELECT`.
    pub select: Option<bool>,
    /// Table `INSERT`.
    pub insert: Option<bool>,
    /// Table `UPDATE`.
    pub update: Option<bool>,
    /// Table `DELETE`.
    pub delete: Option<bool>,
    /// Table `TRUNCATE`.
    pub truncate: Option<bool>,
    /// Table `REFERENCES`.
    pub references: Option<bool>,
    /// Table `TRIGGER`.
    pub trigger: Option<bool>,
}

/// Permission conclusion. `known` means the facts were collected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetPermission {
    /// `known` or `unknown`.
    pub status: String,
    /// True only when every collected fact is read-only.
    pub read_only: bool,
    /// One entry per selected table. Empty when nothing was selected.
    pub objects: Vec<ObjectPrivileges>,
}

/// One `pg_settings` row next to the value this session configured.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedSetting {
    /// Setting name.
    pub name: String,
    /// `known` or `unknown`.
    pub status: String,
    /// Value this session configured, when it configured one.
    pub configured: Option<String>,
    /// `pg_settings.setting`, when a row came back.
    pub setting: Option<String>,
    /// `pg_settings.unit`, when a row came back. Empty string is an empty unit.
    pub unit: Option<String>,
}

/// Session guards and client caps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedSettings {
    /// `statement_timeout`.
    pub statement_timeout: ObservedSetting,
    /// `lock_timeout`.
    pub lock_timeout: ObservedSetting,
    /// `idle_in_transaction_session_timeout`.
    pub idle_in_transaction_session_timeout: ObservedSetting,
    /// `default_transaction_read_only`.
    pub default_transaction_read_only: ObservedSetting,
    /// Client connect timeout in milliseconds.
    pub connect_timeout_ms: u64,
    /// The client connect timeout is enforced here.
    pub connect_timeout_client_enforced: bool,
    /// The server does not receive the connect timeout.
    pub connect_timeout_server_enforced: bool,
    /// Client row-rate window. This is [`session::MAX_ROWS_PER_SEC`].
    pub row_rate_per_sec: u32,
    /// The row-rate window is enforced here.
    pub row_rate_client_enforced: bool,
    /// The server does not enforce this row-rate window.
    pub row_rate_server_enforced: bool,
}

/// One budget unit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetBound {
    /// `Rows`, `Bytes`, or `WallClock`.
    pub unit: String,
    /// Configured limit. Rows and bytes are counts. WallClock is milliseconds.
    pub configured_limit: u64,
    /// `known` when the limit is the session budget.
    pub configured_status: String,
    /// True when this process enforces the limit.
    pub client_enforced: bool,
    /// True only for a WallClock limit that `pg_settings` echoed.
    pub server_enforced: bool,
    /// Server setting name, or null when the server does not enforce the unit.
    pub server_setting: Option<String>,
    /// Observed server value, or null when there is no comparable number.
    pub server_observed: Option<u64>,
    /// `known`, `mismatch`, `unknown`, or `not_applicable`.
    pub server_status: String,
}

/// OLTP accounting. This is not a cloud price.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OltpCostModel {
    /// `oltp_accounting`.
    pub family: String,
    /// `Rows`, `Bytes`, `WallClock`.
    pub units: Vec<String>,
    /// Always `unknown`. A hard cap does not make an estimate known.
    pub estimate_confidence: String,
    /// Always `not_collected`.
    pub estimate_basis: String,
    /// Always `none`.
    pub billing: String,
}

/// One fail-closed fact derived from the evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetBlock {
    /// Stable code.
    pub code: String,
    /// Stable fact.
    pub fact: String,
}

struct SettingRow {
    name: String,
    setting: String,
    unit: String,
}

impl PgSession {
    /// Read the access report, the session settings, and the selected privileges.
    ///
    /// A SQL or budget error returns [`SourceError`] and no document. The
    /// evidence is produced only after every read succeeds.
    pub async fn inspect_budget(&mut self) -> Result<BudgetInspection, SourceError> {
        self.ensure_open()?;
        let access = self.inspect_access().await?;
        let settings = self.read_budget_settings().await?;
        let privileges = self.read_budget_privileges().await?;
        assemble_inspection(AssembleIn {
            budget: self.budget,
            access,
            current_user: self.current_user.clone(),
            session_user: self.session_user.clone(),
            database: self.database_name.clone(),
            aligned: self.identity_aligned,
            selected: self
                .selection
                .tables()
                .iter()
                .map(|table| (table.schema().to_string(), table.table().to_string()))
                .collect(),
            settings,
            privileges,
        })
    }

    async fn read_budget_settings(&mut self) -> Result<Vec<SettingRow>, SourceError> {
        if self.budget.max_rows() < 4 {
            return Err(SourceError::budget("max_rows"));
        }
        let names = vec![
            "default_transaction_read_only".to_string(),
            "idle_in_transaction_session_timeout".to_string(),
            "lock_timeout".to_string(),
            "statement_timeout".to_string(),
        ];
        let admitted = self.admit_rows(session::Admit::Exact {
            wanted: 4,
            natural: true,
        })?;
        self.arm_rows(admitted.limit);
        let rows = session::fetch_all(self, sqlx::query(sql::BUDGET_SETTINGS).bind(&names)).await?;
        if rows.len() > 4 {
            return Err(SourceError::catalog("budget_settings"));
        }
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let name = session::cell_string(row, 0)?;
            if out.iter().any(|item: &SettingRow| item.name == name) {
                return Err(SourceError::catalog("budget_settings"));
            }
            out.push(SettingRow {
                name,
                setting: session::cell_string(row, 1)?,
                unit: session::cell_string(row, 2)?,
            });
        }
        Ok(out)
    }

    async fn read_budget_privileges(&mut self) -> Result<Vec<ObjectPrivileges>, SourceError> {
        let selected = self.selection.tables();
        if selected.is_empty() {
            return Ok(Vec::new());
        }
        let wanted = u32::try_from(selected.len()).map_err(|_| SourceError::budget("max_rows"))?;
        if wanted > self.budget.max_rows() {
            return Err(SourceError::budget("max_rows"));
        }
        let schemas: Vec<String> = selected
            .iter()
            .map(|table| table.schema().to_string())
            .collect();
        let tables: Vec<String> = selected
            .iter()
            .map(|table| table.table().to_string())
            .collect();
        let admitted = self.admit_rows(session::Admit::Exact {
            wanted,
            natural: true,
        })?;
        self.arm_rows(admitted.limit);
        let rows = session::fetch_all(
            self,
            sqlx::query(sql::BUDGET_PRIVILEGES)
                .bind(&schemas)
                .bind(&tables)
                .bind(i64::from(wanted)),
        )
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            out.push(ObjectPrivileges {
                schema: session::cell_string(row, 0)?,
                table: session::cell_string(row, 1)?,
                connect: session::cell_opt_bool(row, 2)?,
                temp: session::cell_opt_bool(row, 3)?,
                database_create: session::cell_opt_bool(row, 4)?,
                usage: session::cell_opt_bool(row, 5)?,
                schema_create: session::cell_opt_bool(row, 6)?,
                select: session::cell_opt_bool(row, 7)?,
                insert: session::cell_opt_bool(row, 8)?,
                update: session::cell_opt_bool(row, 9)?,
                delete: session::cell_opt_bool(row, 10)?,
                truncate: session::cell_opt_bool(row, 11)?,
                references: session::cell_opt_bool(row, 12)?,
                trigger: session::cell_opt_bool(row, 13)?,
            });
        }
        Ok(out)
    }
}

struct AssembleIn {
    budget: PgBudget,
    access: AccessReport,
    current_user: String,
    session_user: String,
    database: String,
    aligned: bool,
    selected: Vec<(String, String)>,
    settings: Vec<SettingRow>,
    privileges: Vec<ObjectPrivileges>,
}

fn assemble_inspection(input: AssembleIn) -> Result<BudgetInspection, SourceError> {
    let access_bytes = document_bytes(&input.access)?;
    let objects = align_privileges(&input.selected, &input.privileges);
    let permission = judge_permission(&input.access, objects);
    let statement_ms = millis(input.budget.statement_timeout())?;
    let lock_ms = millis(input.budget.lock_timeout())?;
    let connect_ms = millis(input.budget.connect_timeout())?;
    let settings = observed_settings(&input.settings, statement_ms, lock_ms, connect_ms);
    let bounds = vec![
        client_bound("Rows", u64::from(input.budget.max_rows())),
        client_bound("Bytes", input.budget.max_bytes()),
        wall_bound(statement_ms, &settings.statement_timeout),
    ];
    let native = native_version(&input.access.version);
    let mut evidence = BudgetEvidence {
        protocol: BUDGET_PROTOCOL.to_string(),
        core_contract: input.access.core_contract.clone(),
        source_id: input.access.source_id.clone(),
        secret_ref: input.access.secret_ref.clone(),
        companion: Companion {
            protocol: crate::PROTOCOL.to_string(),
            sha256: sha256_hex(&access_bytes),
        },
        identity: build_identity(
            &input.access,
            &input.current_user,
            &input.session_user,
            &input.database,
            input.aligned,
        ),
        tls: BudgetTls {
            mode: input.access.tls.mode.clone(),
            trust: input.access.tls.trust.clone(),
            hostname_verification: input.access.tls.hostname_verification,
            server_observation: input.access.tls.server_observation.clone(),
            negotiated_version: input.access.tls.negotiated_version.clone(),
        },
        version: BudgetVersion {
            server_version_num: input.access.version.server_version_num,
            server_version: input.access.version.server_version.clone(),
            major: input.access.version.major,
            minor: input.access.version.minor,
            admission: input.access.version.admission.clone(),
        },
        driver: DriverIdentity {
            kind: DRIVER_KIND.to_string(),
            name: DRIVER_NAME.to_string(),
            version: DRIVER_VERSION.to_string(),
            features: DRIVER_FEATURES.to_string(),
            digest: driver_digest(),
        },
        shadow: "none".to_string(),
        compat_mode: if native {
            "postgres-native".to_string()
        } else {
            "unknown".to_string()
        },
        compat_status: if native {
            "known".to_string()
        } else {
            "unknown".to_string()
        },
        target_id: if native {
            "postgres-17-native".to_string()
        } else {
            "unknown".to_string()
        },
        registration_allowed: input.access.registration_allowed,
        block_reasons: input.access.block_reasons.clone(),
        permission,
        settings,
        bounds,
        cost_model: OltpCostModel {
            family: "oltp_accounting".to_string(),
            units: vec![
                "Rows".to_string(),
                "Bytes".to_string(),
                "WallClock".to_string(),
            ],
            estimate_confidence: "unknown".to_string(),
            estimate_basis: "not_collected".to_string(),
            billing: "none".to_string(),
        },
        blocks: Vec::new(),
    };
    evidence.blocks = derive_blocks(&evidence);
    let evidence_bytes = document_bytes(&evidence)?;
    Ok(BudgetInspection {
        access_report: input.access,
        evidence,
        access_bytes,
        evidence_bytes,
    })
}

fn document_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, SourceError> {
    let mut bytes =
        serde_json::to_vec(value).map_err(|_| SourceError::protocol("budget_encode"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_bytes(&Sha256::digest(bytes))
}

fn millis(value: std::time::Duration) -> Result<u64, SourceError> {
    let text = session::duration_ms(value)?;
    text.parse().map_err(|_| SourceError::config("timeout"))
}

fn align_privileges(
    selected: &[(String, String)],
    rows: &[ObjectPrivileges],
) -> Vec<ObjectPrivileges> {
    if selected.is_empty() {
        return Vec::new();
    }
    let exact = rows.len() == selected.len()
        && selected.iter().all(|(schema, table)| {
            rows.iter()
                .filter(|row| row.schema == *schema && row.table == *table)
                .count()
                == 1
        });
    if !exact {
        return selected
            .iter()
            .map(|(schema, table)| null_object(schema, table))
            .collect();
    }
    selected
        .iter()
        .map(|(schema, table)| {
            rows.iter()
                .find(|row| row.schema == *schema && row.table == *table)
                .cloned()
                .unwrap_or_else(|| null_object(schema, table))
        })
        .collect()
}

fn null_object(schema: &str, table: &str) -> ObjectPrivileges {
    ObjectPrivileges {
        schema: schema.to_string(),
        table: table.to_string(),
        connect: None,
        temp: None,
        database_create: None,
        usage: None,
        schema_create: None,
        select: None,
        insert: None,
        update: None,
        delete: None,
        truncate: None,
        references: None,
        trigger: None,
    }
}

fn judge_permission(access: &AccessReport, objects: Vec<ObjectPrivileges>) -> BudgetPermission {
    if objects.is_empty() || objects.iter().any(privilege_incomplete) || access_unknown(access) {
        return BudgetPermission {
            status: "unknown".to_string(),
            read_only: false,
            objects,
        };
    }
    let read_only = objects.iter().all(object_readonly) && access_readonly(access);
    BudgetPermission {
        status: "known".to_string(),
        read_only,
        objects,
    }
}

fn privilege_incomplete(object: &ObjectPrivileges) -> bool {
    object.schema.is_empty()
        || object.table.is_empty()
        || object.connect.is_none()
        || object.temp.is_none()
        || object.database_create.is_none()
        || object.usage.is_none()
        || object.schema_create.is_none()
        || object.select.is_none()
        || object.insert.is_none()
        || object.update.is_none()
        || object.delete.is_none()
        || object.truncate.is_none()
        || object.references.is_none()
        || object.trigger.is_none()
}

fn object_readonly(object: &ObjectPrivileges) -> bool {
    object.connect == Some(true)
        && object.temp == Some(false)
        && object.database_create == Some(false)
        && object.usage == Some(true)
        && object.schema_create == Some(false)
        && object.select == Some(true)
        && object.insert == Some(false)
        && object.update == Some(false)
        && object.delete == Some(false)
        && object.truncate == Some(false)
        && object.references == Some(false)
        && object.trigger == Some(false)
}

fn access_unknown(access: &AccessReport) -> bool {
    access.principal.status != FactStatus::Known
        || access.role.status != FactStatus::Known
        || access.principal.value.as_ref().is_none_or(|principal| {
            principal.current_user.is_empty()
                || principal.session_user.is_empty()
                || principal.database.is_empty()
                || principal.current_user != principal.session_user
        })
        || !matches!(access.write.state.as_str(), "read_only" | "write_capable")
}

fn access_readonly(access: &AccessReport) -> bool {
    let Some(role) = access.role.value.as_ref() else {
        return false;
    };
    let Some(principal) = access.principal.value.as_ref() else {
        return false;
    };
    access.write.state == "read_only"
        && access.write.findings.is_empty()
        && access.security_definers.is_empty()
        && role_is_login_reader(role)
        && role.name == principal.current_user
        && access
            .memberships
            .iter()
            .all(|edge| !edge.admin_option && !edge.set_option)
}

fn role_is_login_reader(role: &RoleAttributes) -> bool {
    !role.name.is_empty()
        && !role.superuser
        && !role.create_role
        && !role.create_db
        && !role.replication
        && !role.bypass_rls
        && role.can_login
}

fn build_identity(
    access: &AccessReport,
    current_user: &str,
    session_user: &str,
    database: &str,
    aligned: bool,
) -> BudgetIdentity {
    let matched = access.principal.value.as_ref().is_some_and(|principal| {
        access.principal.status == FactStatus::Known
            && principal.current_user == current_user
            && principal.session_user == session_user
            && principal.database == database
            && principal.current_user == principal.session_user
            && !principal.current_user.is_empty()
            && !principal.database.is_empty()
    });
    let known = matched && aligned && current_user == session_user;
    BudgetIdentity {
        status: if known { "known" } else { "unknown" }.to_string(),
        current_user: current_user.to_string(),
        session_user: session_user.to_string(),
        database: database.to_string(),
        aligned: aligned && current_user == session_user,
    }
}

fn observed_settings(
    rows: &[SettingRow],
    statement_ms: u64,
    lock_ms: u64,
    connect_ms: u64,
) -> ObservedSettings {
    ObservedSettings {
        statement_timeout: timeout_setting("statement_timeout", statement_ms, rows),
        lock_timeout: timeout_setting("lock_timeout", lock_ms, rows),
        idle_in_transaction_session_timeout: timeout_setting(
            "idle_in_transaction_session_timeout",
            statement_ms,
            rows,
        ),
        default_transaction_read_only: flag_setting("default_transaction_read_only", "on", rows),
        connect_timeout_ms: connect_ms,
        connect_timeout_client_enforced: true,
        connect_timeout_server_enforced: false,
        row_rate_per_sec: session::MAX_ROWS_PER_SEC,
        row_rate_client_enforced: true,
        row_rate_server_enforced: false,
    }
}

fn timeout_setting(name: &str, configured_ms: u64, rows: &[SettingRow]) -> ObservedSetting {
    let configured = configured_ms.to_string();
    let Some(row) = rows.iter().find(|row| row.name == name) else {
        return unknown_setting(name, Some(configured));
    };
    if row.unit != "ms" {
        return ObservedSetting {
            name: name.to_string(),
            status: "unknown".to_string(),
            configured: Some(configured),
            setting: Some(row.setting.clone()),
            unit: Some(row.unit.clone()),
        };
    }
    if row.setting.parse::<u64>().is_err() {
        return unknown_setting(name, Some(configured));
    }
    ObservedSetting {
        name: name.to_string(),
        status: "known".to_string(),
        configured: Some(configured),
        setting: Some(row.setting.clone()),
        unit: Some(row.unit.clone()),
    }
}

fn flag_setting(name: &str, configured: &str, rows: &[SettingRow]) -> ObservedSetting {
    let Some(row) = rows.iter().find(|row| row.name == name) else {
        return unknown_setting(name, Some(configured.to_string()));
    };
    if row.unit.is_empty() && (row.setting == "on" || row.setting == "off") {
        return ObservedSetting {
            name: name.to_string(),
            status: "known".to_string(),
            configured: Some(configured.to_string()),
            setting: Some(row.setting.clone()),
            unit: Some(String::new()),
        };
    }
    ObservedSetting {
        name: name.to_string(),
        status: "unknown".to_string(),
        configured: Some(configured.to_string()),
        setting: Some(row.setting.clone()),
        unit: Some(row.unit.clone()),
    }
}

fn unknown_setting(name: &str, configured: Option<String>) -> ObservedSetting {
    ObservedSetting {
        name: name.to_string(),
        status: "unknown".to_string(),
        configured,
        setting: None,
        unit: None,
    }
}

fn client_bound(unit: &str, limit: u64) -> BudgetBound {
    BudgetBound {
        unit: unit.to_string(),
        configured_limit: limit,
        configured_status: "known".to_string(),
        client_enforced: true,
        server_enforced: false,
        server_setting: None,
        server_observed: None,
        server_status: "not_applicable".to_string(),
    }
}

fn wall_bound(limit_ms: u64, setting: &ObservedSetting) -> BudgetBound {
    let parsed = setting
        .setting
        .as_deref()
        .filter(|_| setting.unit.as_deref() == Some("ms") && setting.status == "known")
        .and_then(|value| value.parse::<u64>().ok());
    let configured = limit_ms.to_string();
    let matched =
        parsed == Some(limit_ms) && setting.configured.as_deref() == Some(configured.as_str());
    BudgetBound {
        unit: "WallClock".to_string(),
        configured_limit: limit_ms,
        configured_status: "known".to_string(),
        client_enforced: true,
        server_enforced: matched,
        server_setting: Some("statement_timeout".to_string()),
        server_observed: parsed,
        server_status: if setting.status != "known" {
            "unknown".to_string()
        } else if matched {
            "known".to_string()
        } else {
            "mismatch".to_string()
        },
    }
}

fn native_version(version: &VersionFact) -> bool {
    version.admission == "tested"
        && version.major == 17
        && version.minor == 10
        && version.server_version.starts_with("17.10")
        && !foreign_product(&version.server_version)
}

fn foreign_product(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    FOREIGN_PRODUCTS.iter().any(|token| lower.contains(token))
}

fn tls_observed(tls: &BudgetTls) -> bool {
    tls.mode == "verify-full"
        && tls.trust == "caller-ca-only"
        && tls.hostname_verification
        && tls.server_observation == "observed"
        && tls.negotiated_version.starts_with("TLSv1.")
}

fn derive_blocks(evidence: &BudgetEvidence) -> Vec<BudgetBlock> {
    let mut blocks = Vec::new();
    if evidence.identity.status != "known"
        || !evidence.identity.aligned
        || evidence.identity.current_user.is_empty()
        || evidence.identity.session_user.is_empty()
        || evidence.identity.database.is_empty()
        || evidence.identity.current_user != evidence.identity.session_user
    {
        push(&mut blocks, "IDENTITY_MISSING", "identity.status");
    }
    if !tls_observed(&evidence.tls) {
        push(&mut blocks, "TLS_UNOBSERVED", "tls");
    }
    if !compat_ok(evidence) {
        push(&mut blocks, "COMPAT_MODE_UNKNOWN", "compat_mode");
    }
    if retagged(&evidence.shadow) {
        push(&mut blocks, "NATIVE_SOURCE_RETAG", "shadow");
    }
    match (
        evidence.permission.status.as_str(),
        evidence.permission.read_only,
    ) {
        ("known", true) => {}
        ("known", false) => push(&mut blocks, "PERMISSION_NOT_READONLY", "permission"),
        _ => push(&mut blocks, "PERMISSION_UNKNOWN", "permission"),
    }
    if settings_unknown(evidence) {
        push(&mut blocks, "SERVER_SETTING_UNKNOWN", "settings");
    }
    if settings_mismatch(evidence) {
        push(&mut blocks, "SERVER_SETTING_MISMATCH", "settings");
    }
    if caps_misstated(evidence) {
        push(&mut blocks, "SERVER_CAP_MISSTATED", "bounds");
    }
    blocks
}

fn compat_ok(evidence: &BudgetEvidence) -> bool {
    evidence.compat_mode == "postgres-native"
        && evidence.compat_status == "known"
        && evidence.target_id == "postgres-17-native"
        && evidence.version.admission == "tested"
        && evidence.version.major == 17
        && evidence.version.minor == 10
        && evidence.version.server_version.starts_with("17.10")
        && !foreign_product(&evidence.version.server_version)
}

fn retagged(shadow: &str) -> bool {
    let lower = shadow.to_ascii_lowercase();
    shadow != "none" || lower.contains("local-pg") || lower.contains("local_pg")
}

fn setting_match(setting: &ObservedSetting, expect_name: &str, expect_unit: &str) -> &'static str {
    let Some(configured) = setting.configured.as_deref() else {
        return "unknown";
    };
    let Some(value) = setting.setting.as_deref() else {
        return "unknown";
    };
    let unit = setting.unit.as_deref().unwrap_or("");
    if setting.name != expect_name
        || setting.status != "known"
        || unit != expect_unit
        || configured.is_empty()
    {
        return "unknown";
    }
    if expect_unit == "ms" && (configured.parse::<u64>().is_err() || value.parse::<u64>().is_err())
    {
        return "unknown";
    }
    if value == configured {
        "match"
    } else {
        "mismatch"
    }
}

fn settings_unknown(evidence: &BudgetEvidence) -> bool {
    setting_match(
        &evidence.settings.statement_timeout,
        "statement_timeout",
        "ms",
    ) == "unknown"
        || setting_match(&evidence.settings.lock_timeout, "lock_timeout", "ms") == "unknown"
        || setting_match(
            &evidence.settings.idle_in_transaction_session_timeout,
            "idle_in_transaction_session_timeout",
            "ms",
        ) == "unknown"
        || setting_match(
            &evidence.settings.default_transaction_read_only,
            "default_transaction_read_only",
            "",
        ) == "unknown"
        || evidence
            .bounds
            .iter()
            .any(|bound| bound.unit == "WallClock" && bound.server_status == "unknown")
}

fn settings_mismatch(evidence: &BudgetEvidence) -> bool {
    setting_match(
        &evidence.settings.statement_timeout,
        "statement_timeout",
        "ms",
    ) == "mismatch"
        || setting_match(&evidence.settings.lock_timeout, "lock_timeout", "ms") == "mismatch"
        || setting_match(
            &evidence.settings.idle_in_transaction_session_timeout,
            "idle_in_transaction_session_timeout",
            "ms",
        ) == "mismatch"
        || setting_match(
            &evidence.settings.default_transaction_read_only,
            "default_transaction_read_only",
            "",
        ) == "mismatch"
        || evidence
            .bounds
            .iter()
            .any(|bound| bound.unit == "WallClock" && bound.server_status == "mismatch")
}

fn caps_misstated(evidence: &BudgetEvidence) -> bool {
    let mut bad_client = false;
    for bound in &evidence.bounds {
        if bound.unit == "Rows" || bound.unit == "Bytes" {
            bad_client |= !bound.client_enforced
                || bound.server_enforced
                || bound.server_setting.is_some()
                || bound.server_observed.is_some()
                || bound.server_status != "not_applicable";
        }
    }
    let bad_wall = match evidence
        .bounds
        .iter()
        .find(|bound| bound.unit == "WallClock")
    {
        Some(bound) => {
            !bound.client_enforced || bound.server_setting.as_deref() != Some("statement_timeout")
        }
        None => true,
    };
    bad_client
        || bad_wall
        || !evidence.settings.connect_timeout_client_enforced
        || evidence.settings.connect_timeout_server_enforced
        || evidence.settings.connect_timeout_ms == 0
        || evidence.settings.row_rate_per_sec != session::MAX_ROWS_PER_SEC
        || !evidence.settings.row_rate_client_enforced
        || evidence.settings.row_rate_server_enforced
}

fn push(blocks: &mut Vec<BudgetBlock>, code: &str, fact: &str) {
    let block = BudgetBlock {
        code: code.to_string(),
        fact: fact.to_string(),
    };
    if !blocks.iter().any(|item| item == &block) {
        blocks.push(block);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::model::{
        CapabilityStatus, CdcPrerequisite, DbaItem, Fact, Principal, RoleAttributes, TlsReport,
        WriteDetermination, connector_capabilities,
    };

    fn budget() -> PgBudget {
        PgBudget::new(
            Duration::from_secs(5),
            Duration::from_millis(7_000),
            Duration::from_millis(3_000),
            11,
            4_096,
        )
        .unwrap()
    }

    fn tested_access() -> AccessReport {
        AccessReport {
            protocol: crate::PROTOCOL.to_string(),
            core_contract: crate::CORE_BOUNDARY.to_string(),
            source_id: "oo_dl01w1_38".to_string(),
            secret_ref: "secret://oo-dl01w1-38/reader".to_string(),
            tls: TlsReport {
                mode: "verify-full".to_string(),
                trust: "caller-ca-only".to_string(),
                hostname_verification: true,
                negotiated_version: "TLSv1.3".to_string(),
                server_observation: "observed".to_string(),
            },
            version: VersionFact {
                server_version_num: 170_010,
                server_version: "17.10".to_string(),
                major: 17,
                minor: 10,
                declared_minimum_major: crate::DECLARED_MIN_MAJOR,
                tested_versions: crate::TESTED_VERSIONS
                    .iter()
                    .map(|item| (*item).to_string())
                    .collect(),
                admission: "tested".to_string(),
            },
            registration_allowed: true,
            block_reasons: Vec::new(),
            principal: Fact::known(Principal {
                current_user: "reader".to_string(),
                session_user: "reader".to_string(),
                database: "src".to_string(),
            }),
            in_recovery: Fact::known(false),
            role: Fact::known(RoleAttributes {
                name: "reader".to_string(),
                superuser: false,
                inherit: true,
                create_role: false,
                create_db: false,
                can_login: true,
                replication: false,
                bypass_rls: false,
            }),
            memberships: Vec::new(),
            write: WriteDetermination {
                state: "read_only".to_string(),
                findings: Vec::new(),
            },
            security_definers: Vec::new(),
            rls: Vec::new(),
            publications: Fact::known(Vec::new()),
            replication_slots: Fact::known(Vec::new()),
            cdc: CdcPrerequisite {
                wal_level: Fact::known("replica".to_string()),
                replication_attribute: Fact::known(false),
                publication_for_selected: Fact::known(false),
                implemented: false,
            },
            capabilities: connector_capabilities(),
            dba_checklist: Vec::<DbaItem>::new(),
        }
    }

    fn readonly_object() -> ObjectPrivileges {
        ObjectPrivileges {
            schema: "app".to_string(),
            table: "item".to_string(),
            connect: Some(true),
            temp: Some(false),
            database_create: Some(false),
            usage: Some(true),
            schema_create: Some(false),
            select: Some(true),
            insert: Some(false),
            update: Some(false),
            delete: Some(false),
            truncate: Some(false),
            references: Some(false),
            trigger: Some(false),
        }
    }

    fn settings(statement: &str, unit: &str) -> Vec<SettingRow> {
        vec![
            SettingRow {
                name: "default_transaction_read_only".to_string(),
                setting: "on".to_string(),
                unit: String::new(),
            },
            SettingRow {
                name: "idle_in_transaction_session_timeout".to_string(),
                setting: "7000".to_string(),
                unit: "ms".to_string(),
            },
            SettingRow {
                name: "lock_timeout".to_string(),
                setting: "3000".to_string(),
                unit: "ms".to_string(),
            },
            SettingRow {
                name: "statement_timeout".to_string(),
                setting: statement.to_string(),
                unit: unit.to_string(),
            },
        ]
    }

    fn assemble(
        access: AccessReport,
        selected: Vec<(String, String)>,
        privileges: Vec<ObjectPrivileges>,
        settings: Vec<SettingRow>,
    ) -> BudgetInspection {
        let budget = budget();
        assemble_inspection(AssembleIn {
            budget,
            access,
            current_user: "reader".to_string(),
            session_user: "reader".to_string(),
            database: "src".to_string(),
            aligned: true,
            selected,
            settings,
            privileges,
        })
        .unwrap()
    }

    fn bound<'a>(evidence: &'a BudgetEvidence, unit: &str) -> &'a BudgetBound {
        evidence
            .bounds
            .iter()
            .find(|item| item.unit == unit)
            .unwrap()
    }

    #[test]
    fn limits_come_from_the_session_budget() {
        let inspection = assemble(
            tested_access(),
            vec![("app".to_string(), "item".to_string())],
            vec![readonly_object()],
            settings("7000", "ms"),
        );
        let evidence = &inspection.evidence;
        let rows = bound(evidence, "Rows");
        assert_eq!(rows.configured_limit, u64::from(budget().max_rows()));
        assert!(rows.client_enforced);
        assert!(!rows.server_enforced);
        assert!(rows.server_setting.is_none());
        assert!(rows.server_observed.is_none());
        assert_eq!(rows.server_status, "not_applicable");
        let bytes = bound(evidence, "Bytes");
        assert_eq!(bytes.configured_limit, budget().max_bytes());
        assert!(!bytes.server_enforced);
        assert_eq!(bytes.server_status, "not_applicable");
        let wall = bound(evidence, "WallClock");
        assert_eq!(
            wall.configured_limit,
            budget().statement_timeout().as_millis() as u64
        );
        assert!(wall.client_enforced);
        assert!(wall.server_enforced);
        assert_eq!(wall.server_setting.as_deref(), Some("statement_timeout"));
        assert_eq!(wall.server_observed, Some(7_000));
        assert_eq!(wall.server_status, "known");
        assert_eq!(evidence.cost_model.family, "oltp_accounting");
        assert_eq!(
            evidence.cost_model.units,
            vec![
                "Rows".to_string(),
                "Bytes".to_string(),
                "WallClock".to_string()
            ]
        );
        assert_eq!(evidence.cost_model.estimate_confidence, "unknown");
        assert_eq!(evidence.cost_model.estimate_basis, "not_collected");
        assert_eq!(evidence.cost_model.billing, "none");
        assert_eq!(evidence.shadow, "none");
        assert_eq!(session::MAX_ROWS_PER_SEC, 50_000);
        assert_eq!(
            evidence.settings.row_rate_per_sec,
            session::MAX_ROWS_PER_SEC
        );
        assert!(evidence.settings.row_rate_client_enforced);
        assert!(!evidence.settings.row_rate_server_enforced);
        assert!(!evidence.settings.connect_timeout_server_enforced);
        assert_eq!(evidence.settings.connect_timeout_ms, 5_000);
        assert!(evidence.permission.read_only);
        assert!(evidence.blocks.is_empty());
        assert_eq!(inspection.access_report.protocol, "oo.source.pgsql/v1");
        assert_eq!(evidence.protocol, BUDGET_PROTOCOL);
        let access_json = String::from_utf8(inspection.access_bytes.clone()).unwrap();
        assert!(access_json.starts_with("{\"protocol\":\"oo.source.pgsql/v1\""));
        assert!(!access_json.contains("oltp_accounting"));
        assert!(!access_json.contains("budget_unit"));
        assert_eq!(
            evidence.companion.sha256,
            sha256_hex(&inspection.access_bytes)
        );
        assert_eq!(
            driver_digest(),
            "d510f5df0eddeb651367bbdf62f30b795c0d747f379104d990f307f8fd0664a5"
        );
        let _kept: &[CapabilityStatus] = &inspection.access_report.capabilities;
    }

    #[test]
    fn wallclock_mismatch_stays_unknown_estimate() {
        let inspection = assemble(
            tested_access(),
            vec![("app".to_string(), "item".to_string())],
            vec![readonly_object()],
            settings("1", "ms"),
        );
        let wall = bound(&inspection.evidence, "WallClock");
        assert!(!wall.server_enforced);
        assert_eq!(wall.server_status, "mismatch");
        assert_eq!(wall.server_observed, Some(1));
        assert_eq!(wall.configured_limit, 7_000);
        assert_eq!(
            inspection.evidence.cost_model.estimate_confidence,
            "unknown"
        );
        assert!(
            inspection
                .evidence
                .blocks
                .iter()
                .any(|block| block.code == "SERVER_SETTING_MISMATCH")
        );
    }

    #[test]
    fn non_millisecond_unit_is_not_converted() {
        let inspection = assemble(
            tested_access(),
            vec![("app".to_string(), "item".to_string())],
            vec![readonly_object()],
            settings("7", "s"),
        );
        let wall = bound(&inspection.evidence, "WallClock");
        assert!(!wall.server_enforced);
        assert_eq!(wall.server_status, "unknown");
        assert!(wall.server_observed.is_none());
        assert!(
            inspection
                .evidence
                .blocks
                .iter()
                .any(|block| block.code == "SERVER_SETTING_UNKNOWN")
        );
        assert!(
            !inspection
                .evidence
                .blocks
                .iter()
                .any(|block| block.code == "SERVER_SETTING_MISMATCH")
        );
    }

    #[test]
    fn empty_selection_and_temp_fail_closed() {
        let empty = assemble(
            tested_access(),
            Vec::new(),
            Vec::new(),
            settings("7000", "ms"),
        );
        assert_eq!(empty.evidence.permission.status, "unknown");
        assert!(!empty.evidence.permission.read_only);
        assert!(
            empty
                .evidence
                .blocks
                .iter()
                .any(|block| block.code == "PERMISSION_UNKNOWN")
        );
        let mut temp = readonly_object();
        temp.temp = Some(true);
        let granted = assemble(
            tested_access(),
            vec![("app".to_string(), "item".to_string())],
            vec![temp],
            settings("7000", "ms"),
        );
        assert_eq!(granted.evidence.permission.status, "known");
        assert!(!granted.evidence.permission.read_only);
        assert!(
            granted
                .evidence
                .blocks
                .iter()
                .any(|block| block.code == "PERMISSION_NOT_READONLY")
        );
    }

    #[test]
    fn foreign_product_token_is_not_native() {
        let mut access = tested_access();
        access.version.server_version = "17.10 KingbaseES".to_string();
        let inspection = assemble(
            access,
            vec![("app".to_string(), "item".to_string())],
            vec![readonly_object()],
            settings("7000", "ms"),
        );
        assert_eq!(inspection.evidence.compat_mode, "unknown");
        assert_eq!(inspection.evidence.compat_status, "unknown");
        assert_eq!(inspection.evidence.target_id, "unknown");
        assert_eq!(inspection.evidence.shadow, "none");
        assert!(
            inspection
                .evidence
                .blocks
                .iter()
                .any(|block| block.code == "COMPAT_MODE_UNKNOWN")
        );
        assert!(!inspection.evidence.shadow.contains("local"));
    }
}
