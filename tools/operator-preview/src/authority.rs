//! Trusted operator file. A private file is not authority until it binds one.

use std::path::{Path, PathBuf};

use oo_gateway::policy::catalog::{CatalogLimits, UnknownRows};
use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass, PolicyError,
};
use serde::Deserialize;
use serde_json::Value;

use crate::durable::{read_capped, refuse_symlink};
use crate::output::Fail;
use crate::{BINDS, CONFIG_PROTOCOL, POLICY_PROTOCOL, QUERY_PROTOCOL};

const DOC_BYTES: usize = 1_048_576;
const FORBIDDEN: &[&str] = &[
    "source_password",
    "source_host",
    "source_url",
    "bearer",
    "token",
];

pub struct Authority {
    pub project: String,
    pub role: String,
    pub policy_pack_id: String,
    pub policy_version: String,
    pub knowledge_id: String,
    pub knowledge_version: String,
    pub import: ImportId,
    pub policy_path: PathBuf,
    pub pack_dir: PathBuf,
    pub receipt_path: PathBuf,
    pub role_pack_dir: PathBuf,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_path: PathBuf,
    pub ca_path: PathBuf,
    pub state_dir: PathBuf,
    pub audit_dir: PathBuf,
    pub receipt_dir: PathBuf,
    pub audit_max_bytes: u64,
    pub limits: CatalogLimits,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportId {
    pub import_generation: i64,
    pub import_hash: String,
    pub snapshot_id: String,
    pub source_id: String,
    pub manifest_sha256: String,
    pub target_config_sha256: String,
}

pub struct PolicyFile {
    pub project: String,
    pub config_digest: String,
    pub generation: i64,
    pub role_slug: String,
    pub login: String,
    pub acl: ColumnAclPolicy,
}

pub struct QueryFile {
    pub sql: String,
    pub max_rows: u32,
    pub max_result_bytes: u64,
    pub wall_clock_ms: u64,
}

pub fn load_authority(path: &Path) -> Result<Authority, Fail> {
    let value = read_value(path)?;
    let base = path.parent().ok_or(Fail::Request)?;
    let doc: ConfigDoc = serde_json::from_value(value).map_err(|_| Fail::Request)?;
    if doc.protocol != CONFIG_PROTOCOL
        || doc.authority.kind != "trusted-operator-file"
        || doc.authority.binds != BINDS
    {
        return Err(Fail::Request);
    }
    if doc.audit_max_bytes < 64 {
        return Err(Fail::Request);
    }
    let unknown_rows = match doc.catalog.unknown_rows.as_str() {
        "refuse" => UnknownRows::Refuse,
        "zero" => UnknownRows::Zero,
        "saturate" => UnknownRows::Saturate,
        _ => return Err(Fail::Request),
    };
    let import = doc.import.checked()?;
    if doc.project != import.source_id {
        return Err(Fail::Request);
    }
    let authority = Authority {
        project: doc.project,
        role: doc.role,
        policy_pack_id: doc.policy_pack_id,
        policy_version: doc.policy_version,
        knowledge_id: doc.knowledge.id,
        knowledge_version: doc.knowledge.version,
        import,
        policy_path: anchored(base, &doc.policy_path)?,
        pack_dir: anchored(base, &doc.pack_dir)?,
        receipt_path: anchored(base, &doc.pack_receipt)?,
        role_pack_dir: anchored(base, &doc.role_pack_dir)?,
        host: doc.target.host,
        port: doc.target.port,
        database: doc.target.database,
        user: doc.target.user,
        password_path: anchored(base, &doc.target.password_path)?,
        ca_path: anchored(base, &doc.target.ca_path)?,
        state_dir: anchored(base, &doc.state_dir)?,
        audit_dir: anchored(base, &doc.audit_dir)?,
        receipt_dir: anchored(base, &doc.receipt_dir)?,
        audit_max_bytes: doc.audit_max_bytes,
        limits: CatalogLimits {
            max_tables: doc.catalog.max_tables,
            max_columns: doc.catalog.max_columns,
            max_functions: doc.catalog.max_functions,
            max_operators: doc.catalog.max_operators,
            large_table_row_threshold: doc.catalog.large_table_row_threshold,
            unknown_rows,
        },
    };
    if !host_ok(&authority.host) || authority.port == 0 {
        return Err(Fail::Request);
    }
    for path in [
        &authority.policy_path,
        &authority.pack_dir,
        &authority.receipt_path,
        &authority.role_pack_dir,
        &authority.password_path,
        &authority.ca_path,
    ] {
        refuse_symlink(path)?;
    }
    Ok(authority)
}

pub fn load_policy(path: &Path) -> Result<PolicyFile, Fail> {
    let value = read_value(path)?;
    if value.get("row_filter").is_some() || value.get("policies").is_some() {
        return Err(Fail::Request);
    }
    let doc: PolicyDoc = serde_json::from_value(value).map_err(|_| Fail::Request)?;
    if doc.protocol != POLICY_PROTOCOL {
        return Err(Fail::Request);
    }
    let mut columns = Vec::with_capacity(doc.table.columns.len());
    for column in &doc.table.columns {
        columns.push(column.to_spec()?);
    }
    let acl = ColumnAclPolicy {
        project: doc.project.clone(),
        config_digest: doc.config_digest.clone(),
        generation: doc.generation,
        role: doc.role.clone(),
        login: doc.login.clone(),
        table: LogicalTable {
            namespace: doc.table.namespace,
            name: doc.table.name,
            columns,
        },
        row_filter: None,
    };
    Ok(PolicyFile {
        project: doc.project,
        config_digest: doc.config_digest,
        generation: doc.generation,
        role_slug: doc.role,
        login: doc.login,
        acl,
    })
}

pub fn load_query(path: &Path) -> Result<QueryFile, Fail> {
    let value = read_value(path)?;
    if value.get("policies").is_some() || value.get("row_filter").is_some() {
        return Err(Fail::Request);
    }
    let doc: QueryDoc = serde_json::from_value(value).map_err(|_| Fail::Request)?;
    if doc.protocol != QUERY_PROTOCOL {
        return Err(Fail::Request);
    }
    if doc.sql.is_empty() || doc.sql.len() > 65_536 || doc.sql.contains('\0') {
        return Err(Fail::Request);
    }
    if doc.max_rows == 0
        || doc.max_rows > 2_000
        || doc.max_result_bytes == 0
        || doc.max_result_bytes > 256 * 1024
        || doc.wall_clock_ms == 0
        || doc.wall_clock_ms > 30_000
    {
        return Err(Fail::Request);
    }
    Ok(QueryFile {
        sql: doc.sql,
        max_rows: doc.max_rows,
        max_result_bytes: doc.max_result_bytes,
        wall_clock_ms: doc.wall_clock_ms,
    })
}

pub fn policy_compile_error(err: PolicyError) -> Fail {
    match err {
        PolicyError::CertificateRejected => Fail::Tls,
        _ => Fail::Policy,
    }
}

fn read_value(path: &Path) -> Result<Value, Fail> {
    let bytes = read_capped(path, DOC_BYTES)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Fail::Request)?;
    let value: Value = serde_json::from_str(text).map_err(|_| Fail::Request)?;
    if forbidden_key(&value) {
        return Err(Fail::Request);
    }
    Ok(value)
}

fn forbidden_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.keys().any(|key| FORBIDDEN.contains(&key.as_str()))
            || map.values().any(forbidden_key),
        Value::Array(items) => items.iter().any(forbidden_key),
        _ => false,
    }
}

fn anchored(base: &Path, value: &str) -> Result<PathBuf, Fail> {
    if value.is_empty() || value.contains('\0') {
        return Err(Fail::Request);
    }
    let path = PathBuf::from(value);
    let full = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    if full.as_os_str().is_empty() {
        return Err(Fail::Request);
    }
    Ok(full)
}

fn host_ok(host: &str) -> bool {
    let bytes = host.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 253
        && !bytes.starts_with(b"-")
        && !bytes.ends_with(b"-")
        && !bytes.starts_with(b".")
        && !bytes.ends_with(b".")
        && !host.contains("..")
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'.' || *byte == b'-')
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigDoc {
    protocol: String,
    authority: AuthorityDoc,
    project: String,
    role: String,
    policy_pack_id: String,
    policy_version: String,
    knowledge: VersionDoc,
    import: ImportDoc,
    policy_path: String,
    pack_dir: String,
    pack_receipt: String,
    role_pack_dir: String,
    target: TargetDoc,
    state_dir: String,
    audit_dir: String,
    receipt_dir: String,
    audit_max_bytes: u64,
    catalog: CatalogDoc,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityDoc {
    kind: String,
    binds: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionDoc {
    id: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportDoc {
    import_generation: i64,
    import_hash: String,
    snapshot_id: String,
    source_id: String,
    manifest_sha256: String,
    target_config_sha256: String,
}

impl ImportDoc {
    fn checked(self) -> Result<ImportId, Fail> {
        if self.import_generation < 1
            || !hex64(&self.import_hash)
            || !hex64(&self.manifest_sha256)
            || !hex64(&self.target_config_sha256)
            || self.snapshot_id.is_empty()
            || self.snapshot_id.len() > 128
            || self.source_id.is_empty()
            || self.source_id.len() > 63
        {
            return Err(Fail::Request);
        }
        Ok(ImportId {
            import_generation: self.import_generation,
            import_hash: self.import_hash,
            snapshot_id: self.snapshot_id,
            source_id: self.source_id,
            manifest_sha256: self.manifest_sha256,
            target_config_sha256: self.target_config_sha256,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetDoc {
    host: String,
    port: u16,
    database: String,
    user: String,
    ca_path: String,
    password_path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogDoc {
    max_tables: u32,
    max_columns: u32,
    max_functions: u32,
    max_operators: u32,
    large_table_row_threshold: u64,
    unknown_rows: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDoc {
    protocol: String,
    project: String,
    config_digest: String,
    generation: i64,
    role: String,
    login: String,
    table: TableDoc,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableDoc {
    namespace: String,
    name: String,
    columns: Vec<ColumnDoc>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnDoc {
    name: String,
    status: String,
    semantic_type: String,
    pii: PiiDoc,
}

impl ColumnDoc {
    fn to_spec(&self) -> Result<ColumnSpec, Fail> {
        let status = match self.status.as_str() {
            "confirmed" => ClassStatus::Confirmed,
            "auto_accepted" => ClassStatus::AutoAccepted,
            "unclassified" => ClassStatus::Unclassified,
            _ => return Err(Fail::Request),
        };
        Ok(ColumnSpec {
            name: self.name.clone(),
            status,
            semantic_type: self.semantic_type.clone(),
            pii: self.pii.to_class()?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PiiDoc {
    class: String,
    #[serde(default)]
    explicit_allow: Option<bool>,
}

impl PiiDoc {
    fn to_class(&self) -> Result<PiiClass, Fail> {
        match self.class.as_str() {
            "none" if self.explicit_allow.is_none() => Ok(PiiClass::None),
            "unknown" if self.explicit_allow.is_none() => Ok(PiiClass::Unknown),
            "declared" => Ok(PiiClass::Declared {
                explicit_allow: self.explicit_allow.ok_or(Fail::Request)?,
            }),
            _ => Err(Fail::Request),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryDoc {
    protocol: String,
    sql: String,
    max_rows: u32,
    max_result_bytes: u64,
    wall_clock_ms: u64,
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{load_authority, load_query};
    use crate::output::Fail;

    fn scratch() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("oo-dl01w1-21-auth-{nanos}"));
        fs::create_dir_all(&path).expect("dir");
        path
    }

    fn write(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).expect("write");
        path
    }

    #[test]
    fn private_file_without_binds_is_not_authority() {
        let dir = scratch();
        let path = write(
            &dir,
            "config.json",
            r#"{"protocol":"oo.operator-preview.config/v1","note":"private"}"#,
        );
        assert_eq!(load_authority(&path).err(), Some(Fail::Request));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn query_document_cannot_carry_policy() {
        let dir = scratch();
        let path = write(
            &dir,
            "query.json",
            r#"{"protocol":"oo.operator-preview.query/v1","sql":"SELECT 1","policies":[],"max_rows":1,"max_result_bytes":8,"wall_clock_ms":1000}"#,
        );
        assert_eq!(load_query(&path).err(), Some(Fail::Request));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn source_credential_field_is_refused() {
        let dir = scratch();
        let path = write(
            &dir,
            "config.json",
            r#"{"protocol":"oo.operator-preview.config/v1","authority":{"kind":"trusted-operator-file","binds":"import+mapping+role-policy+pack+target-tls"},"source_password":"no"}"#,
        );
        assert_eq!(load_authority(&path).err(), Some(Fail::Request));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn symlink_authority_is_refused() {
        let dir = scratch();
        let real = write(&dir, "real.json", "{}");
        let link = dir.join("link.json");
        symlink(&real, &link).expect("link");
        assert_eq!(load_authority(&link).err(), Some(Fail::Request));
        let _ = fs::remove_dir_all(dir);
    }
}
