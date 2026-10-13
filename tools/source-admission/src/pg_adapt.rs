//! Explicit adapter for an existing `oo.source.pgsql/v1` AccessReport.
//! Identity strings are copied. The adapter does not rewrite them and does
//! not treat a policy result as a zero-write proof.

use sha2::{Digest, Sha256};

use crate::checklist::{ExistingDba, from_existing};
use crate::model::{
    Block, DECISION_PROTOCOL, Decision, DeclaredCapability, Identity, Model, PERMISSIONS,
    PG_REPORT_PROTOCOL, READ_ONLY_PERMISSIONS, ZeroWrite, known,
};
use crate::parse::{self, ParseError};

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AccessReport {
    pub(crate) protocol: String,
    pub(crate) core_contract: String,
    pub(crate) source_id: String,
    pub(crate) secret_ref: String,
    pub(crate) tls: TlsReport,
    pub(crate) version: VersionFact,
    pub(crate) registration_allowed: bool,
    pub(crate) block_reasons: Vec<BlockReason>,
    pub(crate) principal: Fact<Principal>,
    pub(crate) in_recovery: Fact<bool>,
    pub(crate) role: Fact<RoleAttributes>,
    pub(crate) memberships: Vec<RoleMembership>,
    pub(crate) write: WriteDetermination,
    pub(crate) security_definers: Vec<DefinerFunction>,
    pub(crate) rls: Vec<RlsTableFact>,
    pub(crate) publications: Fact<Vec<String>>,
    pub(crate) replication_slots: Fact<Vec<String>>,
    pub(crate) cdc: CdcPrerequisite,
    pub(crate) capabilities: Vec<CapabilityStatus>,
    pub(crate) dba_checklist: Vec<DbaItem>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TlsReport {
    pub(crate) mode: String,
    pub(crate) trust: String,
    pub(crate) hostname_verification: bool,
    pub(crate) negotiated_version: String,
    pub(crate) server_observation: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VersionFact {
    pub(crate) server_version_num: i64,
    pub(crate) server_version: String,
    pub(crate) major: i64,
    pub(crate) minor: i64,
    pub(crate) declared_minimum_major: i64,
    pub(crate) tested_versions: Vec<String>,
    pub(crate) admission: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BlockReason {
    pub(crate) code: String,
    pub(crate) fact: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fact<T> {
    pub(crate) status: String,
    pub(crate) value: Option<T>,
    pub(crate) reason: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Principal {
    pub(crate) current_user: String,
    pub(crate) session_user: String,
    pub(crate) database: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoleAttributes {
    pub(crate) name: String,
    pub(crate) superuser: bool,
    pub(crate) inherit: bool,
    pub(crate) create_role: bool,
    pub(crate) create_db: bool,
    pub(crate) can_login: bool,
    pub(crate) replication: bool,
    pub(crate) bypass_rls: bool,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoleMembership {
    pub(crate) role: String,
    pub(crate) inherit_option: bool,
    pub(crate) set_option: bool,
    pub(crate) admin_option: bool,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WriteDetermination {
    pub(crate) state: String,
    pub(crate) findings: Vec<WriteFinding>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WriteFinding {
    pub(crate) schema: String,
    pub(crate) table: String,
    pub(crate) role: String,
    pub(crate) privilege: String,
    pub(crate) path: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DefinerFunction {
    pub(crate) schema: String,
    pub(crate) name: String,
    pub(crate) owner: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RlsTableFact {
    pub(crate) schema: String,
    pub(crate) table: String,
    pub(crate) enabled: Option<bool>,
    pub(crate) forced: Option<bool>,
    pub(crate) policy_count: Option<u32>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CdcPrerequisite {
    pub(crate) wal_level: Fact<String>,
    pub(crate) replication_attribute: Fact<bool>,
    pub(crate) publication_for_selected: Fact<bool>,
    pub(crate) implemented: bool,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CapabilityStatus {
    pub(crate) name: String,
    pub(crate) supported: bool,
    pub(crate) reason: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DbaItem {
    pub(crate) id: String,
    pub(crate) level: String,
    pub(crate) risk: String,
    pub(crate) rollback: String,
    pub(crate) statement: String,
}

pub(crate) fn deserialize_access(bytes: &[u8]) -> Result<AccessReport, ParseError> {
    let report: AccessReport = parse::parse_strict(bytes)?;
    let _shape = shape_digest(&report);
    Ok(report)
}

pub fn adapt(bytes: &[u8]) -> Result<Decision, ParseError> {
    let report: AccessReport = parse::parse_strict(bytes)?;
    if report.protocol != PG_REPORT_PROTOCOL {
        return Err(ParseError {
            fact: "protocol".to_string(),
        });
    }
    let digest = sha256(bytes);
    let mut blocks = Vec::new();
    // The frozen connector report has no budget unit. Missing is unknown.
    blocks.push(block("BUDGET_UNIT_UNKNOWN", "source_report.budget_unit"));
    match report.write.state.as_str() {
        "read_only" => {}
        "unknown" => blocks.push(block("PERMISSION_UNKNOWN", "write.state")),
        "write_capable" => blocks.push(block("PERMISSION_NOT_READONLY", "write.state")),
        _ => blocks.push(block("PERMISSION_UNKNOWN", "write.state")),
    }
    // FactStatus is serde rename_all = snake_case: known, absent, unknown, not_collected.
    if report.role.status != "known" || report.principal.status != "known" {
        blocks.push(block("PERMISSION_UNKNOWN", "role"));
    }
    if report.role.status == "known"
        && let Some(role) = &report.role.value
    {
        if role.superuser
            || role.create_role
            || role.create_db
            || role.replication
            || role.bypass_rls
            || !role.can_login
        {
            blocks.push(block("PERMISSION_NOT_READONLY", "role.attributes"));
        }
        let _ = (&role.name, role.inherit);
    }
    if report
        .memberships
        .iter()
        .any(|edge| edge.admin_option || edge.set_option)
    {
        blocks.push(block("PERMISSION_NOT_READONLY", "memberships"));
    }
    if !report.security_definers.is_empty() {
        blocks.push(block("PERMISSION_NOT_READONLY", "security_definers"));
    }
    for finding in &report.write.findings {
        if !known(PERMISSIONS, &finding.privilege) || finding.privilege.is_empty() {
            blocks.push(block(
                "PERMISSION_UNKNOWN",
                &format!("write.findings.{}", finding.privilege),
            ));
        } else if !known(READ_ONLY_PERMISSIONS, &finding.privilege) {
            blocks.push(block(
                "PERMISSION_NOT_READONLY",
                &format!("write.findings.{}", finding.privilege),
            ));
        }
    }
    for reason in &report.block_reasons {
        blocks.push(block(&reason.code, &reason.fact));
    }
    if !report.registration_allowed && report.block_reasons.is_empty() {
        blocks.push(block("REGISTRATION_BLOCKED", "registration_allowed"));
    }
    let mut capabilities = Vec::new();
    for cap in &report.capabilities {
        if (cap.name == "source_write" || cap.name == "source_ddl") && cap.supported {
            blocks.push(block("SOURCE_WRITE_FORBIDDEN", &cap.name));
        }
        if cap.name == "scan" && cap.supported {
            capabilities.push(declared("Catalog"));
        }
        if cap.name == "bounded_read" && cap.supported {
            capabilities.push(declared("BatchFull"));
        }
        if cap.name == "cdc" && cap.supported {
            blocks.push(block("CDC_NOT_IN_SLICE", "capabilities.cdc"));
        }
    }
    if report.cdc.implemented {
        blocks.push(block("CDC_NOT_IN_SLICE", "cdc.implemented"));
    }
    let _shape = shape_digest(&report);
    let checklist = from_existing(
        &report
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
    );
    Ok(Decision {
        protocol: DECISION_PROTOCOL.to_string(),
        admitted: blocks.is_empty(),
        matrix_mutated: false,
        blocks,
        model: Model {
            capabilities,
            capture_methods: Vec::new(),
        },
        identity: Identity {
            target_id: String::new(),
            source_id: report.source_id,
            secret_ref: report.secret_ref,
            source_report_protocol: Some(report.protocol),
            core_contract: Some(report.core_contract),
            source_report_sha256: Some(digest),
        },
        dba_checklist: checklist,
        zero_write: ZeroWrite {
            proven: false,
            basis: "not_audited".to_string(),
        },
    })
}

fn declared(name: &str) -> DeclaredCapability {
    DeclaredCapability {
        name: name.to_string(),
        cursor: None,
        feed: None,
        cdc: None,
        token_order: None,
        budget_unit: None,
        level: "designed".to_string(),
        write_class: "R0".to_string(),
        consistency: "S0".to_string(),
    }
}

fn block(code: &str, fact: &str) -> Block {
    Block {
        code: code.to_string(),
        fact: fact.to_string(),
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

fn shape_digest(report: &AccessReport) -> usize {
    let principal = report
        .principal
        .value
        .as_ref()
        .map(|item| item.current_user.len() + item.session_user.len() + item.database.len())
        .unwrap_or(0);
    let recovery = report.in_recovery.value.unwrap_or(false) as usize;
    let members = report
        .memberships
        .iter()
        .map(|edge| edge.role.len() + edge.inherit_option as usize)
        .sum::<usize>();
    let findings = report
        .write
        .findings
        .iter()
        .map(|item| item.schema.len() + item.table.len() + item.role.len() + item.path.len())
        .sum::<usize>();
    let definers = report
        .security_definers
        .iter()
        .map(|item| item.schema.len() + item.name.len() + item.owner.len())
        .sum::<usize>();
    let rls = report
        .rls
        .iter()
        .map(|item| {
            item.schema.len()
                + item.table.len()
                + item.enabled.unwrap_or(false) as usize
                + item.forced.unwrap_or(false) as usize
                + item.policy_count.unwrap_or(0) as usize
        })
        .sum::<usize>();
    let cap_reasons = report
        .capabilities
        .iter()
        .map(|item| item.reason.len())
        .sum::<usize>();
    let pubs = report
        .publications
        .value
        .as_ref()
        .map(|v| v.len())
        .unwrap_or(0);
    let slots = report
        .replication_slots
        .value
        .as_ref()
        .map(|v| v.len())
        .unwrap_or(0);
    let wal = report
        .cdc
        .wal_level
        .value
        .as_ref()
        .map(|v| v.len())
        .unwrap_or(0);
    let repl = report.cdc.replication_attribute.value.unwrap_or(false) as usize;
    let pub_selected = report.cdc.publication_for_selected.value.unwrap_or(false) as usize;
    let reasons = report.role.reason.as_ref().map(|r| r.len()).unwrap_or(0)
        + report
            .principal
            .reason
            .as_ref()
            .map(|r| r.len())
            .unwrap_or(0)
        + report
            .in_recovery
            .reason
            .as_ref()
            .map(|r| r.len())
            .unwrap_or(0)
        + report
            .publications
            .reason
            .as_ref()
            .map(|r| r.len())
            .unwrap_or(0)
        + report
            .replication_slots
            .reason
            .as_ref()
            .map(|r| r.len())
            .unwrap_or(0)
        + report
            .cdc
            .wal_level
            .reason
            .as_ref()
            .map(|r| r.len())
            .unwrap_or(0)
        + report
            .cdc
            .replication_attribute
            .reason
            .as_ref()
            .map(|r| r.len())
            .unwrap_or(0)
        + report
            .cdc
            .publication_for_selected
            .reason
            .as_ref()
            .map(|r| r.len())
            .unwrap_or(0);
    report.tls.mode.len()
        + report.tls.trust.len()
        + report.tls.hostname_verification as usize
        + report.tls.negotiated_version.len()
        + report.tls.server_observation.len()
        + report.version.server_version.len()
        + report.version.admission.len()
        + report.version.tested_versions.len()
        + principal
        + recovery
        + members
        + findings
        + definers
        + rls
        + pubs
        + slots
        + wal
        + repl
        + pub_selected
        + cap_reasons
        + report.version.major.unsigned_abs() as usize
        + report.version.minor.unsigned_abs() as usize
        + report.version.declared_minimum_major.unsigned_abs() as usize
        + report.version.server_version_num.unsigned_abs() as usize
        + reasons
}
