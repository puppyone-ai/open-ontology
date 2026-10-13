//! Operator config. Paths and secret refs stay server-side.

use serde::Deserialize;
use serde_json::Value;

use crate::error::ApiError;
use crate::ids::{hex64, ident_ok, secret_ref_ok};
use crate::jsonutil::parse_exact;

pub const CONFIG_CONTRACT: &str = "oo.selfbuild-control-api.config/v1";
pub const INSTALL_USER: &str = "oo_selfbuild_api_admin";

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub deployment_origins: Vec<String>,
    pub listen: Listen,
    pub identity: IdentityCfg,
    pub postgres: Endpoint,
    pub work_dir: String,
    pub model_cache_dir: String,
    pub binaries: Binaries,
    pub projects: Vec<ProjectCfg>,
    pub ledger: Endpoint,
    pub principals: Vec<PrincipalCfg>,
}

#[derive(Clone, Debug)]
pub struct Listen {
    pub bind: String,
    pub cert_file: String,
    pub key_file: String,
}

#[derive(Clone, Debug)]
pub struct IdentityCfg {
    pub origin: String,
    pub ca_file: String,
}

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_file: String,
    pub ca_file: String,
    pub server_name: String,
}

#[derive(Clone, Debug)]
pub struct Binaries {
    pub catalog: String,
    pub pg_builder: String,
    pub rules: String,
    pub suggestions: String,
    pub review_ledger: String,
}

#[derive(Clone, Debug)]
pub struct ProjectCfg {
    pub project_id: String,
    pub config_commit: String,
    pub source_ref: String,
    pub allowance_id: String,
    pub llm_secret_ref: String,
    pub source_secret_ref: String,
    pub source: SourceCfg,
    pub sample_policy: String,
    pub rules_auto_accept: String,
    pub llm: LlmCfg,
    pub role_policy_file: String,
    pub hidden_columns: Vec<String>,
    pub visible_columns: Vec<String>,
    pub sealed_bundle: String,
    pub source_protocol: Option<SourceProtocol>,
}

#[derive(Clone, Debug)]
pub struct SourceProtocol {
    pub catalog: String,
    pub profile: String,
    pub semantics: String,
    pub source_commit: String,
    pub semantics_commit: String,
    pub type_basis: Vec<TypeBasis>,
    pub schema_audit: Vec<SchemaAudit>,
}

#[derive(Clone, Debug)]
pub struct SchemaAudit {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct TypeBasis {
    pub protocol: String,
    pub outer_protocol: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct SourceCfg {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password_file: String,
    pub ca_file: String,
    pub tables: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct LlmCfg {
    pub bearer_file: String,
    pub origin: String,
    pub model_id: String,
}

#[derive(Clone, Debug)]
pub struct PrincipalCfg {
    pub subject: String,
    pub ledger_role: String,
    pub principal_id: String,
    pub reviewer_label: String,
    pub credential_file: String,
    pub object_scope: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    contract: String,
    deployment_origins: Vec<String>,
    listen: FileListen,
    identity: FileIdentity,
    postgres: FileEndpoint,
    work_dir: String,
    #[serde(default)]
    model_cache_dir: String,
    binaries: FileBinaries,
    projects: Vec<FileProject>,
    ledger: FileEndpoint,
    principals: Vec<FilePrincipal>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileListen {
    bind: String,
    cert_file: String,
    key_file: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileIdentity {
    origin: String,
    ca_file: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEndpoint {
    host: String,
    port: u16,
    database: String,
    user: String,
    password_file: String,
    ca_file: String,
    server_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileBinaries {
    catalog: String,
    pg_builder: String,
    rules: String,
    suggestions: String,
    review_ledger: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileProject {
    project_id: String,
    config_commit: String,
    source_ref: String,
    allowance_id: String,
    llm_secret_ref: String,
    source_secret_ref: String,
    source: FileSource,
    sample_policy: String,
    rules_auto_accept: String,
    llm: FileLlm,
    role_policy_file: String,
    hidden_columns: Vec<String>,
    visible_columns: Vec<String>,
    #[serde(default)]
    sealed_bundle: String,
    #[serde(default)]
    source_protocol: Option<FileSourceProtocol>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSourceProtocol {
    catalog: String,
    profile: String,
    semantics: String,
    source_commit: String,
    semantics_commit: String,
    type_basis: Vec<FileTypeBasis>,
    #[serde(default)]
    schema_audit: Vec<FileSchemaAudit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSchemaAudit {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileTypeBasis {
    protocol: String,
    outer_protocol: String,
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSource {
    host: String,
    port: u16,
    database: String,
    username: String,
    password_file: String,
    ca_file: String,
    tables: Vec<FileTable>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileTable {
    schema: String,
    table: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileLlm {
    bearer_file: String,
    origin: String,
    model_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePrincipal {
    subject: String,
    ledger_role: String,
    principal_id: String,
    reviewer_label: String,
    credential_file: String,
    object_scope: Vec<String>,
}

pub fn load_config_bytes(bytes: &[u8]) -> Result<AppConfig, ApiError> {
    let value = parse_exact(bytes)?;
    let file: FileConfig = serde_json::from_value(value).map_err(|_| ApiError::malformed())?;
    validate(file)
}

pub fn load_config_file(path: &str) -> Result<AppConfig, ApiError> {
    let bytes = std::fs::read(path).map_err(|_| ApiError::malformed())?;
    if bytes.len() > 65_536 {
        return Err(ApiError::malformed());
    }
    load_config_bytes(&bytes)
}

fn validate(file: FileConfig) -> Result<AppConfig, ApiError> {
    if file.contract != CONFIG_CONTRACT || file.deployment_origins.is_empty() || file.projects.is_empty() {
        return Err(ApiError::malformed());
    }
    if file.deployment_origins.iter().any(|origin| !https_origin_ok(origin) || origin.contains('*')) {
        return Err(ApiError::malformed());
    }
    if !https_origin_ok(&file.identity.origin) {
        return Err(ApiError::malformed());
    }
    if file.listen.bind.parse::<std::net::SocketAddr>().is_err() {
        return Err(ApiError::malformed());
    }
    if !absolute_path(&file.work_dir)
        || !absolute_path(&file.listen.cert_file)
        || !absolute_path(&file.listen.key_file)
        || !absolute_path(&file.identity.ca_file)
        || (!file.model_cache_dir.is_empty() && !absolute_path(&file.model_cache_dir))
    {
        return Err(ApiError::malformed());
    }
    let postgres = endpoint(file.postgres, true)?;
    let ledger = endpoint(file.ledger, false)?;
    let binaries = Binaries {
        catalog: absolute_required(file.binaries.catalog)?,
        pg_builder: absolute_required(file.binaries.pg_builder)?,
        rules: absolute_required(file.binaries.rules)?,
        suggestions: absolute_required(file.binaries.suggestions)?,
        review_ledger: absolute_required(file.binaries.review_ledger)?,
    };
    let mut projects = Vec::new();
    let mut seen = Vec::new();
    for project in file.projects {
        if !seen.iter().all(|id: &String| id != &project.project_id) {
            return Err(ApiError::malformed());
        }
        seen.push(project.project_id.clone());
        projects.push(project_cfg(project)?);
    }
    let mut principals = Vec::new();
    for principal in file.principals {
        principals.push(principal_cfg(principal)?);
    }
    Ok(AppConfig {
        deployment_origins: file.deployment_origins,
        listen: Listen {
            bind: file.listen.bind,
            cert_file: file.listen.cert_file,
            key_file: file.listen.key_file,
        },
        identity: IdentityCfg {
            origin: file.identity.origin,
            ca_file: file.identity.ca_file,
        },
        postgres,
        work_dir: file.work_dir,
        model_cache_dir: file.model_cache_dir,
        binaries,
        projects,
        ledger,
        principals,
    })
}

fn project_cfg(project: FileProject) -> Result<ProjectCfg, ApiError> {
    if !ident_ok(&project.project_id)
        || !ident_ok(&project.source_ref)
        || !ident_ok(&project.allowance_id)
        || !hex64(&project.config_commit)
        || !secret_ref_ok(&project.llm_secret_ref)
        || !secret_ref_ok(&project.source_secret_ref)
        || project.sample_policy != "none"
        || !matches!(project.rules_auto_accept.as_str(), "none" | "rule_high_confidence")
        || project.llm.origin != "http://127.0.0.1:8317"
        || !matches!(project.llm.model_id.as_str(), "grok-4.7" | "gpt-6.1-sol")
        || !absolute_path(&project.llm.bearer_file)
        || !absolute_path(&project.role_policy_file)
    {
        return Err(ApiError::malformed());
    }
    if project.source.tables.is_empty() || project.source.tables.len() > 16 || project.source.port == 0 {
        return Err(ApiError::malformed());
    }
    if !host_ok(&project.source.host)
        || !sql_ident(&project.source.database)
        || !sql_ident(&project.source.username)
        || !absolute_path(&project.source.password_file)
        || !absolute_path(&project.source.ca_file)
    {
        return Err(ApiError::malformed());
    }
    let mut tables = Vec::new();
    for table in project.source.tables {
        if !crate::ids::source_ident_ok(&table.schema) || !crate::ids::source_ident_ok(&table.table) {
            return Err(ApiError::malformed());
        }
        tables.push((table.schema, table.table));
    }
    for column in project.hidden_columns.iter().chain(project.visible_columns.iter()) {
        if column.split('.').count() != 3 || !column.split('.').all(crate::ids::source_ident_ok) {
            return Err(ApiError::malformed());
        }
    }
    if !project.sealed_bundle.is_empty() && !absolute_path(&project.sealed_bundle) {
        return Err(ApiError::malformed());
    }
    let source_protocol = match project.source_protocol {
        None => None,
        Some(item) => Some(source_protocol_cfg(item)?),
    };
    if source_protocol.is_some() && project.sealed_bundle.is_empty() {
        return Err(ApiError::malformed());
    }
    Ok(ProjectCfg {
        project_id: project.project_id,
        config_commit: project.config_commit,
        source_ref: project.source_ref,
        allowance_id: project.allowance_id,
        llm_secret_ref: project.llm_secret_ref,
        source_secret_ref: project.source_secret_ref,
        source: SourceCfg {
            host: project.source.host,
            port: project.source.port,
            database: project.source.database,
            username: project.source.username,
            password_file: project.source.password_file,
            ca_file: project.source.ca_file,
            tables,
        },
        sample_policy: project.sample_policy,
        rules_auto_accept: project.rules_auto_accept,
        llm: LlmCfg {
            bearer_file: project.llm.bearer_file,
            origin: project.llm.origin,
            model_id: project.llm.model_id,
        },
        role_policy_file: project.role_policy_file,
        hidden_columns: project.hidden_columns,
        visible_columns: project.visible_columns,
        sealed_bundle: project.sealed_bundle,
        source_protocol,
    })
}

fn source_protocol_cfg(file: FileSourceProtocol) -> Result<SourceProtocol, ApiError> {
    let measured = crate::adapt::measured_catalog(&file.catalog)
        || matches!(file.catalog.as_str(), "oo.pipeline.sealed-import-catalog/v1" | "oo.pipeline.sealed-import-collection/v1");
    if measured {
        if file.profile.is_empty()
            || file.profile.len() > 128
            || file.semantics.is_empty()
            || file.semantics.len() > 128
            || file.profile == crate::adapt::CAPTURE_COMPARE
            || !crate::adapt::git_commit(&file.source_commit)
            || !crate::adapt::git_commit(&file.semantics_commit)
            || file.type_basis.len() > 8
        {
            return Err(ApiError::malformed());
        }
    } else if file.catalog != crate::adapt::CAPTURE_CATALOG
        || file.profile != crate::adapt::CAPTURE_COMPARE
        || file.semantics != crate::adapt::SEMANTICS_PROFILE
        || !crate::adapt::git_commit(&file.source_commit)
        || !crate::adapt::git_commit(&file.semantics_commit)
        || file.type_basis.is_empty()
        || file.type_basis.len() > 8
    {
        return Err(ApiError::malformed());
    }
    let mut type_basis = Vec::new();
    for item in file.type_basis {
        if item.protocol != crate::adapt::SEALED_MANIFEST
            || item.outer_protocol != crate::adapt::CATALOG_IMPORT
            || !crate::ids::hex64(&item.sha256)
            || !absolute_path(&item.path)
        {
            return Err(ApiError::malformed());
        }
        type_basis.push(TypeBasis {
            protocol: item.protocol,
            outer_protocol: item.outer_protocol,
            path: item.path,
            sha256: item.sha256,
        });
    }
    if file.schema_audit.len() > 4 {
        return Err(ApiError::malformed());
    }
    let mut schema_audit = Vec::new();
    for item in file.schema_audit {
        if !crate::ids::hex64(&item.sha256) || !absolute_path(&item.path) || schema_audit.iter().any(|prior: &SchemaAudit| prior.path == item.path) {
            return Err(ApiError::malformed());
        }
        schema_audit.push(SchemaAudit {
            path: item.path,
            sha256: item.sha256,
        });
    }
    Ok(SourceProtocol {
        catalog: file.catalog,
        profile: file.profile,
        semantics: file.semantics,
        source_commit: file.source_commit,
        semantics_commit: file.semantics_commit,
        type_basis,
        schema_audit,
    })
}

fn principal_cfg(principal: FilePrincipal) -> Result<PrincipalCfg, ApiError> {
    if principal.subject.is_empty()
        || principal.subject.len() > 256
        || !matches!(principal.ledger_role.as_str(), "reviewer" | "owner" | "auditor")
        || principal.principal_id.is_empty()
        || principal.reviewer_label.is_empty()
        || !absolute_path(&principal.credential_file)
        || principal.object_scope.is_empty()
        || principal.object_scope.len() > 64
        || principal.object_scope.iter().any(|item| !crate::ids::object_id_ok(item))
    {
        return Err(ApiError::malformed());
    }
    Ok(PrincipalCfg {
        subject: principal.subject,
        ledger_role: principal.ledger_role,
        principal_id: principal.principal_id,
        reviewer_label: principal.reviewer_label,
        credential_file: principal.credential_file,
        object_scope: principal.object_scope,
    })
}

fn endpoint(file: FileEndpoint, install_user: bool) -> Result<Endpoint, ApiError> {
    if file.port == 0
        || !host_ok(&file.host)
        || !host_ok(&file.server_name)
        || !sql_ident(&file.database)
        || !sql_ident(&file.user)
        || !absolute_path(&file.password_file)
        || !absolute_path(&file.ca_file)
    {
        return Err(ApiError::malformed());
    }
    if install_user
        && file.user != INSTALL_USER
        && !(file.user.starts_with("oo_") && file.user.ends_with("_selfbuild_api_admin"))
    {
        return Err(ApiError::malformed());
    }
    Ok(Endpoint {
        host: file.host,
        port: file.port,
        database: file.database,
        user: file.user,
        password_file: file.password_file,
        ca_file: file.ca_file,
        server_name: file.server_name,
    })
}

pub fn https_origin_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    if rest.is_empty() || rest.contains(['/', '?', '#', '@', ' ', '*']) {
        return false;
    }
    if let Some((host, port)) = rest.rsplit_once(':') {
        !host.is_empty()
            && !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().ok().is_some_and(|port| port > 0)
            && host_ok(host)
    } else {
        host_ok(rest)
    }
}

fn host_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && !value.contains("://")
}

fn sql_ident(value: &str) -> bool {
    (1..=63).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && value.as_bytes()[0].is_ascii_alphabetic()
}

fn absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 512
        && !value.contains('\0')
        && !value.contains("//")
        && !value.split('/').any(|part| part == "..")
}

fn absolute_required(value: String) -> Result<String, ApiError> {
    if absolute_path(&value) {
        Ok(value)
    } else {
        Err(ApiError::malformed())
    }
}

pub fn project_input_hash(project: &ProjectCfg) -> String {
    let tables: Vec<Value> = project
        .source
        .tables
        .iter()
        .map(|(schema, table)| serde_json::json!({"schema": schema, "table": table}))
        .collect();
    let mut body = serde_json::json!({
        "allowance_id": project.allowance_id,
        "config_commit": project.config_commit,
        "project_id": project.project_id,
        "source_ref": project.source_ref,
        "tables": tables
    });
    if !project.sealed_bundle.is_empty() {
        body["sealed_bundle"] = serde_json::json!(project.sealed_bundle);
    }
    if let Some(protocol) = &project.source_protocol {
        let basis: Vec<Value> = protocol
            .type_basis
            .iter()
            .map(|item| {
                serde_json::json!({
                    "outer_protocol": item.outer_protocol,
                    "path": item.path,
                    "protocol": item.protocol,
                    "sha256": item.sha256
                })
            })
            .collect();
        let mut pinned = serde_json::json!({
            "catalog": protocol.catalog,
            "profile": protocol.profile,
            "semantics": protocol.semantics,
            "semantics_commit": protocol.semantics_commit,
            "source_commit": protocol.source_commit,
            "type_basis": basis
        });
        if !protocol.schema_audit.is_empty() {
            let audits: Vec<Value> = protocol
                .schema_audit
                .iter()
                .map(|item| serde_json::json!({"path": item.path, "sha256": item.sha256}))
                .collect();
            pinned["schema_audit"] = serde_json::json!(audits);
        }
        body["source_protocol"] = pinned;
    }
    crate::canonical::canonical_sha256(&body)
}

pub fn regular_file(path: &str) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => meta.file_type().is_file(),
        Err(_) => false,
    }
}

pub fn binaries_ready(binaries: &Binaries) -> bool {
    regular_file(&binaries.catalog)
        && regular_file(&binaries.pg_builder)
        && regular_file(&binaries.rules)
        && regular_file(&binaries.suggestions)
        && regular_file(&binaries.review_ledger)
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;

    #[test]
    fn scoped_install_login_can_use_certificate_dns_name() {
        let file = || FileEndpoint {
            host: "oo-dl01w1-41-pg-native".into(),
            server_name: "oo-dl01w1-41-pg".into(),
            port: 5432,
            database: "oo_w141_selfbuild".into(),
            user: "oo_w141_selfbuild_api_admin".into(),
            password_file: "/private/password".into(),
            ca_file: "/private/ca.crt".into(),
        };
        let accepted = endpoint(file(), true).unwrap();
        assert_eq!(accepted.host, "oo-dl01w1-41-pg-native");
        assert_eq!(accepted.server_name, "oo-dl01w1-41-pg");
        let mut unsafe_user = file();
        unsafe_user.user = "postgres".into();
        assert!(endpoint(unsafe_user, true).is_err());
        let mut unsafe_name = file();
        unsafe_name.server_name = "*.example.com".into();
        assert!(endpoint(unsafe_name, true).is_err());
    }
}
