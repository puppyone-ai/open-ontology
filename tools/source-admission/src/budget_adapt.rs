//! Admission for `oo.source.pgsql.budget/v1`.
//!
//! The legacy `oo.source.pgsql/v1` adapter is unchanged and still denies
//! `BUDGET_UNIT_UNKNOWN`. This path admits only when the evidence bytes, the
//! companion access bytes, and the hard-cap config agree.

use sha2::{Digest, Sha256};

use crate::admit::{check_options, check_r1w, check_write_class, simple_id};
use crate::checklist::{ExistingDba, from_existing};
use crate::model::{
    Block, CORE_CONTRACT, DECISION_PROTOCOL, DebeziumOptions, Decision, DeclaredCapability,
    Identity, Model, PG_BUDGET_CONFIG_PROTOCOL, PG_BUDGET_PROTOCOL, PG_REPORT_PROTOCOL,
    R1wApproval, WriteOptions, ZeroWrite,
};
use crate::parse::{self, ParseError};
use crate::pg_adapt::{self, AccessReport};

const ROW_RATE_PER_SEC: u32 = 50_000;
const DRIVER_KIND: &str = "in_process";
const DRIVER_NAME: &str = "sqlx-postgres";
const DRIVER_VERSION: &str = "0.9.0";
const DRIVER_FEATURES: &str = "_tls-rustls-ring-no-roots";
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

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    protocol: String,
    core_contract: String,
    source_id: String,
    secret_ref: String,
    companion: Companion,
    identity: IdentityFact,
    tls: TlsFact,
    version: VersionFact,
    driver: DriverFact,
    shadow: String,
    compat_mode: String,
    compat_status: String,
    target_id: String,
    registration_allowed: bool,
    block_reasons: Vec<Block>,
    permission: PermissionFact,
    settings: SettingsFact,
    bounds: Vec<BoundFact>,
    cost_model: CostFact,
    blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Companion {
    protocol: String,
    sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityFact {
    status: String,
    current_user: String,
    session_user: String,
    database: String,
    aligned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TlsFact {
    mode: String,
    trust: String,
    hostname_verification: bool,
    server_observation: String,
    negotiated_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionFact {
    server_version_num: i64,
    server_version: String,
    major: i64,
    minor: i64,
    admission: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DriverFact {
    kind: String,
    name: String,
    version: String,
    features: String,
    digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectPriv {
    schema: String,
    table: String,
    connect: Option<bool>,
    temp: Option<bool>,
    database_create: Option<bool>,
    usage: Option<bool>,
    schema_create: Option<bool>,
    select: Option<bool>,
    insert: Option<bool>,
    update: Option<bool>,
    delete: Option<bool>,
    truncate: Option<bool>,
    references: Option<bool>,
    trigger: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionFact {
    status: String,
    read_only: bool,
    objects: Vec<ObjectPriv>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingFact {
    name: String,
    status: String,
    configured: Option<String>,
    setting: Option<String>,
    unit: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsFact {
    statement_timeout: SettingFact,
    lock_timeout: SettingFact,
    idle_in_transaction_session_timeout: SettingFact,
    default_transaction_read_only: SettingFact,
    connect_timeout_ms: u64,
    connect_timeout_client_enforced: bool,
    connect_timeout_server_enforced: bool,
    row_rate_per_sec: u32,
    row_rate_client_enforced: bool,
    row_rate_server_enforced: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundFact {
    unit: String,
    configured_limit: u64,
    configured_status: String,
    client_enforced: bool,
    server_enforced: bool,
    server_setting: Option<String>,
    server_observed: Option<u64>,
    server_status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CostFact {
    family: String,
    units: Vec<String>,
    estimate_confidence: String,
    estimate_basis: String,
    billing: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HardCap {
    unit: String,
    hard_limit: u64,
    client_cap: u64,
    confidence: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PgBudgetConfig {
    protocol: String,
    target_id: String,
    source_id: String,
    secret_ref: String,
    compat_mode: String,
    current_user: String,
    database: String,
    hard_caps: Vec<HardCap>,
    write_options: WriteOptions,
    debezium: DebeziumOptions,
    requested_write_class: String,
    r1w: Option<R1wApproval>,
}

/// Admit one budget evidence document, its companion access report, and a hard-cap config.
///
/// The three arguments are the exact file bytes. A parse failure is [`ParseError`].
/// A parsed document that is missing a fact returns a blocked [`Decision`].
pub fn admit_pg_budget(
    evidence_bytes: &[u8],
    access_bytes: &[u8],
    config_bytes: &[u8],
) -> Result<Decision, ParseError> {
    let evidence: Evidence = parse::parse_strict(evidence_bytes)?;
    let access = pg_adapt::deserialize_access(access_bytes)?;
    let config: PgBudgetConfig = parse::parse_strict(config_bytes)?;
    Ok(decide(
        &evidence,
        evidence_bytes,
        &access,
        access_bytes,
        &config,
    ))
}

fn decide(
    evidence: &Evidence,
    evidence_bytes: &[u8],
    access: &AccessReport,
    access_bytes: &[u8],
    config: &PgBudgetConfig,
) -> Decision {
    let mut blocks = Vec::new();
    push_protocols(evidence, access, config, &mut blocks);
    push_byte_identity(evidence, access, access_bytes, config, &mut blocks);
    push_derived(evidence, access, &mut blocks);
    push_budget_driver(evidence, config, &mut blocks);
    push_registration_and_writes(access, config, &mut blocks);
    let capabilities = capabilities_from(evidence, access, &mut blocks);
    let checklist = checklist_from(access);
    Decision {
        protocol: DECISION_PROTOCOL.to_string(),
        admitted: blocks.is_empty(),
        matrix_mutated: false,
        blocks,
        model: Model {
            capabilities,
            capture_methods: Vec::new(),
        },
        identity: Identity {
            target_id: evidence.target_id.clone(),
            source_id: evidence.source_id.clone(),
            secret_ref: evidence.secret_ref.clone(),
            source_report_protocol: Some(evidence.protocol.clone()),
            core_contract: Some(evidence.core_contract.clone()),
            source_report_sha256: Some(sha256(evidence_bytes)),
        },
        dba_checklist: checklist,
        zero_write: ZeroWrite {
            proven: false,
            basis: "not_audited".to_string(),
        },
    }
}

fn push_protocols(
    evidence: &Evidence,
    access: &AccessReport,
    config: &PgBudgetConfig,
    blocks: &mut Vec<Block>,
) {
    if evidence.protocol != PG_BUDGET_PROTOCOL {
        push(blocks, "PROTOCOL_UNSUPPORTED", "evidence.protocol");
    }
    if access.protocol != PG_REPORT_PROTOCOL {
        push(blocks, "PROTOCOL_UNSUPPORTED", "access.protocol");
    }
    if config.protocol != PG_BUDGET_CONFIG_PROTOCOL {
        push(blocks, "PROTOCOL_UNSUPPORTED", "config.protocol");
    }
}

fn push_byte_identity(
    evidence: &Evidence,
    access: &AccessReport,
    access_bytes: &[u8],
    config: &PgBudgetConfig,
    blocks: &mut Vec<Block>,
) {
    if evidence.core_contract != CORE_CONTRACT || access.core_contract != CORE_CONTRACT {
        push(blocks, "IDENTITY_MISMATCH", "core_contract");
    }
    if evidence.companion.protocol != PG_REPORT_PROTOCOL
        || evidence.companion.sha256 != sha256(access_bytes)
    {
        push(blocks, "IDENTITY_MISMATCH", "companion");
    }
    if evidence.source_id != access.source_id || evidence.source_id != config.source_id {
        push(blocks, "IDENTITY_MISMATCH", "identity");
    }
    if evidence.secret_ref != access.secret_ref || evidence.secret_ref != config.secret_ref {
        push(blocks, "IDENTITY_MISMATCH", "identity");
    }
    if evidence.target_id != config.target_id || evidence.compat_mode != config.compat_mode {
        push(blocks, "IDENTITY_MISMATCH", "identity");
    }
    if config.compat_mode != "postgres-native" {
        push(blocks, "COMPAT_MODE_UNKNOWN", "config.compat_mode");
    }
    match_identity(evidence, access, config, blocks);
    if tls_differs(evidence, access) {
        push(blocks, "IDENTITY_MISMATCH", "tls");
    }
    if version_differs(evidence, access) {
        push(blocks, "IDENTITY_MISMATCH", "version");
    }
    if registration_differs(evidence, access) {
        push(blocks, "IDENTITY_MISMATCH", "registration");
    }
}

fn tls_differs(evidence: &Evidence, access: &AccessReport) -> bool {
    evidence.tls.mode != access.tls.mode
        || evidence.tls.trust != access.tls.trust
        || evidence.tls.hostname_verification != access.tls.hostname_verification
        || evidence.tls.server_observation != access.tls.server_observation
        || evidence.tls.negotiated_version != access.tls.negotiated_version
}

fn version_differs(evidence: &Evidence, access: &AccessReport) -> bool {
    evidence.version.server_version_num != access.version.server_version_num
        || evidence.version.server_version != access.version.server_version
        || evidence.version.major != access.version.major
        || evidence.version.minor != access.version.minor
        || evidence.version.admission != access.version.admission
}

fn registration_differs(evidence: &Evidence, access: &AccessReport) -> bool {
    let reasons = access
        .block_reasons
        .iter()
        .map(|reason| Block {
            code: reason.code.clone(),
            fact: reason.fact.clone(),
        })
        .collect::<Vec<_>>();
    evidence.registration_allowed != access.registration_allowed
        || evidence.block_reasons != reasons
}

fn push_derived(evidence: &Evidence, access: &AccessReport, blocks: &mut Vec<Block>) {
    let derived = derived_blocks(evidence);
    for block in &derived {
        push(blocks, &block.code, &block.fact);
    }
    if evidence.blocks != derived {
        push(blocks, "IDENTITY_MISMATCH", "evidence.blocks");
    }
    let (status, read_only) = permission_judge(access, &evidence.permission.objects);
    if evidence.permission.status != status || evidence.permission.read_only != read_only {
        push(blocks, "IDENTITY_MISMATCH", "permission");
        match (status, read_only) {
            ("known", true) => {}
            ("known", false) => push(blocks, "PERMISSION_NOT_READONLY", "permission"),
            _ => push(blocks, "PERMISSION_UNKNOWN", "permission"),
        }
    }
}

fn push_budget_driver(evidence: &Evidence, config: &PgBudgetConfig, blocks: &mut Vec<Block>) {
    check_cost(evidence, blocks);
    check_caps(evidence, config, blocks);
    if driver_differs(evidence) {
        push(blocks, "DRIVER_MISSING", "driver");
    }
    if !simple_id(&evidence.source_id) || !simple_id(&config.source_id) {
        push(blocks, "SOURCE_ID_INVALID", "source_id");
    }
    if !evidence.secret_ref.starts_with("secret://") || evidence.secret_ref.len() < 10 {
        push(blocks, "SECRET_REF_INVALID", "secret_ref");
    }
}

fn driver_differs(evidence: &Evidence) -> bool {
    evidence.driver.kind != DRIVER_KIND
        || evidence.driver.name != DRIVER_NAME
        || evidence.driver.version != DRIVER_VERSION
        || evidence.driver.features != DRIVER_FEATURES
        || evidence.driver.digest != driver_digest()
}

fn push_registration_and_writes(
    access: &AccessReport,
    config: &PgBudgetConfig,
    blocks: &mut Vec<Block>,
) {
    for reason in &access.block_reasons {
        push(blocks, &reason.code, &reason.fact);
    }
    if !access.registration_allowed && access.block_reasons.is_empty() {
        push(blocks, "REGISTRATION_BLOCKED", "registration_allowed");
    }
    check_write_class(
        &config.requested_write_class,
        "config.requested_write_class",
        blocks,
    );
    check_r1w(
        config.requested_write_class == "R1w",
        None,
        config.r1w.as_ref(),
        blocks,
    );
    check_options(&config.write_options, &config.debezium, blocks);
}

fn capabilities_from(
    evidence: &Evidence,
    access: &AccessReport,
    blocks: &mut Vec<Block>,
) -> Vec<DeclaredCapability> {
    let mut capabilities = Vec::new();
    for cap in &access.capabilities {
        if (cap.name == "source_write" || cap.name == "source_ddl") && cap.supported {
            push(blocks, "SOURCE_WRITE_FORBIDDEN", &cap.name);
        }
        if cap.name == "scan" && cap.supported {
            capabilities.push(declared("Catalog", None));
        }
        if cap.name == "bounded_read" && cap.supported {
            capabilities.push(declared("BatchFull", None));
        }
        if cap.name == "cdc" && cap.supported {
            push(blocks, "CDC_NOT_IN_SLICE", "capabilities.cdc");
        }
    }
    if access.cdc.implemented {
        push(blocks, "CDC_NOT_IN_SLICE", "cdc.implemented");
    }
    let units = ["Rows", "Bytes", "WallClock"];
    if evidence.bounds.len() == units.len()
        && evidence
            .bounds
            .iter()
            .map(|bound| bound.unit.as_str())
            .eq(units)
    {
        for unit in units {
            capabilities.push(declared("Budget", Some(unit)));
        }
    }
    capabilities
}

fn checklist_from(access: &AccessReport) -> Vec<crate::model::ChecklistItem> {
    from_existing(
        &access
            .dba_checklist
            .iter()
            .map(|item| ExistingDba {
                id: item.id.clone(),
                level: item.level.clone(),
                risk: item.risk.clone(),
                rollback: item.rollback.clone(),
                statement: item.statement.clone(),
            })
            .collect::<Vec<_>>(),
    )
}

fn match_identity(
    evidence: &Evidence,
    access: &AccessReport,
    config: &PgBudgetConfig,
    blocks: &mut Vec<Block>,
) {
    let missing = evidence.identity.status != "known"
        || !evidence.identity.aligned
        || evidence.identity.current_user.is_empty()
        || evidence.identity.session_user.is_empty()
        || evidence.identity.database.is_empty()
        || evidence.identity.current_user != evidence.identity.session_user
        || access.principal.status != "known";
    if missing {
        push(blocks, "IDENTITY_MISSING", "identity.status");
    }
    let Some(principal) = access.principal.value.as_ref() else {
        push(blocks, "IDENTITY_MISMATCH", "identity");
        return;
    };
    if evidence.identity.current_user != principal.current_user
        || evidence.identity.session_user != principal.session_user
        || evidence.identity.database != principal.database
        || config.current_user != principal.current_user
        || config.database != principal.database
        || principal.current_user != principal.session_user
    {
        push(blocks, "IDENTITY_MISMATCH", "identity");
    }
}

fn check_cost(evidence: &Evidence, blocks: &mut Vec<Block>) {
    let units = ["Rows", "Bytes", "WallClock"];
    if evidence.cost_model.family != "oltp_accounting"
        || evidence.cost_model.units != units
        || evidence.cost_model.estimate_confidence != "unknown"
        || evidence.cost_model.estimate_basis != "not_collected"
        || evidence.cost_model.billing != "none"
        || evidence.bounds.len() != 3
        || evidence
            .bounds
            .iter()
            .map(|bound| bound.unit.as_str())
            .ne(units)
        || evidence
            .bounds
            .iter()
            .any(|bound| bound.configured_status != "known")
    {
        push(blocks, "BUDGET_UNIT_UNKNOWN", "cost_model");
    }
}

fn check_caps(evidence: &Evidence, config: &PgBudgetConfig, blocks: &mut Vec<Block>) {
    let units = ["Rows", "Bytes", "WallClock"];
    if config.hard_caps.len() != 3
        || config
            .hard_caps
            .iter()
            .any(|cap| !units.contains(&cap.unit.as_str()))
    {
        push(blocks, "BUDGET_UNIT_UNKNOWN", "hard_caps");
    }
    for unit in units {
        let Some(cap) = config.hard_caps.iter().find(|cap| cap.unit == unit) else {
            push(blocks, "BUDGET_UNIT_UNKNOWN", "hard_caps");
            continue;
        };
        if cap.confidence != "unknown" || cap.hard_limit == 0 {
            push(blocks, "BUDGET_UNIT_UNKNOWN", "hard_caps");
        }
        let Some(bound) = evidence.bounds.iter().find(|bound| bound.unit == unit) else {
            push(blocks, "BUDGET_UNIT_UNKNOWN", "hard_caps");
            continue;
        };
        if cap.client_cap != bound.configured_limit || cap.hard_limit != bound.configured_limit {
            push(blocks, "BUDGET_UNIT_MISMATCH", "hard_caps");
        }
    }
}

fn declared(name: &str, unit: Option<&str>) -> DeclaredCapability {
    DeclaredCapability {
        name: name.to_string(),
        cursor: None,
        feed: None,
        cdc: None,
        token_order: None,
        budget_unit: unit.map(str::to_string),
        level: "designed".to_string(),
        write_class: "R0".to_string(),
        consistency: "S0".to_string(),
    }
}

fn derived_blocks(evidence: &Evidence) -> Vec<Block> {
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

fn tls_observed(tls: &TlsFact) -> bool {
    tls.mode == "verify-full"
        && tls.trust == "caller-ca-only"
        && tls.hostname_verification
        && tls.server_observation == "observed"
        && tls.negotiated_version.starts_with("TLSv1.")
}

fn compat_ok(evidence: &Evidence) -> bool {
    evidence.compat_mode == "postgres-native"
        && evidence.compat_status == "known"
        && evidence.target_id == "postgres-17-native"
        && evidence.version.admission == "tested"
        && evidence.version.major == 17
        && evidence.version.minor == 10
        && evidence.version.server_version.starts_with("17.10")
        && !foreign_product(&evidence.version.server_version)
}

fn foreign_product(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    FOREIGN_PRODUCTS.iter().any(|token| lower.contains(token))
}

fn retagged(shadow: &str) -> bool {
    let lower = shadow.to_ascii_lowercase();
    shadow != "none" || lower.contains("local-pg") || lower.contains("local_pg")
}

fn setting_match(setting: &SettingFact, expect_name: &str, expect_unit: &str) -> &'static str {
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

fn settings_unknown(evidence: &Evidence) -> bool {
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

fn settings_mismatch(evidence: &Evidence) -> bool {
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

fn caps_misstated(evidence: &Evidence) -> bool {
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
        || evidence.settings.row_rate_per_sec != ROW_RATE_PER_SEC
        || !evidence.settings.row_rate_client_enforced
        || evidence.settings.row_rate_server_enforced
}

fn permission_judge(access: &AccessReport, objects: &[ObjectPriv]) -> (&'static str, bool) {
    if objects.is_empty() || objects.iter().any(privilege_incomplete) || access_unknown(access) {
        return ("unknown", false);
    }
    let read_only = objects.iter().all(object_readonly) && access_readonly(access);
    ("known", read_only)
}

fn privilege_incomplete(object: &ObjectPriv) -> bool {
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

fn object_readonly(object: &ObjectPriv) -> bool {
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
    access.principal.status != "known"
        || access.role.status != "known"
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
        && !role.name.is_empty()
        && !role.superuser
        && !role.create_role
        && !role.create_db
        && !role.replication
        && !role.bypass_rls
        && role.can_login
        && role.name == principal.current_user
        && access
            .memberships
            .iter()
            .all(|edge| !edge.admin_option && !edge.set_option)
}

fn push(blocks: &mut Vec<Block>, code: &str, fact: &str) {
    let block = Block {
        code: code.to_string(),
        fact: fact.to_string(),
    };
    if !blocks.iter().any(|item| item == &block) {
        blocks.push(block);
    }
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn driver_digest() -> String {
    let mut raw = Vec::new();
    raw.extend_from_slice(DRIVER_NAME.as_bytes());
    raw.push(0);
    raw.extend_from_slice(DRIVER_VERSION.as_bytes());
    raw.push(0);
    raw.extend_from_slice(DRIVER_FEATURES.as_bytes());
    sha256(&raw)
}
