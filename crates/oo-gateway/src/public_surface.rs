//! Authenticated public HTTP surface.
//!
//! Callers reach this module only after the opaque bearer check. The client
//! does not choose a role, a policy version, or a knowledge version. SQL is
//! admitted by `guard_select` and run as the token's `NOLOGIN` role. Audit
//! admission is a local fsync WAL before any result byte. Export is not
//! served. This module does not open a source database.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path as AxumPath, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use serde_json::{Map, Value};
use tokio::sync::Mutex;
use zeroize::Zeroize;

use oo_core::api::{
    ErrorBody, ErrorCode, ErrorEnvelope, Freshness, FreshnessState, QueryCell, QueryColumn,
    QueryFormat, QueryRequest, QueryResponse, TruncationReason, Warning, WarningCode,
    parse_api_json,
};
use oo_core::config::ConsistencyMode;
use oo_core::decimal::{ExactDecimal, Position};
use oo_core::rfc3339::Rfc3339Timestamp;
use oo_core::text::{ErrorText, Label, QualifiedTable, QueryId, SourceName, TableName};

use crate::http_error::JSON_MEDIA_TYPE;
use crate::http_gateway::{AppState, PublicCaller};
use crate::pack_catalog::PackCatalog;
use crate::policy::catalog::{CatalogError, CatalogLimits, UnknownRows, load_policy_catalog};
use crate::policy::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass, TlsEndpoint,
    compile_column_acl, connect_require_tls, rustls_connector,
};
use crate::publication_binding::{BindingCheck, BindingReceipt, DRIFT_HINT, Expected};
use crate::query_executor::{
    Admission, AuditAdmission, Cell, CompletionRecord, DedicatedSession, ExecError,
    ImportRunObserved, ImportTableObserved, MAX_QUERY_BYTES, MAX_QUERY_ROWS, MAX_QUERY_WALL,
    MappingObserved, QualityCount, QualitySnapshot, QualityWarning, ReceiptObserved, ResultColumn,
    SelectRequest, SelectStream, WatermarkObserved, observe_quality,
};
use crate::reviewed_policy::{
    ReviewedAllow, ReviewedBinding, ReviewedIdentity, ReviewedSourceSeal, compile_reviewed_policies,
};
use crate::sql_guard::{GuardError, OutputItem, PolicyIdentity, guard_select};

#[path = "query_lifecycle.rs"]
mod query_lifecycle;
use query_lifecycle::{QueryState, RequestLifetime, SessionLease};

#[cfg(test)]
#[path = "f22_owned_pg_tests.rs"]
mod f22_owned_pg;

#[cfg(test)]
#[path = "performance_owned_pg_tests.rs"]
mod performance_owned_pg;

#[cfg(test)]
#[path = "published_cut_owned_pg_tests.rs"]
mod published_cut_owned_pg;

#[cfg(test)]
#[path = "../tests/token_meta_pg/mod.rs"]
mod token_meta_pg;

#[derive(Clone, Copy)]
struct AuthStamp {
    remaining_ms: u128,
    hit: bool,
}

struct CatalogFailure {
    code: ErrorCode,
    hint: Option<String>,
}

impl From<ErrorCode> for CatalogFailure {
    fn from(code: ErrorCode) -> Self {
        Self { code, hint: None }
    }
}

impl From<CatalogError> for CatalogFailure {
    fn from(error: CatalogError) -> Self {
        let hint = match error {
            CatalogError::PublicationBindingUnavailable => Some(
                "phase=runtime_catalog; reason=publication_binding_unavailable; \
                 verify the coordinated Pack, reviewed artifact and publication fence; \
                 do not rewrite SQL or widen permissions"
                    .to_owned(),
            ),
            _ => None,
        };
        Self {
            code: ErrorCode::SchemaInvalid,
            hint,
        }
    }
}

tokio::task_local! {
    static AUTH_STAMP: AuthStamp;
}

const DATA_KEYS: &[&str] = &[
    "data_endpoint_file",
    "data_password_file",
    "data_ca_file",
    "data_user",
    "wal_dir",
    "reviewed_dir",
];

const CATALOG_LIMITS: CatalogLimits = CatalogLimits {
    max_tables: 32,
    max_columns: 512,
    max_functions: crate::policy::catalog::FUNCTION_CATALOG_LIMIT,
    max_operators: 128,
    large_table_row_threshold: 1_000_000_000,
    unknown_rows: UnknownRows::Zero,
};

/// True for an optional data-plane key. Absent keys leave the older config valid.
pub(crate) fn data_config_key(key: &str) -> bool {
    DATA_KEYS.contains(&key) || key == "audit_collector_dir"
}

#[derive(Clone, PartialEq, Eq)]
struct Published {
    mapping_source: Option<String>,
    policies: Vec<ColumnAclPolicy>,
    role: String,
    project: String,
    policy_version: String,
    knowledge_version: String,
    config_digest: String,
    native_vector: Option<Value>,
    native_catalog: Option<Value>,
    quality_vector: Option<Value>,
}

impl Published {
    fn bound_quality_vector(&self) -> Option<Value> {
        let mut vector = self.quality_vector.clone()?;
        if let Some(catalog) = self
            .native_catalog
            .as_ref()
            .filter(|c| c["protocol"] == "oo.gateway.library-binding/v1")
        {
            for entry in vector.as_array_mut()? {
                entry["_published_library"] = serde_json::json!({
                    "library_id": catalog["library_id"], "receipt_sha256": catalog["receipt_sha256"],
                });
            }
        }
        Some(vector)
    }
}

#[derive(PartialEq, Eq)]
struct ReviewedFileStamp {
    path: PathBuf,
    length: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

struct ReviewedSnapshot {
    files: Vec<ReviewedFileStamp>,
    roles: Arc<Vec<Published>>,
}

struct ReviewedRoles {
    directory: Option<PathBuf>,
    current: std::sync::Mutex<ReviewedSnapshot>,
}

fn reviewed_files(directory: &Path) -> Result<Vec<ReviewedFileStamp>, &'static str> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).map_err(|_| "reviewed_dir")? {
        let entry = entry.map_err(|_| "reviewed_dir")?;
        if !entry.file_type().map_err(|_| "reviewed_dir")?.is_dir() {
            return Err("reviewed_dir");
        }
        one_segment(entry.file_name().to_str().ok_or("reviewed_dir")?)?;
        paths.push(entry.path());
        for name in [
            "library.json",
            "native-input.json",
            "command.json",
            "artifact.json",
            "decisions.json",
            "binding.json",
            "policy.json",
            "visibility.json",
        ] {
            let path = entry.path().join(name);
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_file() => paths.push(path),
                Ok(_) => return Err("reviewed_input"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("reviewed_input"),
            }
        }
    }
    if paths.is_empty() {
        return Err("reviewed_dir");
    }
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let meta = fs::symlink_metadata(&path).map_err(|_| "reviewed_input")?;
            if meta.file_type().is_symlink() {
                return Err("reviewed_input");
            }
            #[cfg(unix)]
            let identity = {
                use std::os::unix::fs::MetadataExt;
                (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
            };
            Ok(ReviewedFileStamp {
                path,
                length: meta.len(),
                modified: meta.modified().map_err(|_| "reviewed_input")?,
                #[cfg(unix)]
                identity,
            })
        })
        .collect()
}

impl ReviewedRoles {
    fn load(directory: PathBuf) -> Result<Self, &'static str> {
        let files = reviewed_files(&directory)?;
        let roles = Arc::new(load_roles(&directory)?);
        if files != reviewed_files(&directory)? {
            return Err("reviewed_changed");
        }
        Ok(Self {
            directory: Some(directory),
            current: std::sync::Mutex::new(ReviewedSnapshot { files, roles }),
        })
    }

    #[cfg(test)]
    fn fixed(roles: Vec<Published>) -> Self {
        Self {
            directory: None,
            current: std::sync::Mutex::new(ReviewedSnapshot {
                files: Vec::new(),
                roles: Arc::new(roles),
            }),
        }
    }

    fn snapshot(&self) -> Result<Arc<Vec<Published>>, &'static str> {
        let mut current = self.current.lock().map_err(|_| "reviewed_lock")?;
        let Some(directory) = self.directory.as_ref() else {
            return Ok(Arc::clone(&current.roles));
        };
        let files = reviewed_files(directory)?;
        // Unix file identity detects atomic replacement even with preserved size/mtime.
        if cfg!(unix) && files == current.files {
            return Ok(Arc::clone(&current.roles));
        }
        let roles = Arc::new(load_roles(directory)?);
        if files != reviewed_files(directory)? {
            return Err("reviewed_changed");
        }
        *current = ReviewedSnapshot { files, roles };
        Ok(Arc::clone(&current.roles))
    }
}

/// One data plane. The password is wiped when the plane is dropped.
pub struct Plane {
    deployment_mode: oo_core::config::DeploymentMode,
    query_timeout_ms: u64,
    query_capacity: Option<tokio::sync::Semaphore>,
    endpoint: TlsEndpoint,
    password: String,
    ca_pem: Vec<u8>,
    connector: tokio_postgres_rustls::MakeRustlsConnect,
    roles: ReviewedRoles,
    wal_dir: PathBuf,
    collector_dir: Option<PathBuf>,
    sessions: Mutex<HashMap<String, Arc<RoleSession>>>,
    opening: Mutex<()>,
    inflight: Mutex<HashMap<String, Flight>>,
}

/// One slot per published role, not a second pool. Slow connects and busy
/// sessions serialize only that role; the map lock never spans network I/O.
#[derive(Default)]
struct RoleSession {
    current: Mutex<Option<Arc<Mutex<DedicatedSession>>>>,
    admission: Arc<Mutex<()>>,
    planning: Mutex<Option<(Published, crate::sql_guard::PolicyCatalog)>>,
}

struct Flight {
    server_id: String,
    query_id: String,
    principal: String,
    project: String,
    role: String,
    state: Arc<QueryState>,
}

impl Flight {
    fn owned_by(&self, caller: &PublicCaller) -> bool {
        self.principal == caller.tuple.principal_id
            && self.project == caller.tuple.project_id
            && self.role == caller.tuple.role
    }
}

impl Drop for Plane {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

/// Load the data plane when every data key is present. A partial set fails closed.
pub(crate) fn load_plane(
    dir: &Path,
    object: &Map<String, Value>,
) -> Result<Option<Arc<Plane>>, &'static str> {
    let installed = Value::Object(object.clone());
    let deployment_mode = oo_core::config::DeploymentMode::from_installed(&installed)?;
    let (query_concurrency, query_timeout_ms) = deployment_mode.query_limits(&installed)?;
    let present = DATA_KEYS.iter().any(|key| object.contains_key(*key));
    let collector = object.contains_key("audit_collector_dir");
    if !present && !collector {
        return Ok(None);
    }
    if !present || DATA_KEYS.iter().any(|key| !object.contains_key(*key)) {
        return Err("data_plane");
    }
    let endpoint_file = one_segment(text_field(object, "data_endpoint_file")?)?;
    let password_file = one_segment(text_field(object, "data_password_file")?)?;
    let ca_file = one_segment(text_field(object, "data_ca_file")?)?;
    let user = text_field(object, "data_user")?;
    if !login_ok(user) {
        return Err("data_user");
    }
    let wal_name = one_segment(text_field(object, "wal_dir")?)?;
    let reviewed_name = one_segment(text_field(object, "reviewed_dir")?)?;
    let endpoint = read_endpoint(&dir.join(endpoint_file), user)?;
    let mut password = fs::read_to_string(dir.join(password_file)).map_err(|_| "data_password")?;
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    if password.is_empty() || password.contains('\0') {
        password.zeroize();
        return Err("data_password");
    }
    let ca_pem = fs::read(dir.join(ca_file)).map_err(|_| "data_ca")?;
    if ca_pem.is_empty() {
        password.zeroize();
        return Err("data_ca");
    }
    let connector = match rustls_connector(&ca_pem) {
        Ok(connector) => connector,
        Err(_) => {
            password.zeroize();
            return Err("data_ca");
        }
    };
    let wal_dir = dir.join(wal_name);
    if !wal_dir.is_dir() {
        password.zeroize();
        return Err("wal_dir");
    }
    let collector_dir = if collector {
        let name = one_segment(text_field(object, "audit_collector_dir")?)?;
        let path = dir.join(name);
        if !path.is_dir() {
            password.zeroize();
            return Err("audit_collector");
        }
        Some(path)
    } else {
        None
    };
    let roles = match ReviewedRoles::load(dir.join(reviewed_name)) {
        Ok(roles) => roles,
        Err(reason) => {
            password.zeroize();
            return Err(reason);
        }
    };
    Ok(Some(Arc::new(Plane {
        deployment_mode,
        query_timeout_ms,
        query_capacity: (deployment_mode == oo_core::config::DeploymentMode::AllInOne)
            .then(|| tokio::sync::Semaphore::new(query_concurrency)),
        endpoint,
        password,
        ca_pem,
        connector,
        roles,
        wal_dir,
        collector_dir,
        sessions: Mutex::new(HashMap::new()),
        opening: Mutex::new(()),
        inflight: Mutex::new(HashMap::new()),
    })))
}

fn load_roles(dir: &Path) -> Result<Vec<Published>, &'static str> {
    let entries = fs::read_dir(dir).map_err(|_| "reviewed_dir")?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| "reviewed_dir")?;
        let file_type = entry.file_type().map_err(|_| "reviewed_dir")?;
        if file_type.is_symlink() || !file_type.is_dir() {
            return Err("reviewed_dir");
        }
        let name = entry.file_name();
        let name = name.to_str().ok_or("reviewed_dir")?;
        one_segment(name)?;
        names.push(name.to_owned());
    }
    names.sort();
    if names.is_empty() {
        return Err("reviewed_dir");
    }
    let mut roles = Vec::new();
    for name in names {
        roles.push(load_role(&dir.join(name))?);
    }
    let mut seen = Vec::new();
    for role in &roles {
        let key = (
            role.project.as_str(),
            role.role.as_str(),
            role.policy_version.as_str(),
            role.knowledge_version.as_str(),
        );
        if seen.contains(&key) {
            return Err("reviewed_duplicate");
        }
        seen.push(key);
    }
    Ok(roles)
}

fn load_role(dir: &Path) -> Result<Published, &'static str> {
    if dir.join("library.json").is_file() {
        let bytes = fs::read(dir.join("library.json")).map_err(|_| "library_input")?;
        let text = std::str::from_utf8(&bytes).map_err(|_| "library_input")?;
        let document = oo_core::json_scan::parse_json_value_with_limits(
            text,
            "oo.gateway.library-reviewed/v1",
            &oo_core::json_scan::JsonScanLimits {
                max_bytes: 67_108_864,
                max_nodes: 4_000_000,
                max_depth: 32,
            },
        )
        .map_err(|_| "library_input")?;
        let library = crate::library_policy::compile_library(&document)?;
        return Ok(Published {
            mapping_source: Some(String::new()),
            policies: library.policies,
            role: library.role,
            project: library.project,
            policy_version: library.policy_version,
            knowledge_version: library.knowledge_version,
            config_digest: library.config_digest,
            quality_vector: Some(library.vector.clone()),
            native_vector: Some(library.vector),
            native_catalog: Some(library.catalog),
        });
    }
    if dir.join("native-input.json").is_file() {
        return load_native_reviewed_role(dir);
    }
    if dir.join("command.json").is_file() {
        return load_reviewed_role(dir);
    }
    // An operator policy plus its visibility file is the source engineer's
    // existing allowlist. It is not a reviewed-effective publication.
    load_operator_policy(dir)
}

fn load_reviewed_role(dir: &Path) -> Result<Published, &'static str> {
    let command = fs::read_to_string(dir.join("command.json")).map_err(|_| "reviewed_command")?;
    let artifact =
        fs::read_to_string(dir.join("artifact.json")).map_err(|_| "reviewed_artifact")?;
    let decisions =
        fs::read_to_string(dir.join("decisions.json")).map_err(|_| "reviewed_decisions")?;
    let binding_text =
        fs::read_to_string(dir.join("binding.json")).map_err(|_| "reviewed_binding")?;
    let binding = parse_binding(&binding_text)?;
    let reviewed = compile_reviewed_policies(&command, &artifact, &decisions, &binding)
        .map_err(|_| "reviewed_policy")?;
    if reviewed.policies.is_empty() {
        return Err("reviewed_policy");
    }
    let role = reviewed.compiled.role().to_string();
    let project = reviewed.policies[0].project.clone();
    let digest = reviewed.policies[0].config_digest.clone();
    if project != binding.identity.project || digest != binding.config_digest {
        return Err("reviewed_policy");
    }
    Ok(Published {
        mapping_source: None,
        native_vector: None,
        native_catalog: None,
        quality_vector: None,
        policies: reviewed.policies,
        role,
        project,
        policy_version: binding.identity.policy_version,
        knowledge_version: binding.identity.knowledge_version,
        config_digest: digest,
    })
}

const OPERATOR_POLICY: &str = "oo.operator-preview.policy/v1";
const ROLE_VISIBILITY: &str = "oo.builder.role-visibility/v1";

fn load_operator_policy(dir: &Path) -> Result<Published, &'static str> {
    let policy_text = fs::read_to_string(dir.join("policy.json")).map_err(|_| "reviewed_policy")?;
    let visibility_text =
        fs::read_to_string(dir.join("visibility.json")).map_err(|_| "reviewed_policy")?;
    let policy: Value = serde_json::from_str(&policy_text).map_err(|_| "reviewed_policy")?;
    let visibility: Value =
        serde_json::from_str(&visibility_text).map_err(|_| "reviewed_policy")?;
    let policy = policy.as_object().ok_or("reviewed_policy")?;
    let visibility = visibility.as_object().ok_or("reviewed_policy")?;
    exact_keys(
        policy,
        &[
            "config_digest",
            "generation",
            "login",
            "project",
            "protocol",
            "role",
            "table",
        ],
    )?;
    if policy.get("protocol").and_then(Value::as_str) != Some(OPERATOR_POLICY) {
        return Err("reviewed_policy");
    }
    if visibility.get("protocol").and_then(Value::as_str) != Some(ROLE_VISIBILITY) {
        return Err("reviewed_policy");
    }
    let project = field_str(policy, "project")?;
    let slug = field_str(policy, "role")?;
    let login = field_str(policy, "login")?;
    let digest = field_str(policy, "config_digest")?;
    let generation = policy
        .get("generation")
        .and_then(Value::as_i64)
        .ok_or("reviewed_policy")?;
    let table = policy
        .get("table")
        .and_then(Value::as_object)
        .ok_or("reviewed_policy")?;
    exact_keys(table, &["columns", "name", "namespace"])?;
    let namespace = field_str(table, "namespace")?;
    let name = field_str(table, "name")?;
    let column_values = table
        .get("columns")
        .and_then(Value::as_array)
        .ok_or("reviewed_policy")?;
    let mut columns = Vec::new();
    let mut names = Vec::new();
    for item in column_values {
        let item = item.as_object().ok_or("reviewed_policy")?;
        exact_keys(item, &["name", "pii", "semantic_type", "status"])?;
        let column_name = field_str(item, "name")?;
        let semantic_type = field_str(item, "semantic_type")?;
        let status = match field_str(item, "status")?.as_str() {
            "confirmed" => ClassStatus::Confirmed,
            "auto_accepted" => ClassStatus::AutoAccepted,
            _ => return Err("reviewed_policy"),
        };
        let pii_object = item
            .get("pii")
            .and_then(Value::as_object)
            .ok_or("reviewed_policy")?;
        exact_keys(pii_object, &["class"])?;
        let pii = match pii_object.get("class").and_then(Value::as_str) {
            Some("none") => PiiClass::None,
            _ => return Err("reviewed_policy"),
        };
        names.push(column_name.clone());
        columns.push(ColumnSpec {
            name: column_name,
            status,
            semantic_type,
            pii,
        });
    }
    let visible_role = field_str(visibility, "role")?;
    if field_str(visibility, "project")? != project || visible_role != format!("oo_r_{slug}") {
        return Err("reviewed_policy");
    }
    let knowledge = visibility
        .get("knowledge")
        .and_then(Value::as_object)
        .ok_or("reviewed_policy")?;
    let policy_meta = visibility
        .get("policy")
        .and_then(Value::as_object)
        .ok_or("reviewed_policy")?;
    let knowledge_version = field_str(knowledge, "version")?;
    let policy_version = field_str(policy_meta, "version")?;
    let datasets = visibility
        .get("datasets")
        .and_then(Value::as_array)
        .ok_or("reviewed_policy")?;
    if datasets.len() != 1 {
        return Err("reviewed_policy");
    }
    let dataset = datasets[0].as_object().ok_or("reviewed_policy")?;
    if field_str(dataset, "dataset")? != format!("{namespace}.{name}") {
        return Err("reviewed_policy");
    }
    let listed = dataset
        .get("columns")
        .and_then(Value::as_array)
        .ok_or("reviewed_policy")?;
    if listed.len() != names.len() {
        return Err("reviewed_policy");
    }
    for (item, expected) in listed.iter().zip(names.iter()) {
        if item.as_str() != Some(expected) {
            return Err("reviewed_policy");
        }
    }
    let acl = ColumnAclPolicy {
        project: project.clone(),
        config_digest: digest.clone(),
        generation,
        role: slug,
        login,
        table: LogicalTable {
            namespace,
            name,
            columns,
        },
        row_filter: None,
    };
    compile_column_acl(&acl).map_err(|_| "reviewed_policy")?;
    Ok(Published {
        mapping_source: None,
        native_vector: None,
        native_catalog: None,
        quality_vector: None,
        policies: vec![acl],
        role: visible_role,
        project,
        policy_version,
        knowledge_version,
        config_digest: digest,
    })
}

fn exact_keys(object: &Map<String, Value>, keys: &[&str]) -> Result<(), &'static str> {
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        Err("reviewed_policy")
    } else {
        Ok(())
    }
}

fn field_str(object: &Map<String, Value>, key: &str) -> Result<String, &'static str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or("reviewed_policy")
}

fn parse_binding(text: &str) -> Result<ReviewedBinding, &'static str> {
    let value: Value = serde_json::from_str(text).map_err(|_| "reviewed_binding")?;
    let object = value.as_object().ok_or("reviewed_binding")?;
    let allow_value = object
        .get("allow")
        .and_then(Value::as_array)
        .ok_or("reviewed_binding")?;
    let mut allow = Vec::new();
    for item in allow_value {
        let item = item.as_object().ok_or("reviewed_binding")?;
        allow.push(ReviewedAllow {
            dataset: required(item, "dataset")?,
            column: required(item, "column")?,
            explicit_allow: item
                .get("explicit_allow")
                .and_then(Value::as_bool)
                .ok_or("reviewed_binding")?,
        });
    }
    let generation = object
        .get("generation")
        .and_then(Value::as_i64)
        .ok_or("reviewed_binding")?;
    Ok(ReviewedBinding {
        identity: ReviewedIdentity {
            project: required(object, "project")?,
            ontology_version: required(object, "ontology_version")?,
            catalog_version: required(object, "catalog_version")?,
            policy_id: required(object, "policy_id")?,
            policy_version: required(object, "policy_version")?,
            knowledge_id: required(object, "knowledge_id")?,
            knowledge_version: required(object, "knowledge_version")?,
        },
        seal: ReviewedSourceSeal {
            source_id: if object.get("binding_vector").is_some() {
                object
                    .get("source_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            } else {
                required(object, "source_id")?
            },
            schema: if object.get("binding_vector").is_some() {
                object
                    .get("schema")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            } else {
                required(object, "schema")?
            },
            schema_hash: required(object, "schema_hash")?,
            artifact_sha256: required(object, "artifact_sha256")?,
        },
        config_digest: if object.get("binding_vector").is_some() {
            object
                .get("config_digest")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        } else {
            required(object, "config_digest")?
        },
        generation,
        role: required(object, "role")?,
        login: required(object, "login")?,
        binding_vector: object.get("binding_vector").cloned(),
        allow,
    })
}

impl Plane {
    async fn connect(
        &self,
    ) -> Result<(tokio_postgres::Client, crate::policy::TlsConnection), crate::policy::PolicyError>
    {
        if self.deployment_mode == oo_core::config::DeploymentMode::AllInOne {
            crate::policy::connect_readonly_require_tls(
                &self.endpoint,
                &self.password,
                &self.ca_pem,
            )
            .await
        } else {
            connect_require_tls(&self.endpoint, &self.password, &self.ca_pem).await
        }
    }

    fn role_for(&self, caller: &PublicCaller) -> Option<Published> {
        self.roles
            .snapshot()
            .ok()?
            .iter()
            .find(|role| {
                role.project == caller.tuple.project_id
                    && role.role == caller.tuple.role
                    && role.policy_version == caller.tuple.policy_version
                    && role.knowledge_version == caller.tuple.knowledge_version
            })
            .cloned()
    }

    async fn can_start(&self) -> bool {
        let mut guard = self.inflight.lock().await;
        if guard.len() >= 256 {
            guard.retain(|_, flight| flight.state.status() == "running");
        }
        guard
            .values()
            .filter(|flight| flight.state.status() == "running")
            .count()
            < 256
    }

    async fn catalog_for(
        &self,
        published: &Published,
    ) -> Result<crate::sql_guard::PolicyCatalog, CatalogFailure> {
        let mut compiled = Vec::with_capacity(published.policies.len());
        for policy in &published.policies {
            compiled.push(
                if published.native_vector.is_some() {
                    crate::policy::compile_column_acl_native(policy)
                } else {
                    compile_column_acl(policy)
                }
                .map_err(|_| ErrorCode::SchemaInvalid)?,
            );
        }
        let mut limits = CATALOG_LIMITS;
        if let Some(catalog) = published
            .native_catalog
            .as_ref()
            .filter(|c| c["protocol"] == "oo.gateway.library-binding/v1")
        {
            limits.max_tables = catalog["limits"]["max_tables"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(ErrorCode::SchemaInvalid)?;
            limits.max_columns = catalog["limits"]["max_columns"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(ErrorCode::SchemaInvalid)?;
        }
        let identity = PolicyIdentity {
            project: published.project.clone(),
            role: published.role.clone(),
            policy_id: published.config_digest.clone(),
        };
        let (mut client, connection) =
            self.connect().await.map_err(|_| ErrorCode::SchemaInvalid)?;
        let driver = crate::query_executor::PendingConnection::new(
            &client,
            connection,
            self.connector.clone(),
        );
        let loaded = if let Some(source) = &published.mapping_source {
            crate::policy::catalog::load_policy_catalog_for_source_vector(
                &mut client,
                &identity,
                &compiled,
                &limits,
                source,
                published.native_vector.as_ref(),
                published.native_catalog.as_ref(),
            )
            .await
        } else {
            load_policy_catalog(&mut client, &identity, &compiled, &CATALOG_LIMITS).await
        };
        drop(client);
        driver.release().abort();
        let catalog = loaded.map_err(CatalogFailure::from)?;
        let slot = self.role_slot(&published.role).await?;
        *slot.planning.lock().await = Some((published.clone(), catalog.clone()));
        Ok(catalog)
    }

    async fn planning_catalog_for(
        &self,
        published: &Published,
    ) -> Result<crate::sql_guard::PolicyCatalog, CatalogFailure> {
        let slot = self.role_slot(&published.role).await?;
        if let Some((revision, catalog)) = slot.planning.lock().await.as_ref()
            && revision == published
        {
            return Ok(catalog.clone());
        }
        self.catalog_for(published).await
    }

    async fn forget_planning_catalog(&self, published: &Published) {
        if let Ok(slot) = self.role_slot(&published.role).await {
            let mut cached = slot.planning.lock().await;
            if cached
                .as_ref()
                .is_some_and(|(revision, _)| revision == published)
            {
                *cached = None;
            }
        }
    }

    pub(crate) async fn warm_planning_catalogs(&self) {
        let Ok(roles) = self.roles.snapshot() else {
            return;
        };
        for published in roles.iter().filter(|p| {
            p.native_catalog
                .as_ref()
                .is_some_and(|c| c["protocol"] == "oo.gateway.library-binding/v1")
        }) {
            // Only a planning candidate: every bound query still verifies the
            // complete catalog, ACL and publication inside its own snapshot.
            if !matches!(
                tokio::time::timeout(
                    Duration::from_millis(self.query_timeout_ms),
                    self.planning_catalog_for(published),
                )
                .await,
                Ok(Ok(_))
            ) {
                eprintln!("gateway planning warmup unavailable; live validation retained");
            }
        }
    }

    async fn session_for(&self, role: &str) -> Result<Arc<Mutex<DedicatedSession>>, ErrorCode> {
        let slot = self.role_slot(role).await?;
        let mut current = slot.current.lock().await;
        if let Some(existing) = current.as_ref()
            && existing.lock().await.is_open()
        {
            return Ok(Arc::clone(existing));
        }
        let (client, connection) = self.connect().await.map_err(|_| ErrorCode::SchemaInvalid)?;
        let session = DedicatedSession::bind(client, connection, self.connector.clone())
            .await
            .map_err(|_| ErrorCode::SchemaInvalid)?;
        let shared = Arc::new(Mutex::new(session));
        *current = Some(Arc::clone(&shared));
        Ok(shared)
    }

    async fn role_slot(&self, role: &str) -> Result<Arc<RoleSession>, ErrorCode> {
        // Slot cardinality is bounded by the installed role set, never headers.
        if !self
            .roles
            .snapshot()
            .map_err(|_| ErrorCode::SchemaInvalid)?
            .iter()
            .any(|published| published.role == role)
        {
            return Err(ErrorCode::SchemaInvalid);
        }
        Ok({
            let _gate = self.opening.lock().await;
            let mut sessions = self.sessions.lock().await;
            Arc::clone(sessions.entry(role.to_owned()).or_default())
        })
    }

    fn binding_check(
        &self,
        published: &Published,
        caller: &PublicCaller,
        pack_digest: &str,
        catalog: Option<&crate::sql_guard::PolicyCatalog>,
        expected: Expected,
    ) -> Result<BindingCheck, Box<Response>> {
        let policies = published
            .policies
            .iter()
            .map(|policy| {
                if published.native_vector.is_some() {
                    crate::policy::compile_column_acl_native(policy)
                } else {
                    compile_column_acl(policy)
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Box::new(binding_drift(None)))?;
        let mut limits = CATALOG_LIMITS;
        if let Some(native) = published
            .native_catalog
            .as_ref()
            .filter(|c| c["protocol"] == "oo.gateway.library-binding/v1")
        {
            limits.max_tables = native["limits"]["max_tables"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| Box::new(binding_drift(None)))?;
            limits.max_columns = native["limits"]["max_columns"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| Box::new(binding_drift(None)))?;
        }
        let identity = PolicyIdentity {
            project: published.project.clone(),
            role: published.role.clone(),
            policy_id: published.config_digest.clone(),
        };
        if catalog.is_some_and(|catalog| catalog.identity != identity) {
            return Err(Box::new(binding_drift(None)));
        }
        Ok(BindingCheck {
            expected,
            identity,
            principal: caller.tuple.principal_id.clone(),
            policy_version: published.policy_version.clone(),
            knowledge_version: published.knowledge_version.clone(),
            pack_digest: pack_digest.into(),
            source: published.mapping_source.clone(),
            policies,
            limits,
            vector: published.native_vector.clone(),
            native_catalog: published.native_catalog.clone(),
            admitted_catalog: catalog.cloned(),
        })
    }

    async fn track(
        &self,
        query_id: &str,
        caller: &PublicCaller,
        state: Arc<QueryState>,
    ) -> Result<String, ()> {
        let server_id = new_server_id()?;
        let mut guard = self.inflight.lock().await;
        if guard.len() >= 256 {
            guard.retain(|_, flight| flight.state.status() == "running");
        }
        if guard.len() >= 256 {
            return Err(());
        }
        guard.insert(
            server_id.clone(),
            Flight {
                server_id: server_id.clone(),
                query_id: query_id.to_owned(),
                principal: caller.tuple.principal_id.clone(),
                project: caller.tuple.project_id.clone(),
                role: caller.tuple.role.clone(),
                state,
            },
        );
        Ok(server_id)
    }

    async fn finish(&self, query_id: &str, status: &'static str) {
        for flight in self.inflight.lock().await.values_mut() {
            if flight.query_id == query_id {
                flight.state.finish(status);
            }
        }
    }
}

struct WalAudit {
    dir: PathBuf,
    collector: Option<PathBuf>,
    query_id: String,
    principal: String,
}

impl AuditAdmission for WalAudit {
    async fn admit(&mut self, admission: &Admission<'_>) -> Result<(), ExecError> {
        drain_backlog(&self.dir, self.collector.as_deref());
        self.persist("admit", 1, admission_fields(admission)).await
    }

    async fn complete(&mut self, completion: &CompletionRecord) -> Result<(), ExecError> {
        let mut fields = Map::new();
        fields.insert(
            "status".to_owned(),
            Value::String(completion.status().as_str().to_owned()),
        );
        fields.insert(
            "returned_rows".to_owned(),
            Value::from(completion.returned_rows()),
        );
        fields.insert("truncated".to_owned(), Value::from(completion.truncated()));
        fields.insert(
            "rollback".to_owned(),
            Value::from(completion.rollback_confirmed()),
        );
        self.persist("complete", 2, fields).await
    }
}

impl WalAudit {
    async fn persist(
        &self,
        phase: &str,
        sequence: u32,
        mut fields: Map<String, Value>,
    ) -> Result<(), ExecError> {
        fields.insert("sequence".to_owned(), Value::from(sequence));
        let state = if self.collector.is_none() {
            "unconfigured"
        } else {
            "copied"
        };
        fields.insert("collector".to_owned(), Value::String(state.to_owned()));
        let mut line = audit_line(phase, &self.query_id, &self.principal, fields.clone());
        if let Some(dir) = self.collector.as_deref() {
            let name = format!("{}-{phase}.wal", self.query_id);
            if !copy_with_retry(dir, &name, &line).await {
                fields.insert("collector".to_owned(), Value::String("backlog".to_owned()));
                line = audit_line(phase, &self.query_id, &self.principal, fields);
                let _ = write_backlog(&self.dir, &self.query_id, phase, &line);
                alert_collector(&self.dir, &line);
            }
        }
        fsync_record(&self.dir, &self.query_id, phase, &line)
    }
}

fn admission_fields(admission: &Admission<'_>) -> Map<String, Value> {
    let mut fields = Map::new();
    fields.insert(
        "original_sha256".to_owned(),
        Value::String(admission.original_sha256().to_owned()),
    );
    fields.insert(
        "executed_sha256".to_owned(),
        Value::String(admission.executed_sha256().to_owned()),
    );
    fields.insert(
        "fingerprint".to_owned(),
        Value::String(admission.fingerprint_hex().to_owned()),
    );
    fields.insert(
        "role".to_owned(),
        Value::String(admission.role().to_owned()),
    );
    fields.insert(
        "project".to_owned(),
        Value::String(admission.project().to_owned()),
    );
    fields.insert(
        "policy_id".to_owned(),
        Value::String(admission.policy_id().to_owned()),
    );
    fields.insert(
        "backend_pid".to_owned(),
        Value::from(admission.backend_pid()),
    );
    fields
}

fn audit_line(
    phase: &str,
    query_id: &str,
    principal: &str,
    mut fields: Map<String, Value>,
) -> String {
    fields.insert("phase".to_owned(), Value::String(phase.to_owned()));
    fields.insert("query_id".to_owned(), Value::String(query_id.to_owned()));
    fields.insert("principal".to_owned(), Value::String(principal.to_owned()));
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    fields.insert("at_ms".to_owned(), Value::from(millis as u64));
    Value::Object(fields).to_string()
}

/// A second cancel of the same query must not clobber `{query_id}-cancel.wal`.
/// `create_new` failing on that name is not a WAL outage.
fn fresh_audit_phase(dir: &Path, query_id: &str, phase: &str) -> String {
    let primary = format!("{query_id}-{phase}.wal");
    if !dir.join(&primary).exists() {
        return phase.to_owned();
    }
    for n in 2..16 {
        let extra = format!("{phase}-{n}");
        if !dir.join(format!("{query_id}-{extra}.wal")).exists() {
            return extra;
        }
    }
    phase.to_owned()
}

fn fsync_record(dir: &Path, query_id: &str, phase: &str, line: &str) -> Result<(), ExecError> {
    let name = format!("{query_id}-{phase}.wal");
    if name.contains('/') || name.contains('\\') {
        return Err(ExecError::AuditUnavailable);
    }
    let path = dir.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| ExecError::AuditUnavailable)?;
    file.write_all(line.as_bytes())
        .map_err(|_| ExecError::AuditUnavailable)?;
    file.write_all(b"\n")
        .map_err(|_| ExecError::AuditUnavailable)?;
    file.sync_all().map_err(|_| ExecError::AuditUnavailable)?;
    File::open(dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| ExecError::AuditUnavailable)?;
    Ok(())
}

async fn copy_with_retry(dir: &Path, name: &str, line: &str) -> bool {
    for attempt in 0..3 {
        if copy_new(dir, name, line).is_ok() {
            return true;
        }
        if attempt + 1 < 3 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    false
}

fn copy_new(dir: &Path, name: &str, line: &str) -> Result<(), ()> {
    if name.contains('/') || name.contains('\\') {
        return Err(());
    }
    let path = dir.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| ())?;
    if file
        .write_all(line.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .is_err()
    {
        let _ = fs::remove_file(&path);
        return Err(());
    }
    File::open(dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| ())
}

fn write_backlog(dir: &Path, query_id: &str, phase: &str, line: &str) -> Result<(), ()> {
    copy_new(dir, &format!("{query_id}-{phase}.backlog"), line)
}

fn alert_collector(dir: &Path, line: &str) {
    eprintln!("audit_collector=failed");
    let path = dir.join("collector-alert.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
        let _ = file.write_all(b"\n");
        let _ = file.sync_all();
    }
}

fn drain_backlog(wal_dir: &Path, collector: Option<&Path>) {
    let Some(collector) = collector else {
        return;
    };
    let Ok(entries) = fs::read_dir(wal_dir) else {
        return;
    };
    let mut replayed = 0u32;
    for entry in entries {
        if replayed >= 16 {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("backlog") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        if copy_new(collector, &format!("{stem}.wal"), line).is_ok() {
            let _ = fs::remove_file(&path);
            eprintln!("audit_collector=replayed");
            replayed += 1;
        }
    }
}

pub(crate) fn mount(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/v1/tables", get(tables))
        .route("/v1/tables/{table}", get(describe))
        .route("/v1/sql/validate", post(validate))
        .route("/v1/query", post(query))
        .route("/v1/freshness", get(freshness))
        .route("/v1/meta", get(meta))
        .route("/v1/knowledge/search", post(knowledge_search))
        .route("/v1/knowledge/{*path}", get(knowledge))
        .route("/v1/queries/{query_id}", get(query_status))
        .route("/v1/queries/{query_id}/cancel", post(cancel))
        .route("/v1/operations", get(operations))
        .route("/v1/operations/{server_id}", get(operation_status))
        .route("/v1/operations/{server_id}/cancel", post(operation_cancel))
}

fn expected_binding(headers: &HeaderMap) -> Result<Option<Expected>, Box<Response>> {
    Expected::from_headers(headers).map_err(|_| {
        Box::new(error_response(
            ErrorCode::SchemaInvalid,
            "invalid publication binding headers",
            None,
            None,
        ))
    })
}

fn binding_drift(query_id: Option<&str>) -> Response {
    error_response(
        ErrorCode::Conflict,
        "publication binding is unavailable or has drifted; recompute the whole question",
        Some(DRIFT_HINT.into()),
        query_id,
    )
}

fn with_binding(mut response: Response, receipt: Option<&BindingReceipt>) -> Response {
    if response.status().is_success()
        && let Some(receipt) = receipt
    {
        receipt.headers(response.headers_mut());
    }
    response
}

struct BindingDriver(tokio::task::JoinHandle<()>);
impl Drop for BindingDriver {
    fn drop(&mut self) {
        self.0.abort();
    }
}

// Metadata is immutable Pack content; validate/describe reuse this checked
// catalog. Later SQL must verify the same identity again in its own snapshot.
async fn metadata_binding(
    state: &AppState,
    caller: &PublicCaller,
    headers: &HeaderMap,
    read_quality: bool,
) -> Result<
    Option<(
        BindingReceipt,
        crate::sql_guard::PolicyCatalog,
        Option<QualitySnapshot>,
    )>,
    Box<Response>,
> {
    agree(headers, caller)?;
    let expected = expected_binding(headers)?;
    let plane = state.data_plane();
    let published = plane.as_ref().and_then(|p| p.role_for(caller));
    let library = published.as_ref().is_some_and(|p| {
        p.native_catalog
            .as_ref()
            .is_some_and(|c| c["protocol"] == "oo.gateway.library-binding/v1")
    });
    let Some(expected) = expected.or_else(|| {
        library.then(|| Expected {
            publication_id: "current".into(),
            data_vector_sha256: None,
        })
    }) else {
        return Ok(None);
    };
    let plane = plane.ok_or_else(|| Box::new(binding_drift(None)))?;
    let published = published.ok_or_else(|| Box::new(binding_drift(None)))?;
    let pack =
        find_pack(&state.pack_catalog(), caller).ok_or_else(|| Box::new(binding_drift(None)))?;
    let checked = async {
        let binding = plane
            .binding_check(&published, caller, &pack.pack_digest, None, expected)
            .map_err(|_| ())?;
        let (mut client, connection) = plane.connect().await.map_err(|_| ())?;
        let _driver = BindingDriver(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let tx = client
            .build_transaction()
            .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()
            .await
            .map_err(|_| ())?;
        tx.batch_execute(&format!(
            "SET LOCAL statement_timeout = {}",
            plane.query_timeout_ms
        ))
        .await
        .map_err(|_| ())?;
        let (receipt, catalog) = binding.metadata_scope(&tx).await.map_err(|_| ())?;
        let quality = if read_quality {
            audit_freshness(&plane, &published).await.map_err(|_| ())?;
            let tables = crate::query_executor::role_quality_tables(
                &catalog,
                published.bound_quality_vector().as_ref(),
            );
            Some(crate::query_executor::observe_quality_rows(&tx, &tables).await?)
        } else {
            None
        };
        tx.rollback().await.map_err(|_| ())?;
        Ok::<_, ()>((receipt, catalog, quality))
    };
    match tokio::time::timeout(Duration::from_millis(plane.query_timeout_ms), checked).await {
        Ok(Ok(result)) => Ok(Some(result)),
        Ok(Err(())) => Err(Box::new(binding_drift(None))),
        Err(_) => Err(Box::new(error_response(
            ErrorCode::Timeout,
            "binding deadline exceeded",
            None,
            None,
        ))),
    }
}

async fn tables(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let binding = match metadata_binding(&state, &caller, &headers, false).await {
        Ok(binding) => binding,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    let receipt = binding.as_ref().map(|(receipt, _, _)| receipt.clone());
    let response = AUTH_STAMP
        .scope(stamp, async move {
            if let Err(response) = agree(&headers, &caller) {
                return *response;
            }
            let Some(pack) = find_pack(&state.pack_catalog(), &caller) else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "knowledge pack is not loaded",
                    None,
                    None,
                );
            };
            let mut names = Vec::new();
            for column in &pack.columns {
                if !names.contains(&column.table) {
                    names.push(column.table.clone());
                }
            }
            names.sort();
            let tables = names
                .into_iter()
                .map(|name| {
                    let count = pack
                        .columns
                        .iter()
                        .filter(|column| column.table == name)
                        .count();
                    serde_json::json!({"name": name, "columns": count})
                })
                .collect::<Vec<_>>();
            json_ok(serde_json::json!({
                "tables": tables,
                "knowledge_version": pack.knowledge_version,
                "policy_version": pack.policy_version,
                "pack_digest": pack.pack_digest,
            }))
        })
        .await;
    with_binding(response, receipt.as_ref())
}

async fn describe(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    AxumPath(table): AxumPath<String>,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let binding = match metadata_binding(&state, &caller, &headers, false).await {
        Ok(binding) => binding,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    let receipt = binding.as_ref().map(|(receipt, _, _)| receipt.clone());
    let response = AUTH_STAMP
        .scope(stamp, async move {
            if let Err(response) = agree(&headers, &caller) {
                return *response;
            }
            if !table_key_ok(&table) {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "table name is not public",
                    None,
                    None,
                );
            }
            let Some(pack) = find_pack(&state.pack_catalog(), &caller) else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "knowledge pack is not loaded",
                    None,
                    None,
                );
            };
            let mut columns = Vec::new();
            for column in &pack.columns {
                if column.table == table && !columns.contains(&column.name) {
                    columns.push(column.name.clone());
                }
            }
            if columns.is_empty() {
                return error_response(
                    ErrorCode::RelationNotFound,
                    "table is not in this pack",
                    None,
                    None,
                );
            }
            let (column_details, query_capabilities) = if let Some(plane) = state.data_plane() {
                let Some(published) = plane.role_for(&caller) else {
                    return error_response(
                        ErrorCode::SchemaInvalid,
                        "role is not published on this gateway",
                        None,
                        None,
                    );
                };
                let catalog = match if let Some((_, catalog, _)) = &binding {
                    Ok(catalog.clone())
                } else {
                    plane.catalog_for(&published).await
                } {
                    Ok(catalog) => catalog,
                    Err(error) => {
                        return error_response(
                            error.code,
                            "policy catalog is not available",
                            error.hint,
                            None,
                        );
                    }
                };
                (
                    describe_column_details(&table, &columns, &catalog.relations),
                    query_capabilities(Some(&catalog)),
                )
            } else {
                (Vec::new(), query_capabilities(None))
            };
            json_ok(serde_json::json!({
                "table": table,
                "columns": columns,
                "column_details": column_details,
                "query_capabilities": query_capabilities,
                "knowledge_version": pack.knowledge_version,
                "policy_version": pack.policy_version,
                "pack_digest": pack.pack_digest,
            }))
        })
        .await;
    with_binding(response, receipt.as_ref())
}

fn query_capabilities(catalog: Option<&crate::sql_guard::PolicyCatalog>) -> Value {
    let unavailable = |reason| {
        serde_json::json!({
            "version": "gateway.query-capabilities.v1",
            "status": "unavailable",
            "reason": reason,
            "requires_validate": true,
        })
    };
    let Some(catalog) = catalog else {
        return unavailable("runtime_catalog_unavailable");
    };
    // Project only the bounded builtin catalog. Never echo extension/private names.
    if catalog.functions.len() > CATALOG_LIMITS.max_functions as usize
        || catalog.operators.len() > CATALOG_LIMITS.max_operators as usize
        || catalog
            .functions
            .iter()
            .any(|f| crate::policy::catalog::builtin_function_result_type(f).is_none())
        || catalog.operators.iter().any(|op| {
            op.schema != "pg_catalog"
                || !(crate::policy::catalog::arithmetic_signature_allowed(op)
                    || crate::policy::catalog::pattern_signature_allowed(op)
                    || (crate::policy::catalog::COMPARISON_OPERATORS.contains(&op.name.as_str())
                        && op.result_type_oid == postgres_types::Type::BOOL.oid()))
                || postgres_types::Type::from_oid(op.left_type_oid).is_none()
                || postgres_types::Type::from_oid(op.right_type_oid).is_none()
                || postgres_types::Type::from_oid(op.result_type_oid).is_none()
        })
    {
        return unavailable("catalog_projection_unsupported");
    }
    let type_name = |oid: u32| {
        postgres_types::Type::from_oid(oid)
            .map(|kind| kind.name().to_owned())
            .unwrap_or_else(|| "unavailable".to_owned())
    };
    let functions = catalog.functions.iter().map(|function| serde_json::json!({
        "name": function.name,
        "schema": "pg_catalog",
        "argument_type_oids": function.argument_type_oids,
        "argument_types": function.argument_type_oids.iter().map(|oid| type_name(*oid)).collect::<Vec<_>>(),
        "aggregate_star": function.aggregate_star,
    })).collect::<Vec<_>>();
    let operators = catalog
        .operators
        .iter()
        .map(|operator| {
            serde_json::json!({
                "name": operator.name,
                "schema": "pg_catalog",
                "left_type_oid": operator.left_type_oid,
                "right_type_oid": operator.right_type_oid,
                "left_type": type_name(operator.left_type_oid),
                "right_type": type_name(operator.right_type_oid),
                "result_type_oid": operator.result_type_oid,
                "result_type": type_name(operator.result_type_oid),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "version": "gateway.query-capabilities.v1",
        "status": "available",
        "source": "runtime_policy_catalog",
        "requires_validate": true,
        "truncated": false,
        "functions": functions,
        "operators": operators,
        "literal_types": {
            "integer": catalog.literal_types.integer.map(type_name),
            "text": catalog.literal_types.text.map(type_name),
            "boolean": catalog.literal_types.boolean.map(type_name),
        },
        "null_test": catalog.null_test_allowed,
        "sql_shapes": {
            "group_by_output_ordinal": true,
            "order_by_output_ordinal": true,
            "having": true,
            "searched_numeric_case": true,
            "timestamp_to_date": catalog.functions.iter().any(|f| f.name == "date"),
            "date_trunc_day_timestamp": catalog.functions.iter().any(|f| f.name == "date_trunc"),
            "to_char_timestamp_literal_format": catalog.functions.iter().any(|f| f.name == "to_char"),
            "like_literal_pattern": catalog.operators.iter().any(crate::policy::catalog::pattern_signature_allowed),
            "timezone_dependent_day_conversion": false
        },
    })
}

fn describe_column_details(
    table: &str,
    pack_columns: &[String],
    relations: &[crate::sql_guard::VisibleRelation],
) -> Vec<Value> {
    // Intersect the role's live catalog with its pack; never expose physical names.
    relations
        .iter()
        .filter(|relation| format!("{}.{}", relation.schema, relation.name) == table)
        .flat_map(|relation| &relation.columns)
        .filter(|column| pack_columns.contains(&column.name))
        .map(|column| {
            serde_json::json!({
                "name": column.name,
                "type_oid": column.type_oid,
                "type": postgres_types::Type::from_oid(column.type_oid)
                    .map(|kind| kind.name().to_owned()),
            })
        })
        .collect()
}

async fn knowledge(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    AxumPath(path): AxumPath<String>,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let binding = match metadata_binding(&state, &caller, &headers, false).await {
        Ok(binding) => binding,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    let receipt = binding.as_ref().map(|(receipt, _, _)| receipt.clone());
    let response = AUTH_STAMP
        .scope(stamp, async move {
            if let Err(response) = agree(&headers, &caller) {
                return *response;
            }
            let Some(body) = state.pack_catalog().file_text(
                &caller.tuple.project_id,
                &caller.tuple.role,
                &caller.tuple.policy_version,
                &caller.tuple.knowledge_version,
                &path,
            ) else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "knowledge path is not in this pack",
                    None,
                    None,
                );
            };
            json_ok(serde_json::json!({
                "path": body.path,
                "sha256": body.sha256,
                "bytes": body.bytes,
                "knowledge_version": caller.tuple.knowledge_version,
                "policy_version": caller.tuple.policy_version,
                "text": body.text,
            }))
        })
        .await;
    with_binding(response, receipt.as_ref())
}

async fn knowledge_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    body: Bytes,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let binding = match metadata_binding(&state, &caller, &headers, false).await {
        Ok(binding) => binding,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    let receipt = binding.as_ref().map(|(receipt, _, _)| receipt.clone());
    let response = AUTH_STAMP.scope(stamp, async move {
    if let Err(response) = agree(&headers, &caller) {
        return *response;
    }
    let value: crate::pack_catalog::PackSearch = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                ErrorCode::SchemaInvalid,
                "search body is not json",
                None,
                None,
            );
        }
    };
    let page = match state.pack_catalog().search_page(
        &caller.tuple.project_id,
        &caller.tuple.role,
        &caller.tuple.policy_version,
        &caller.tuple.knowledge_version,
        &value,
    ) {
        Ok(page) => page,
        Err(_) => return error_response(ErrorCode::SchemaInvalid, "invalid search options", None, None),
    };
    let hits = page.hits
        .into_iter()
        .map(|hit| serde_json::json!({"path": hit.path, "sha256": hit.sha256, "excerpt": hit.excerpt}))
        .collect::<Vec<_>>();
    json_ok(serde_json::json!({
        "hits": hits,
        "total": page.total,
        "offset": value.offset,
        "limit": value.limit,
        "has_more": page.next_offset.is_some(),
        "next_offset": page.next_offset,
        "knowledge_version": caller.tuple.knowledge_version,
        "policy_version": caller.tuple.policy_version,
    }))
    }).await;
    with_binding(response, receipt.as_ref())
}

async fn validate(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    body: Bytes,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let binding = match metadata_binding(&state, &caller, &headers, false).await {
        Ok(binding) => binding,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    let receipt = binding.as_ref().map(|(receipt, _, _)| receipt.clone());
    let response = AUTH_STAMP
        .scope(stamp, async move {
            if let Err(response) = agree(&headers, &caller) {
                return *response;
            }
            let Some(plane) = state.data_plane() else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "data plane is not configured",
                    None,
                    None,
                );
            };
            let sql = match sql_from_body(&body) {
                Ok(sql) => sql,
                Err(response) => return *response,
            };
            let Some(published) = plane.role_for(&caller) else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "role is not published on this gateway",
                    None,
                    None,
                );
            };
            let catalog = match if let Some((_, catalog, _)) = &binding {
                Ok(catalog.clone())
            } else {
                plane.catalog_for(&published).await
            } {
                Ok(catalog) => catalog,
                Err(error) => {
                    return error_response(
                        error.code,
                        "policy catalog is not available",
                        error.hint,
                        None,
                    );
                }
            };
            let identity = PolicyIdentity {
                project: published.project.clone(),
                role: published.role.clone(),
                policy_id: published.config_digest.clone(),
            };
            match guard_select(&identity, &catalog, &sql) {
                Ok(plan) => json_ok(validate_body(&plan)),
                Err(err) => {
                    let (code, message) = guard_public(&err);
                    error_response(code, message, guard_hint(&err), None)
                }
            }
        })
        .await;
    with_binding(response, receipt.as_ref())
}

async fn meta(
    State(state): State<AppState>,
    mut headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
) -> Response {
    // Anonymous negotiation reveals no role-scoped publication or internal hosts.
    let mut document = serde_json::json!({
        "gateway_version": env!("CARGO_PKG_VERSION"),
        "protocol_version": "0.2",
        "minimum_cli_version": "0.2.0",
        "publication_id": null,
        "freshness": null
    });
    if query.is_some() {
        return error_response(
            ErrorCode::SchemaInvalid,
            "query string is not allowed",
            None,
            None,
        );
    }
    if headers.contains_key(axum::http::header::AUTHORIZATION) {
        headers
            .entry(crate::publication_binding::EXPECTED)
            .or_insert(axum::http::HeaderValue::from_static("current"));
        let response = freshness(State(state), headers, RawQuery(query), peer).await;
        if !response.status().is_success() {
            return response;
        }
        let (parts, body) = response.into_parts();
        if let Some(id) = parts
            .headers
            .get(crate::publication_binding::ACTUAL)
            .and_then(|v| v.to_str().ok())
        {
            document["publication_id"] = serde_json::json!(id);
        }
        let bytes = match axum::body::to_bytes(body, 4 * 1024 * 1024).await {
            Ok(bytes) => bytes,
            Err(_) => {
                return error_response(ErrorCode::Internal, "metadata unavailable", None, None);
            }
        };
        document["freshness"] = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => {
                return error_response(ErrorCode::Internal, "metadata unavailable", None, None);
            }
        };
        let mut response = json_ok(document);
        response.headers_mut().extend(parts.headers);
        response
            .headers_mut()
            .remove(axum::http::header::CONTENT_LENGTH);
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
        return response;
    }
    json_ok(document)
}

async fn freshness(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let binding = match metadata_binding(&state, &caller, &headers, true).await {
        Ok(binding) => binding,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    let receipt = binding.as_ref().map(|(receipt, _, _)| receipt.clone());
    let response = AUTH_STAMP
        .scope(stamp, async move {
            if let Err(response) = agree(&headers, &caller) {
                return *response;
            }
            let Some(plane) = state.data_plane() else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "data plane is not configured",
                    None,
                    None,
                );
            };
            let Some(published) = plane.role_for(&caller) else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "role is not published on this gateway",
                    None,
                    None,
                );
            };
            if find_pack(&state.pack_catalog(), &caller).is_none() {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "knowledge pack is not loaded",
                    None,
                    None,
                );
            }
            let snapshot = match if let Some((_, _, Some(snapshot))) = &binding {
                Ok(snapshot.clone())
            } else {
                read_freshness_only(&plane, &published).await
            } {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return error_response(
                        error.code,
                        "freshness is not available",
                        error.hint,
                        None,
                    );
                }
            };
            if snapshot.relations == "unreadable" {
                return error_response(
                    ErrorCode::Internal,
                    "freshness is not readable",
                    None,
                    None,
                );
            }
            json_ok(freshness_document(
                &snapshot,
                &caller.tuple.knowledge_version,
                &caller.tuple.policy_version,
            ))
        })
        .await;
    with_binding(response, receipt.as_ref())
}

async fn query_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    AxumPath(query_id): AxumPath<String>,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    AUTH_STAMP
        .scope(stamp, async move {
            if let Err(response) = agree(&headers, &caller) {
                return *response;
            }
            let Some(plane) = state.data_plane() else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "data plane is not configured",
                    None,
                    None,
                );
            };
            if QueryId::new(query_id.clone()).is_err() {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "query id is not public",
                    None,
                    None,
                );
            }
            let guard = plane.inflight.lock().await;
            let Some(flight) = guard.values().find(|flight| flight.query_id == query_id) else {
                return error_response(ErrorCode::SchemaInvalid, "query was not found", None, None);
            };
            if !flight.owned_by(&caller) {
                return error_response(ErrorCode::SchemaInvalid, "query was not found", None, None);
            }
            json_ok(serde_json::json!({"query_id": query_id, "status": flight.state.status()}))
        })
        .await
}

async fn cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    AxumPath(query_id): AxumPath<String>,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    AUTH_STAMP
        .scope(stamp, async move {
            if let Err(response) = agree(&headers, &caller) {
                return *response;
            }
            let Some(plane) = state.data_plane() else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "data plane is not configured",
                    None,
                    None,
                );
            };
            if QueryId::new(query_id.clone()).is_err() {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "query id is not public",
                    None,
                    None,
                );
            }
            let (handle, audit_id) = {
                let guard = plane.inflight.lock().await;
                let Some(flight) = guard.values().find(|flight| flight.query_id == query_id) else {
                    return error_response(
                        ErrorCode::SchemaInvalid,
                        "query was not found",
                        None,
                        None,
                    );
                };
                if !flight.owned_by(&caller) {
                    return error_response(
                        ErrorCode::SchemaInvalid,
                        "query was not found",
                        None,
                        None,
                    );
                }
                (Arc::clone(&flight.state), flight.query_id.clone())
            };
            cancel_flight(&plane, &caller, &handle, &audit_id, "query_id", &query_id).await
        })
        .await
}

async fn operations(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    AUTH_STAMP
        .scope(stamp, async move {
            if let Err(response) = agree(&headers, &caller) {
                return *response;
            }
            let Some(plane) = state.data_plane() else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "data plane is not configured",
                    None,
                    None,
                );
            };
            let guard = plane.inflight.lock().await;
            let mut rows = Vec::new();
            for flight in guard.values() {
                if flight.owned_by(&caller) && flight.state.status() == "running" {
                    rows.push(serde_json::json!({
                        "server_id": flight.server_id,
                        "query_id": flight.query_id,
                        "status": flight.state.status(),
                    }));
                }
            }
            rows.sort_by(|left, right| {
                left["server_id"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(right["server_id"].as_str().unwrap_or(""))
            });
            json_ok(serde_json::json!({"operations": rows}))
        })
        .await
}

async fn operation_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    AxumPath(server_id): AxumPath<String>,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    AUTH_STAMP
        .scope(stamp, async move {
            if let Err(response) = agree(&headers, &caller) {
                return *response;
            }
            let Some(plane) = state.data_plane() else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "data plane is not configured",
                    None,
                    None,
                );
            };
            if !server_id_ok(&server_id) {
                return error_response(ErrorCode::SchemaInvalid, "query was not found", None, None);
            }
            let guard = plane.inflight.lock().await;
            let Some(flight) = guard.get(&server_id) else {
                return error_response(ErrorCode::SchemaInvalid, "query was not found", None, None);
            };
            if !flight.owned_by(&caller) {
                return error_response(ErrorCode::SchemaInvalid, "query was not found", None, None);
            }
            json_ok(serde_json::json!({
                "server_id": flight.server_id,
                "status": flight.state.status(),
            }))
        })
        .await
}

async fn operation_cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    AxumPath(server_id): AxumPath<String>,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    AUTH_STAMP
        .scope(stamp, async move {
            if let Err(response) = agree(&headers, &caller) {
                return *response;
            }
            let Some(plane) = state.data_plane() else {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "data plane is not configured",
                    None,
                    None,
                );
            };
            if !server_id_ok(&server_id) {
                return error_response(ErrorCode::SchemaInvalid, "query was not found", None, None);
            }
            let (handle, query_id) = {
                let guard = plane.inflight.lock().await;
                let Some(flight) = guard.get(&server_id) else {
                    return error_response(
                        ErrorCode::SchemaInvalid,
                        "query was not found",
                        None,
                        None,
                    );
                };
                if !flight.owned_by(&caller) {
                    return error_response(
                        ErrorCode::SchemaInvalid,
                        "query was not found",
                        None,
                        None,
                    );
                }
                (Arc::clone(&flight.state), flight.query_id.clone())
            };
            cancel_flight(&plane, &caller, &handle, &query_id, "server_id", &server_id).await
        })
        .await
}

async fn cancel_flight(
    plane: &Plane,
    caller: &PublicCaller,
    handle: &QueryState,
    query_id: &str,
    id_field: &str,
    id_value: &str,
) -> Response {
    let outcome = handle.cancel();
    let audit = record_cancel_audit(plane, query_id, &caller.tuple.principal_id, outcome).await;
    let mut body = Map::new();
    body.insert(id_field.to_owned(), Value::String(id_value.to_owned()));
    body.insert("outcome".to_owned(), Value::String(outcome.to_owned()));
    body.insert("audit".to_owned(), Value::String(audit.to_owned()));
    json_ok(Value::Object(body))
}

async fn record_cancel_audit(
    plane: &Plane,
    query_id: &str,
    principal: &str,
    outcome: &str,
) -> &'static str {
    let phase = fresh_audit_phase(&plane.wal_dir, query_id, "cancel");
    let mut fields = Map::new();
    fields.insert("sequence".to_owned(), Value::from(3u32));
    fields.insert("outcome".to_owned(), Value::String(outcome.to_owned()));
    fields.insert("collector".to_owned(), Value::String("copied".to_owned()));
    let mut line = audit_line(&phase, query_id, principal, fields.clone());
    if let Some(dir) = plane.collector_dir.as_deref() {
        let name = format!("{query_id}-{phase}.wal");
        if !copy_with_retry(dir, &name, &line).await {
            fields.insert("collector".to_owned(), Value::String("backlog".to_owned()));
            line = audit_line(&phase, query_id, principal, fields);
            let _ = write_backlog(&plane.wal_dir, query_id, &phase, &line);
            alert_collector(&plane.wal_dir, &line);
        }
    } else {
        fields.insert(
            "collector".to_owned(),
            Value::String("unconfigured".to_owned()),
        );
        line = audit_line(&phase, query_id, principal, fields);
    }
    if fsync_record(&plane.wal_dir, query_id, &phase, &line).is_err() {
        "not_durable"
    } else {
        "durable"
    }
}

async fn query(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    body: Bytes,
) -> Response {
    let request_started = Instant::now();
    let caller = match crate::http_gateway::public_caller(&state, &headers, &raw_query, peer).await
    {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let stamp = AuthStamp {
        remaining_ms: caller.auth_remaining_ms,
        hit: caller.auth_cache_hit,
    };
    AUTH_STAMP
        .scope(stamp, async move {
            if let Err(response) = agree(&headers, &caller) {
                return *response;
            }
            let expected = match expected_binding(&headers) {
                Ok(expected) => expected,
                Err(response) => return *response,
            };
            let Some(plane) = state.data_plane() else {
                if expected.is_some() { return binding_drift(None); }
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "data plane is not configured",
                    None,
                    None,
                );
            };
            let Some(pack) = find_pack(&state.pack_catalog(), &caller) else {
                if expected.is_some() { return binding_drift(None); }
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "knowledge pack is not loaded",
                    None,
                    None,
                );
            };
            let request = match parse_query_body(&body) {
                Ok(request) => request,
                Err(response) => return *response,
            };
            if request.format != QueryFormat::Json {
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "only json format is served",
                    None,
                    None,
                );
            }
            if matches!(
                request.client_context.as_ref().map(|ctx| ctx.purpose),
                Some(oo_core::api::ClientPurpose::Export)
            ) {
                return error_response(
                    ErrorCode::VersionUnsupported,
                    "export is not implemented",
                    None,
                    None,
                );
            }
            let Some(published) = plane.role_for(&caller) else {
                if expected.is_some() { return binding_drift(None); }
                return error_response(
                    ErrorCode::SchemaInvalid,
                    "role is not published on this gateway",
                    None,
                    None,
                );
            };
            let query_id = match new_query_id() {
                Ok(query_id) => query_id,
                Err(()) => {
                    return error_response(
                        ErrorCode::Internal,
                        "query id was not issued",
                        None,
                        None,
                    );
                }
            };
            let status = QueryState::new();
            let _lifetime = RequestLifetime(Arc::clone(&status));
            if plane.track(query_id.as_str(), &caller, Arc::clone(&status)).await.is_err() {
                return error_response(ErrorCode::Overloaded, "query capacity is full", None, Some(query_id.as_str()));
            }
            let budget = Duration::from_millis(u64::from(request.timeout_ms.unwrap_or(plane.query_timeout_ms as u32)))
                .min(Duration::from_millis(plane.query_timeout_ms))
                .min(MAX_QUERY_WALL)
                .min(state.query_request_budget());
            let work = async {
            let slot = match plane.role_slot(&published.role).await {
                Ok(slot) => slot,
                Err(code) => return error_response(code, "data session is not available", None, Some(query_id.as_str())),
            };
            // Do not let same-role waiters consume every global execution slot.
            let _role_admission = match Arc::clone(&slot.admission).try_lock_owned() {
                Ok(permit) => permit,
                Err(_) => return error_response(ErrorCode::Overloaded, "role query is busy; retry after the active query finishes", None, Some(query_id.as_str())),
            };
            let _capacity = match plane.query_capacity.as_ref().map(|capacity| capacity.try_acquire()) {
                Some(Ok(permit)) => Some(permit),
                Some(Err(_)) => return error_response(ErrorCode::Overloaded, "query capacity is full", None, Some(query_id.as_str())),
                None => None,
            };
            if !plane.can_start().await {
                return error_response(
                    ErrorCode::Overloaded,
                    "query capacity is full",
                    None,
                    Some(query_id.as_str()),
                );
            }
            let expected = expected.or_else(|| published.native_catalog.as_ref()
                .is_some_and(|c| c["protocol"] == "oo.gateway.library-binding/v1")
                .then(|| Expected { publication_id: "current".into(), data_vector_sha256: None }));
            let loaded = if expected.is_some() {
                plane.planning_catalog_for(&published).await
            } else {
                plane.catalog_for(&published).await
            };
            let catalog = match loaded {
                Ok(catalog) => catalog,
                Err(error) => {
                    if expected.is_some() { return binding_drift(Some(query_id.as_str())); }
                    return error_response(
                        error.code,
                        "policy catalog is not available",
                        error.hint,
                        Some(query_id.as_str()),
                    );
                }
            };
            let binding = match expected.map(|expected|
                plane.binding_check(&published, &caller, &pack.pack_digest, Some(&catalog), expected)).transpose() {
                Ok(binding) => binding,
                Err(response) => return *response,
            };
            let identity = PolicyIdentity {
                project: published.project.clone(),
                role: published.role.clone(),
                policy_id: published.config_digest.clone(),
            };
            let max_rows = request
                .max_rows
                .unwrap_or(MAX_QUERY_ROWS)
                .min(MAX_QUERY_ROWS);
            let started = request_started;
            let session = match plane.session_for(&published.role).await {
                Ok(session) => session,
                Err(code) => {
                    return error_response(
                        code,
                        "data session is not available",
                        None,
                        Some(query_id.as_str()),
                    );
                }
            };
            let mut lease = SessionLease(Some(session.lock_owned().await));
            let guard = lease.0.as_mut().expect("session lease");
            if binding.is_some() && !crate::http_gateway::recheck_bound_caller(&state, &headers, &caller).await {
                return error_response(ErrorCode::Unauthenticated, "authentication failed", None, Some(query_id.as_str()));
            }
            let audit = WalAudit {
                dir: plane.wal_dir.clone(),
                collector: plane.collector_dir.clone(),
                query_id: query_id.as_str().to_owned(),
                principal: caller.tuple.principal_id.clone(),
            };
            let quality_vector = published.bound_quality_vector();
            let remaining_ms = budget.saturating_sub(started.elapsed()).as_millis() as u64;
            if remaining_ms == 0 {
                status.finish("timed_out");
                return error_response(ErrorCode::Timeout, "query deadline exceeded", None, Some(query_id.as_str()));
            }
            let mut stream = match guard
                .execute_bound_select(
                    SelectRequest {
                        identity: &identity,
                        catalog: &catalog,
                        original_sql: request.sql.as_str(),
                        role: &published.role,
                        max_rows,
                        max_result_bytes: MAX_QUERY_BYTES,
                        wall_clock: Duration::from_millis(remaining_ms),
                        read_quality: true,
                        quality_vector: quality_vector.as_ref(),
                    },
                    audit,
                    binding,
                )
                .await
            {
                Ok(stream) => stream,
                Err(err) => {
                    if matches!(err, ExecError::Timeout { .. }) { status.finish("timed_out"); }
                    let (code, message) = exec_public(&err);
                    return error_response(code, message, exec_hint(&err), Some(query_id.as_str()));
                }
            };
            let read = read_stream(&mut stream).await;
            let columns = stream.result_columns();
            let quality = stream.quality();
            let publication = stream.publication();
            let summary = stream.summary();
            drop(stream);
            if let Err(err) = guard.wait_idle().await {
                plane.finish(query_id.as_str(), "failed").await;
                let (code, message) = exec_public(&err);
                return error_response(code, message, exec_hint(&err), Some(query_id.as_str()));
            }
            drop(lease.0.take());
            match read {
                Err(err) => {
                    if matches!(err, ExecError::Timeout { .. }) { status.finish("timed_out"); }
                    plane.finish(query_id.as_str(), "failed").await;
                    let (code, message) = exec_public(&err);
                    error_response(code, message, exec_hint(&err), Some(query_id.as_str()))
                }
                Ok(rows) => {
                    let Some(summary) = summary else {
                        plane.finish(query_id.as_str(), "failed").await;
                        return error_response(
                            ErrorCode::Internal,
                            "query summary is missing",
                            None,
                            Some(query_id.as_str()),
                        );
                    };
                    if !summary.end_audit_ok()
                        || summary.status() != crate::query_executor::CompletionStatus::Succeeded
                    {
                        plane.finish(query_id.as_str(), "failed").await;
                        if !summary.end_audit_ok() {
                            return error_response(
                                ErrorCode::AuditUnavailable,
                                "audit completion was not durable",
                                None,
                                Some(query_id.as_str()),
                            );
                        }
                        return error_response(
                            ErrorCode::Internal,
                            "query did not finish",
                            None,
                            Some(query_id.as_str()),
                        );
                    }
                    match build_response(Built {
                        query_id: &query_id,
                        columns: &columns,
                        rows: &rows,
                        summary: &summary,
                        quality: &quality,
                        pack: &pack,
                        elapsed: started.elapsed(),
                        max_rows,
                    }) {
                        Ok(response) => {
                            plane.finish(query_id.as_str(), "succeeded").await;
                            with_binding(response, publication.as_ref())
                        }
                        Err(response) => {
                            plane.finish(query_id.as_str(), "failed").await;
                            *response
                        }
                    }
                }
            }
            };
            let remaining = budget.saturating_sub(request_started.elapsed());
            let response = tokio::select! {
                biased;
                _ = status.canceled() => {
                    status.finish("canceled");
                    error_response(ErrorCode::Timeout, "query canceled", Some("Cancellation requested; no partial result is returned. Use oo status with this query_id.".into()), Some(query_id.as_str()))
                }
                result = tokio::time::timeout(remaining, work) => match result {
                    Ok(response) => response,
                    Err(_) => {
                        status.finish("timed_out");
                        error_response(ErrorCode::Timeout, "query deadline exceeded (admission, catalog, session and SQL share one budget)", Some("No partial result is returned; cleanup is bounded. Use oo status with this query_id; a new query does not require a Gateway restart.".into()), Some(query_id.as_str()))
                    }
                },
            };
            if response.status() == StatusCode::CONFLICT {
                plane.forget_planning_catalog(&published).await;
            }
            status.finish("failed");
            response
        })
        .await
}

async fn read_stream(stream: &mut SelectStream) -> Result<Vec<Vec<Cell>>, ExecError> {
    let mut rows = Vec::new();
    loop {
        match stream.next_row().await {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => return Ok(rows),
            Err(err) => return Err(err),
        }
    }
}

async fn audit_freshness(plane: &Plane, published: &Published) -> Result<(), CatalogFailure> {
    let audit_id = new_query_id().map_err(|_| ErrorCode::Internal)?;
    let line = serde_json::json!({
        "phase": "freshness",
        "sequence": 1,
        "query_id": audit_id.as_str(),
        "role": published.role,
        "project": published.project,
        "collector": if plane.collector_dir.is_some() { "copied" } else { "unconfigured" },
    })
    .to_string();
    fsync_record(&plane.wal_dir, audit_id.as_str(), "freshness", &line)
        .map_err(|_| ErrorCode::AuditUnavailable)?;
    if let Some(dir) = plane.collector_dir.as_deref() {
        let name = format!("{}-freshness.wal", audit_id.as_str());
        if !copy_with_retry(dir, &name, &line).await {
            let _ = write_backlog(&plane.wal_dir, audit_id.as_str(), "freshness", &line);
            alert_collector(&plane.wal_dir, &line);
        }
    }
    Ok(())
}

async fn read_freshness_only(
    plane: &Plane,
    published: &Published,
) -> Result<QualitySnapshot, CatalogFailure> {
    let catalog = plane.catalog_for(published).await?;
    audit_freshness(plane, published).await?;
    let tables = crate::query_executor::role_quality_tables(
        &catalog,
        published.bound_quality_vector().as_ref(),
    );
    let (mut client, connection) = plane
        .connect()
        .await
        .map_err(|_| ErrorCode::SchemaInvalid)?;
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    let snapshot = observe_quality(&mut client, &tables).await;
    drop(client);
    driver.abort();
    Ok(snapshot)
}

struct Built<'a> {
    query_id: &'a QueryId,
    columns: &'a [ResultColumn],
    rows: &'a [Vec<Cell>],
    summary: &'a crate::query_executor::QuerySummary,
    quality: &'a QualitySnapshot,
    pack: &'a crate::pack_catalog::PackMetadata,
    elapsed: Duration,
    max_rows: u32,
}

fn build_response(parts: Built<'_>) -> Result<Response, Box<Response>> {
    let Built {
        query_id,
        columns,
        rows,
        summary,
        quality,
        pack,
        elapsed,
        max_rows,
    } = parts;
    let mut out_columns = Vec::new();
    for column in columns {
        out_columns.push(public_column(column).map_err(|_| {
            Box::new(error_response(
                ErrorCode::SchemaInvalid,
                "result column is not public",
                None,
                Some(query_id.as_str()),
            ))
        })?);
    }
    let mut out_rows = Vec::new();
    for row in rows {
        let mut cells = Vec::new();
        for cell in row {
            cells.push(public_cell(cell));
        }
        out_rows.push(cells);
    }
    let projected = project_quality(quality);
    let mut reasons = Vec::new();
    if summary.truncated() {
        if summary.returned_rows() >= max_rows {
            reasons.push(TruncationReason::Rows);
        } else {
            reasons.push(TruncationReason::Bytes);
        }
    }
    let knowledge_version = Label::new(pack.knowledge_version.clone()).map_err(|_| {
        Box::new(error_response(
            ErrorCode::SchemaInvalid,
            "knowledge version is not a public label",
            None,
            Some(query_id.as_str()),
        ))
    })?;
    let policy_version = Label::new(pack.policy_version.clone()).map_err(|_| {
        Box::new(error_response(
            ErrorCode::SchemaInvalid,
            "policy version is not a public label",
            None,
            Some(query_id.as_str()),
        ))
    })?;
    let response = QueryResponse {
        query_id: query_id.clone(),
        columns: out_columns,
        rows: out_rows,
        returned_rows: u64::from(summary.returned_rows()),
        truncated: summary.truncated(),
        truncation_reasons: reasons,
        duration_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        freshness: projected.freshness,
        sources: projected.sources,
        warnings: projected.warnings,
        star_expanded: None,
        knowledge_version,
        policy_version,
        hints: if source_time_status(quality) == "unmeasured"
            && !mysql_target_observations(quality).is_empty()
        {
            vec![error_text("source_time=unmeasured; source commit time and lag are unavailable; target_applied_at is a target observation, not source freshness.").expect("static hint")]
        } else {
            Vec::new()
        },
        estimated_total_rows: None,
    };
    let mut value = serde_json::to_value(&response).map_err(|_| {
        Box::new(error_response(
            ErrorCode::Internal,
            "response was not encoded",
            None,
            Some(query_id.as_str()),
        ))
    })?;
    annotate_c5plus(&mut value, quality);
    omit_nulls(&mut value);
    let bytes = serde_json::to_vec(&value).map_err(|_| {
        Box::new(error_response(
            ErrorCode::Internal,
            "response was not encoded",
            None,
            Some(query_id.as_str()),
        ))
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        Box::new(error_response(
            ErrorCode::Internal,
            "response was not encoded",
            None,
            Some(query_id.as_str()),
        ))
    })?;
    parse_api_json::<QueryResponse>(text, |_| Vec::new()).map_err(|_| {
        Box::new(error_response(
            ErrorCode::Internal,
            "response was rejected by the schema door",
            None,
            Some(query_id.as_str()),
        ))
    })?;
    Ok(json_bytes(200, bytes))
}

struct Projected {
    freshness: Vec<Freshness>,
    sources: Vec<oo_core::api::SourceWatermark>,
    warnings: Vec<Warning>,
}

fn project_quality(snapshot: &QualitySnapshot) -> Projected {
    let mut freshness = Vec::new();
    for observed in &snapshot.freshness {
        if let Some(row) = freshness_row(observed) {
            freshness.push(row);
        }
    }
    let mut sources = Vec::new();
    for mark in &snapshot.watermarks {
        if let Some(row) = source_row(mark) {
            sources.push(row);
        }
    }
    let mut warnings = Vec::new();
    for warning in &snapshot.warnings {
        if let Some(row) = warning_row(warning) {
            warnings.push(row);
        }
    }
    for item in &snapshot.table_quality {
        if item.capture_method == "c5_diff"
            && item.delete_detection == "reconcile"
            && item.c5_mode.as_deref() == Some("layered")
        {
            warnings.push(Warning {
                code: WarningCode::DeletesLagged,
                table: QualifiedTable::new(format!("{}.{}", item.schema, item.table)).ok(),
                column: None,
                impact: None,
                rows: None,
                message: error_text(
                    "Deletes are detected by periodic reconciliation, not a source change stream.",
                )
                .expect("static warning"),
                c5_mode: item.c5_mode.clone(),
                last_full_digest_scan_completed_at: item.last_full_digest_scan_completed_at.clone(),
                recent_window_watermark: item.recent_window_watermark.clone(),
            });
        }
    }
    if !snapshot.libraries.is_empty()
        && (snapshot
            .mappings
            .iter()
            .map(|m| m.source.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 1
            || snapshot.capture_method == "c5_diff"
            || snapshot
                .watermarks
                .iter()
                .any(|w| !w.cross_table_consistent))
        || snapshot.capture_method == "c5_diff"
            && snapshot
                .mappings
                .iter()
                .any(|m| m.source_engine.as_deref() == Some("mysql"))
            && snapshot.watermarks.iter().any(|w| {
                w.consistency == "per_table"
                    && w.codec == "snapshot-zero/v1"
                    && !w.cross_table_consistent
            })
    {
        warnings.push(Warning {
            c5_mode: None, last_full_digest_scan_completed_at: None, recent_window_watermark: None,
            code: WarningCode::CrossTableSkew,
            table: None, column: None, impact: None, rows: None,
            message: error_text("Per-table snapshots do not establish cross-table consistency or a common source position.").expect("static warning"),
        });
    }
    if snapshot.relations != "skipped"
        && let Some(row) = gap_warning(snapshot)
    {
        warnings.push(row);
    }
    Projected {
        freshness,
        sources,
        warnings,
    }
}

fn annotate_c5plus(value: &mut Value, snapshot: &QualitySnapshot) {
    for item in &snapshot.table_quality {
        if item.capture_method != "c5_diff"
            || item.delete_detection != "reconcile"
            || item.c5_mode.as_deref() != Some("layered")
        {
            continue;
        }
        let name = format!("{}.{}", item.schema, item.table);
        for key in ["table_quality", "target_observations", "warnings"] {
            if let Some(rows) = value.get_mut(key).and_then(Value::as_array_mut) {
                for row in rows {
                    if row["table"] == name || row["dataset"] == name {
                        row["c5_mode"] = serde_json::json!(item.c5_mode);
                        row["last_full_digest_scan_completed_at"] =
                            serde_json::json!(item.last_full_digest_scan_completed_at);
                        row["recent_window_watermark"] =
                            serde_json::json!(item.recent_window_watermark);
                    }
                }
            }
        }
    }
}

fn freshness_document(snapshot: &QualitySnapshot, knowledge: &str, policy: &str) -> Value {
    let projected = project_quality(snapshot);
    let mut value = serde_json::json!({
        "freshness": projected.freshness,
        "source_time": source_time_status(snapshot),
        "source_ids": snapshot.mappings.iter().map(|m|m.source.as_str()).collect::<std::collections::BTreeSet<_>>(),
        "library_publications": snapshot.libraries.iter().map(|l|serde_json::json!({"source_id":l.source,"library_id":l.library_id,"receipt_sha256":l.receipt_sha256,"ready":l.ready})).collect::<Vec<_>>(),
        "target_observations": mysql_target_observations(snapshot),
        "sources": projected.sources,
        "source_watermarks": snapshot.watermarks.iter().map(|mark| serde_json::json!({
            "source": mark.source,
            "consistency": mark.consistency,
            "consistency_grade": mark.consistency_grade,
            "cross_table_consistent": mark.cross_table_consistent,
            "applied_pos": mark.applied_pos,
            "state": mark.state,
            "position_codec": mark.codec,
            "position_lineage": mark.lineage,
            "source_epoch": mark.source_epoch,
        })).collect::<Vec<_>>(),
        "global_position": global_position_status(snapshot),
        "table_quality": snapshot.table_quality.iter().map(|item| serde_json::json!({
            "dataset": format!("{}.{}", item.schema, item.table),
            "capture_method": measured(&item.capture_method),
            "delete_detection": measured(&item.delete_detection),
        })).collect::<Vec<_>>(),
        "warnings": projected.warnings,
        "mappings": snapshot.mappings.iter().map(mapping_value).collect::<Vec<_>>(),
        "gap": snapshot.probes.iter().map(|probe| serde_json::json!({
            "relation": probe.name,
            "state": probe.state,
        })).collect::<Vec<_>>(),
        "receipts": snapshot.receipts.iter().map(receipt_value).collect::<Vec<_>>(),
        "runs": snapshot.runs.iter().map(run_value).collect::<Vec<_>>(),
        "imports": snapshot.imports.iter().map(import_value).collect::<Vec<_>>(),
        "quality_counts": snapshot.quality_counts.iter().map(quality_count_value).collect::<Vec<_>>(),
        "publication": publication_label(snapshot),
        "replica_lag": "unmeasured",
        "capture_method": measured(&snapshot.capture_method),
        "deletion_guarantee": measured(&snapshot.delete_detection),
        "knowledge_version": knowledge,
        "policy_version": policy,
    });
    annotate_c5plus(&mut value, snapshot);
    value
}

fn source_time_status(snapshot: &QualitySnapshot) -> &'static str {
    if snapshot
        .mappings
        .iter()
        .any(|mapping| mapping.source_engine.as_deref() == Some("doris"))
    {
        return "unmeasured";
    }
    let live: Vec<_> = snapshot
        .freshness
        .iter()
        .filter(|row| row.state == "live")
        .collect();
    if live.is_empty() {
        return "unmeasured";
    }
    if live
        .iter()
        .all(|row| Rfc3339Timestamp::new(&row.source_committed_at).is_ok())
    {
        "measured"
    } else if live.iter().any(|row| row.source_committed_at.is_empty()) {
        "unmeasured"
    } else {
        "unknown"
    }
}

fn global_position_status(snapshot: &QualitySnapshot) -> &'static str {
    if !snapshot.libraries.is_empty()
        && (snapshot.watermarks.len() > 1
            || snapshot
                .watermarks
                .iter()
                .any(|w| w.consistency == "per_table" || w.codec == "snapshot-zero/v1"))
    {
        return "unmeasured";
    }
    if snapshot.watermarks.is_empty()
        || snapshot.watermarks.iter().any(|mark| {
            mark.applied_pos.is_none()
                || mark.consistency == "per_table"
                    && mark.codec == "snapshot-zero/v1"
                    && snapshot.capture_method == "c5_diff"
                    && snapshot.mappings.iter().any(|mapping| {
                        mapping.source == mark.source
                            && mapping.source_engine.as_deref() == Some("mysql")
                    })
        })
    {
        "unmeasured"
    } else {
        "measured"
    }
}

fn measured(value: &str) -> &str {
    if value.is_empty() {
        "unmeasured"
    } else {
        value
    }
}

fn publication_label(snapshot: &QualitySnapshot) -> &'static str {
    if snapshot.libraries.iter().all(|l| l.ready)
        && snapshot.freshness.iter().any(|row| row.state == "live")
    {
        "live"
    } else {
        "not_ready"
    }
}

fn mapping_value(item: &MappingObserved) -> Value {
    serde_json::json!({
        "source": item.source,
        "schema": item.schema,
        "table": item.table,
        "run_generation": item.run_generation,
        "bound": item.bound,
    })
}

fn receipt_value(item: &ReceiptObserved) -> Value {
    serde_json::json!({
        "source": item.source,
        "schema": item.schema,
        "table": item.table,
        "run_generation": item.run_generation,
        "applied_pos": item.applied_pos,
        "codec": item.codec,
        "cross_table_consistent": item.cross_table_consistent,
        "partial_txn": item.partial_txn,
        "target_commit_at": item.target_commit_at,
    })
}

fn run_value(item: &ImportRunObserved) -> Value {
    serde_json::json!({
        "source": item.source,
        "source_generation": item.source_generation,
        "target_generation": item.target_generation,
        "state": item.state,
        "sealed": item.sealed,
    })
}

fn import_value(item: &ImportTableObserved) -> Value {
    serde_json::json!({
        "schema": item.schema,
        "table": item.table,
        "run_generation": item.run_generation,
        "applied_rows": item.applied_rows,
        "applied_pos": item.applied_pos,
        "exhausted": item.exhausted,
    })
}

fn quality_count_value(item: &QualityCount) -> Value {
    serde_json::json!({
        "kind": item.kind,
        "rejected_rows": item.rejected_rows,
        "accepted_rows": item.accepted_rows,
        "batches": item.batches,
    })
}

fn mysql_unknown_source_time(
    m: &MappingObserved,
    w: &WatermarkObserved,
    f: &crate::query_executor::FreshnessObserved,
) -> bool {
    m.source_engine.as_deref() == Some("mysql")
        && w.codec == "mysql-gtid/v1"
        && w.consistency_grade == "T"
        && w.source_epoch.is_some_and(|epoch| epoch > 0)
        && w.lineage.starts_with("mysql.")
        && f.source_committed_at.is_empty()
        && Rfc3339Timestamp::new(&f.applied_at).is_ok()
}

fn mysql_target_observations(snapshot: &QualitySnapshot) -> Vec<Value> {
    if !snapshot.libraries.is_empty() {
        if snapshot.libraries.iter().any(|l| !l.ready) {
            return Vec::new();
        }
        return snapshot.mappings.iter().filter_map(|mapping| {
            let row=snapshot.freshness.iter().find(|f| f.schema==mapping.schema && f.table==mapping.table)?;
            if Rfc3339Timestamp::new(&row.applied_at).is_err() {return None;}
            Some(serde_json::json!({"table":format!("{}.{}",row.schema,row.table),"source":mapping.source,"source_generation":mapping.run_generation,"state":row.state,"source_time":if row.source_committed_at.is_empty(){"unmeasured"}else{"observed"},"source_committed_at":if row.source_committed_at.is_empty(){Value::Null}else{Value::String(row.source_committed_at.clone())},"target_applied_at":row.applied_at}))
        }).collect();
    }
    let c5 = snapshot.capture_method == "c5_diff"
        && snapshot.delete_detection == "reconcile"
        && ready_quality(snapshot);
    if !c5 && (snapshot.capture_method != "c1_cdc" || snapshot.delete_detection != "native") {
        return Vec::new();
    }
    snapshot
        .mappings
        .iter()
        .filter_map(|mapping| {
            let mark = snapshot
                .watermarks
                .iter()
                .find(|w| w.source == mapping.source)?;
            let row = snapshot.freshness.iter().find(|f| {
                f.schema == mapping.schema
                    && f.table == mapping.table
                    && (mysql_unknown_source_time(mapping, mark, f)
                        || c5
                            && mapping.source_engine.as_deref() == Some("mysql")
                            && f.source_committed_at.is_empty())
            })?;
            Some(
                serde_json::json!({"table":format!("{}.{}",row.schema,row.table),
            "source":mapping.source,"source_generation":mapping.run_generation,
            "state":row.state,"source_time":"unmeasured","source_committed_at":Value::Null,
            "target_applied_at":row.applied_at}),
            )
        })
        .collect()
}

fn mysql_c5_receipt<'a>(
    s: &'a QualitySnapshot,
    m: &MappingObserved,
) -> Option<&'a ReceiptObserved> {
    if m.source_engine.as_deref() != Some("mysql") || !m.bound || m.run_generation < 1 {
        return None;
    }
    let mut receipts = s.receipts.iter().filter(|r| {
        r.source == m.source
            && r.schema == m.schema
            && r.table == m.table
            && r.run_generation == m.run_generation
    });
    let receipt = receipts.next()?;
    if receipts.next().is_some()
        || receipt.codec != "snapshot-zero/v1"
        || (receipt.lineage != "full-import.snapshot"
            && receipt.lineage != format!("{}.{}", m.source, m.table))
        || receipt.source_epoch != 0
        || receipt.applied_pos != "0"
        || receipt.cross_table_consistent
        || receipt.partial_txn
        || Rfc3339Timestamp::new(&receipt.target_commit_at).is_err()
    {
        return None;
    }
    let mut qualities = s
        .table_quality
        .iter()
        .filter(|q| q.schema == m.schema && q.table == m.table);
    let quality = qualities.next()?;
    if qualities.next().is_some()
        || quality.capture_method != "c5_diff"
        || quality.delete_detection != "reconcile"
    {
        return None;
    }
    let complete_run = s.runs.iter().any(|r| {
        r.source == m.source
            && r.source_generation == m.run_generation
            && r.target_generation > 0
            && r.state == "published"
            && r.sealed
    });
    let complete_import = s.imports.iter().any(|i| {
        i.schema == m.schema
            && i.table == m.table
            && i.run_generation == m.run_generation
            && i.exhausted
            && i.applied_pos == "0"
    });
    let live = s.freshness.iter().any(|f| {
        f.schema == m.schema
            && f.table == m.table
            && f.state == "live"
            && Rfc3339Timestamp::new(&f.applied_at).is_ok()
    });
    (complete_run && complete_import && live).then_some(receipt)
}

fn mysql_c5_quality(s: &QualitySnapshot, m: &MappingObserved, w: &WatermarkObserved) -> bool {
    if s.capture_method != "c5_diff"
        || s.delete_detection != "reconcile"
        || w.consistency != "per_table"
        || !matches!(w.consistency_grade.as_str(), "B" | "S0")
        || w.codec != "snapshot-zero/v1"
        || w.applied_pos.as_deref() != Some("0")
        || w.source_epoch.is_some()
        || w.cross_table_consistent
    {
        return false;
    }
    let Some(receipt) = mysql_c5_receipt(s, m) else {
        return false;
    };
    let Some(anchor) = s
        .mappings
        .iter()
        .filter(|a| a.source == m.source)
        .max_by_key(|a| a.run_generation)
    else {
        return false;
    };
    let Some(anchor_receipt) = mysql_c5_receipt(s, anchor) else {
        return false;
    };
    w.lineage == format!("{}.{}", anchor.source, anchor.table)
        && stamp_millis(&anchor_receipt.target_commit_at)
            .zip(stamp_millis(&receipt.target_commit_at))
            .is_some_and(|(latest, current)| latest >= current)
        && s.runs
            .iter()
            .filter(|r| r.source == m.source && r.source_generation == m.run_generation)
            .all(|r| {
                s.runs.iter().any(|a| {
                    a.source == anchor.source
                        && a.source_generation == anchor.run_generation
                        && a.target_generation >= r.target_generation
                })
            })
}

// Readiness is proved for every authorized physical mapping, never a global max generation.
pub(crate) fn ready_quality(s: &QualitySnapshot) -> bool {
    let doris_only = !s.mappings.is_empty()
        && s.mappings
            .iter()
            .all(|m| m.source_engine.as_deref() == Some("doris"));
    s.libraries.iter().all(|l| l.ready)
        && !s.mappings.is_empty()
        && s.relations == "read"
        && s.probes.iter().all(|p| p.state == "read")
        && s.warnings.is_empty()
        && !s.capture_method.is_empty()
        && !s.delete_detection.is_empty()
        && (doris_only || (s.capture_method != "mixed" && s.delete_detection != "mixed"))
        && s.runs.len() >= s.mappings.len()
        && s.runs.iter().all(|r| r.state == "published" && r.sealed)
        && s.mappings.iter().all(|m| {
            m.bound
                && s.runs.iter().any(|r| {
                    r.source == m.source
                        && r.source_generation == m.run_generation
                        && r.state == "published"
                        && r.sealed
                })
                && s.imports.iter().any(|i| {
                    i.schema == m.schema
                        && i.table == m.table
                        && i.run_generation == m.run_generation
                        && i.exhausted
                })
                && s.watermarks.iter().any(|w| {
                    if w.source != m.source || w.state != "live" {
                        return false;
                    }
                    if !s.libraries.is_empty() {
                        return s.libraries.iter().any(|l| l.source == m.source && l.ready)
                            && s.freshness.iter().any(|f| {
                                f.schema == m.schema
                                    && f.table == m.table
                                    && f.state == "live"
                                    && Rfc3339Timestamp::new(&f.applied_at).is_ok()
                            })
                            && s.receipts.iter().any(|r| {
                                r.source == m.source
                                    && r.schema == m.schema
                                    && r.table == m.table
                                    && r.run_generation == m.run_generation
                                    && !r.partial_txn
                                    && Rfc3339Timestamp::new(&r.target_commit_at).is_ok()
                            });
                    }
                    match w.consistency.as_str() {
                        "transactional" => {
                            w.cross_table_consistent
                                && s.capture_method != "mixed"
                                && s.delete_detection != "mixed"
                                && s.freshness.iter().any(|f| {
                                    f.schema == m.schema
                                        && f.table == m.table
                                        && f.state == "live"
                                        && (freshness_row(f).is_some()
                                            || s.capture_method == "c1_cdc"
                                                && s.delete_detection == "native"
                                                && mysql_unknown_source_time(m, w, f))
                                })
                                && s.receipts.iter().any(|r| {
                                    r.source == m.source
                                        && r.schema == m.schema
                                        && r.table == m.table
                                        && r.run_generation == m.run_generation
                                        && r.codec == w.codec
                                        && r.lineage == w.lineage
                                        && Some(r.source_epoch) == w.source_epoch
                                        && Some(r.applied_pos.as_str()) == w.applied_pos.as_deref()
                                        && r.cross_table_consistent
                                        && !r.partial_txn
                                })
                        }
                        "per_table" => {
                            let Some(engine) = m.source_engine.as_deref() else {
                                return false;
                            };
                            if !crate::query_executor::supported_source_engine(engine) {
                                return false;
                            }
                            if engine == "doris" {
                                let matching_quality: Vec<_> = s
                                    .table_quality
                                    .iter()
                                    .filter(|q| q.schema == m.schema && q.table == m.table)
                                    .collect();
                                let Some(quality) = matching_quality.first() else {
                                    return false;
                                };
                                if matching_quality.len() != 1 {
                                    return false;
                                }
                                let expected_delete = match quality.capture_method.as_str() {
                                    "c4_partition" => "partition_replace",
                                    "c5_diff" => "reconcile",
                                    _ => return false,
                                };
                                w.codec == "partition-signature/v1"
                                    && w.applied_pos.is_none()
                                    && w.source_epoch.is_none()
                                    && w.consistency_grade == "S0"
                                    && w.lineage == "partition-map"
                                    && !w.cross_table_consistent
                                    && quality.delete_detection == expected_delete
                                    && s.freshness.iter().any(|f| {
                                        f.schema == m.schema
                                            && f.table == m.table
                                            && f.state == "live"
                                            && Rfc3339Timestamp::new(&f.applied_at).is_ok()
                                    })
                            } else if engine == "mysql" && w.source_epoch.is_none() {
                                mysql_c5_quality(s, m, w)
                            } else {
                                let expected_lineage = format!("configured.{engine}");
                                w.codec == "snapshot-zero/v1"
                                    && w.applied_pos.as_deref() == Some("0")
                                    && w.source_epoch == Some(0)
                                    && !w.cross_table_consistent
                                    && w.lineage == expected_lineage
                                    && s.capture_method != "mixed"
                                    && s.delete_detection != "mixed"
                                    && s.freshness.iter().any(|f| {
                                        f.schema == m.schema
                                            && f.table == m.table
                                            && f.state == "live"
                                            && Rfc3339Timestamp::new(&f.applied_at).is_ok()
                                    })
                                    && s.receipts.iter().any(|r| {
                                        r.source == m.source
                                            && r.schema == m.schema
                                            && r.table == m.table
                                            && r.run_generation == m.run_generation
                                            && r.codec == "snapshot-zero/v1"
                                            && r.lineage == "full-import.snapshot"
                                            && r.source_epoch == 0
                                            && r.applied_pos == "0"
                                            && !r.cross_table_consistent
                                            && !r.partial_txn
                                            && Rfc3339Timestamp::new(&r.target_commit_at).is_ok()
                                    })
                            }
                        }
                        _ => false,
                    }
                })
        })
}

fn gap_warning(snapshot: &QualitySnapshot) -> Option<Warning> {
    if ready_quality(snapshot) {
        return None;
    }
    let message = gap_message(snapshot);
    if message.is_empty() {
        return None;
    }
    Some(Warning {
        c5_mode: None,
        last_full_digest_scan_completed_at: None,
        recent_window_watermark: None,
        code: WarningCode::StaleData,
        table: None,
        column: None,
        impact: None,
        message: error_text(&message).ok()?,
        rows: None,
    })
}

fn gap_message(snapshot: &QualitySnapshot) -> String {
    let mut parts = Vec::new();
    if snapshot.mappings.is_empty() {
        parts.push("mappings none".to_owned());
    }
    for item in &snapshot.mappings {
        parts.push(format!(
            "source {} {}.{} generation {} bound {}",
            item.source, item.schema, item.table, item.run_generation, item.bound
        ));
    }
    for probe in &snapshot.probes {
        parts.push(format!("{} {}", probe.name, probe.state));
    }
    for item in snapshot.receipts.iter().take(8) {
        parts.push(format!(
            "receipt {} {}.{} generation {} pos {} codec {} cross_table {} partial {} at {}",
            item.source,
            item.schema,
            item.table,
            item.run_generation,
            item.applied_pos,
            item.codec,
            item.cross_table_consistent,
            item.partial_txn,
            item.target_commit_at
        ));
    }
    for item in snapshot.runs.iter().take(8) {
        parts.push(format!(
            "run {} source_generation {} target_generation {} state {} sealed {}",
            item.source, item.source_generation, item.target_generation, item.state, item.sealed
        ));
    }
    parts.push(format!(
        "capture_method {}",
        measured(&snapshot.capture_method)
    ));
    parts.push(format!(
        "deletion_guarantee {}",
        measured(&snapshot.delete_detection)
    ));
    parts.push(format!("publication {}", publication_label(snapshot)));
    parts.push("replica_lag unmeasured".to_owned());
    let mut text = parts.join("; ");
    if text.len() > 1800 {
        text.truncate(1800);
    }
    text
}

fn source_row(mark: &WatermarkObserved) -> Option<oo_core::api::SourceWatermark> {
    let source = SourceName::new(mark.source.clone()).ok()?;
    let consistency = match mark.consistency.as_str() {
        "transactional" => ConsistencyMode::Transactional,
        "per_table" => ConsistencyMode::PerTable,
        _ => return None,
    };
    let applied_pos =
        Position::new(ExactDecimal::parse_sql(mark.applied_pos.as_deref()?).ok()?).ok()?;
    Some(oo_core::api::SourceWatermark {
        source,
        consistency,
        cross_table_consistent: mark.cross_table_consistent,
        applied_pos,
    })
}

fn freshness_row(observed: &crate::query_executor::FreshnessObserved) -> Option<Freshness> {
    let as_of = Rfc3339Timestamp::new(&observed.source_committed_at).ok()?;
    let applied = Rfc3339Timestamp::new(&observed.applied_at).ok()?;
    let lag_ms = lag_between(as_of.as_str(), applied.as_str())?;
    let state = match observed.state.as_str() {
        "live" => FreshnessState::Live,
        "paused" => FreshnessState::Paused,
        "snapshotting" | "catching_up" | "awaiting_publish" | "error" => FreshnessState::Stale,
        _ => return None,
    };
    let table = QualifiedTable::new(format!("{}.{}", observed.schema, observed.table)).ok()?;
    Some(Freshness {
        table,
        as_of,
        lag_ms,
        state,
    })
}

fn lag_between(source: &str, applied: &str) -> Option<u64> {
    let source_ms = stamp_millis(source)?;
    let applied_ms = stamp_millis(applied)?;
    applied_ms.checked_sub(source_ms)
}

fn stamp_millis(value: &str) -> Option<u64> {
    let (date, rest) = value.split_once('T')?;
    let rest = rest.strip_suffix('Z')?;
    let (hms, frac) = rest.split_once('.').unwrap_or((rest, ""));
    if frac.len() > 6 || !frac.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut dates = date.split('-');
    let year: i64 = dates.next()?.parse().ok()?;
    let month: u32 = dates.next()?.parse().ok()?;
    let day: u32 = dates.next()?.parse().ok()?;
    let mut clock = hms.split(':');
    let hour: u32 = clock.next()?.parse().ok()?;
    let minute: u32 = clock.next()?.parse().ok()?;
    let second: u32 = clock.next()?.parse().ok()?;
    let days = days_from_civil(year, month, day)?;
    let sod = u64::from(hour) * 3600 + u64::from(minute) * 60 + u64::from(second);
    let micros: u64 = if frac.is_empty() {
        0
    } else {
        frac.parse::<u64>()
            .ok()?
            .checked_mul(10u64.pow(6 - frac.len() as u32))?
    };
    days.checked_mul(86_400)?
        .checked_add(sod)?
        .checked_mul(1000)?
        .checked_add(micros / 1000)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<u64> {
    if !(1970..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u64;
    let mp = if month > 2 {
        month as u64 - 3
    } else {
        month as u64 + 9
    };
    let doy = (153 * mp + 2) / 5 + day as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe as i64 - 719_468;
    u64::try_from(days).ok()
}

fn warning_row(warning: &QualityWarning) -> Option<Warning> {
    let code = match warning.code {
        "RECONCILE_MISMATCH" => WarningCode::ReconcileMismatch,
        "RECONCILE_STALE" => WarningCode::ReconcileStale,
        "UNEXPLAINED_CODES" => WarningCode::UnexplainedCodes,
        "PARTIAL_TRANSACTION" => WarningCode::PartialTransaction,
        "ENTITY_CHECK_FAILED" => WarningCode::EntityCheckFailed,
        "STALE_DATA" => WarningCode::StaleData,
        _ => return None,
    };
    Some(Warning {
        c5_mode: None,
        last_full_digest_scan_completed_at: None,
        recent_window_watermark: None,
        code,
        table: QualifiedTable::new(format!("{}.{}", warning.schema, warning.table)).ok(),
        column: None,
        impact: None,
        message: error_text(warning.message.as_str()).ok()?,
        rows: None,
    })
}

fn public_column(column: &ResultColumn) -> Result<QueryColumn, ()> {
    let name = TableName::new(column.name.clone()).map_err(|_| ())?;
    let type_name = Label::new(type_label(column.oid)).map_err(|_| ())?;
    let (precision, scale) = if column.oid == 1700 {
        numeric_parts(column.typmod)
    } else {
        (None, None)
    };
    QueryColumn::new(name, type_name, precision, scale, None).map_err(|_| ())
}

fn type_label(oid: u32) -> &'static str {
    match oid {
        16 => "bool",
        17 => "bytea",
        20 => "int8",
        21 => "int2",
        23 => "int4",
        25 | 1042 | 1043 => "text",
        114 => "json",
        701 => "float8",
        1082 => "date",
        1114 => "timestamp",
        1184 => "timestamptz",
        1700 => "numeric",
        2950 => "uuid",
        3802 => "jsonb",
        _ => "text",
    }
}

fn numeric_parts(typmod: i32) -> (Option<u32>, Option<i32>) {
    if typmod < 0 {
        return (None, None);
    }
    let adjusted = typmod.wrapping_sub(4);
    let precision = (adjusted >> 16) & 0xFFFF;
    let raw = adjusted & 0x7FF;
    let scale = (raw ^ 0x400) - 0x400;
    (u32::try_from(precision).ok(), Some(scale))
}

fn public_cell(cell: &Cell) -> QueryCell {
    match cell {
        Cell::Null => QueryCell::Null,
        Cell::Bool(value) => QueryCell::Bool(*value),
        Cell::Int(text) | Cell::Text(text) => QueryCell::Text(text.clone()),
        Cell::Decimal(value) => QueryCell::Text(value.to_string()),
    }
}

fn validate_body(plan: &crate::sql_guard::GuardedPlan) -> Value {
    let mut columns = Vec::new();
    for output in &plan.bindings.outputs {
        if let OutputItem::Column(column) = output {
            columns.push(serde_json::json!({
                "name": column.output_name,
                "type_oid": column.type_oid,
            }));
        }
    }
    let stars = plan
        .star_expansions
        .iter()
        .map(|star| format!("{}.{}", star.schema, star.name))
        .collect::<Vec<_>>();
    serde_json::json!({
        "valid": true,
        "fingerprint": plan.fingerprint_hex,
        "columns": columns,
        "star_expanded": stars,
    })
}

fn parse_query_body(body: &[u8]) -> Result<QueryRequest, Box<Response>> {
    let text = std::str::from_utf8(body).map_err(|_| {
        Box::new(error_response(
            ErrorCode::SchemaInvalid,
            "query body is not utf-8",
            None,
            None,
        ))
    })?;
    parse_api_json::<QueryRequest>(text, |_| Vec::new()).map_err(|_| {
        Box::new(error_response(
            ErrorCode::SchemaInvalid,
            "query body was rejected",
            None,
            None,
        ))
    })
}

fn sql_from_body(body: &[u8]) -> Result<String, Box<Response>> {
    let value: Value = serde_json::from_slice(body).map_err(|_| {
        Box::new(error_response(
            ErrorCode::SchemaInvalid,
            "validate body is not json",
            None,
            None,
        ))
    })?;
    let sql = value.get("sql").and_then(Value::as_str).ok_or_else(|| {
        Box::new(error_response(
            ErrorCode::SchemaInvalid,
            "sql is missing",
            None,
            None,
        ))
    })?;
    if sql.is_empty() || sql.len() > 65_536 {
        return Err(Box::new(error_response(
            ErrorCode::SchemaInvalid,
            "sql is not public",
            None,
            None,
        )));
    }
    Ok(sql.to_owned())
}

fn find_pack(
    catalog: &PackCatalog,
    caller: &PublicCaller,
) -> Option<crate::pack_catalog::PackMetadata> {
    catalog.find(
        &caller.tuple.project_id,
        &caller.tuple.role,
        &caller.tuple.policy_version,
        &caller.tuple.knowledge_version,
    )
}

fn agree(headers: &HeaderMap, caller: &PublicCaller) -> Result<(), Box<Response>> {
    let pairs = [
        ("x-oo-project", caller.tuple.project_id.as_str()),
        ("x-oo-role", caller.tuple.role.as_str()),
        ("x-oo-policy-version", caller.tuple.policy_version.as_str()),
        (
            "x-oo-knowledge-version",
            caller.tuple.knowledge_version.as_str(),
        ),
        ("x-oo-scope", caller.scope),
    ];
    for (name, expected) in pairs {
        if let Some(value) = headers.get(name) {
            let Ok(text) = value.to_str() else {
                return Err(Box::new(error_response(
                    ErrorCode::SchemaInvalid,
                    "client header does not match the token",
                    None,
                    None,
                )));
            };
            if text != expected {
                return Err(Box::new(error_response(
                    ErrorCode::SchemaInvalid,
                    "client header does not match the token",
                    None,
                    None,
                )));
            }
        }
    }
    Ok(())
}

fn exec_hint(err: &ExecError) -> Option<String> {
    match err {
        ExecError::Protocol {
            step: "publication_drift",
        } => Some(DRIFT_HINT.into()),
        ExecError::Guard(guard) => guard_hint(guard),
        ExecError::Canceled { sqlstate } if sqlstate == "57014" => {
            Some("sqlstate 57014".to_owned())
        }
        _ => None,
    }
}

fn exec_public(err: &ExecError) -> (ErrorCode, &'static str) {
    match err {
        ExecError::Protocol {
            step: "publication_drift",
        } => (
            ErrorCode::Conflict,
            "publication binding is unavailable or has drifted; recompute the whole question",
        ),
        ExecError::Guard(guard) => guard_public(guard),
        ExecError::AuditUnavailable => (ErrorCode::AuditUnavailable, "audit admission failed"),
        ExecError::Timeout { .. } => (ErrorCode::Timeout, "query deadline exceeded"),
        ExecError::Canceled { .. } => (ErrorCode::Timeout, "query canceled"),
        ExecError::TypeRefused { .. } => {
            (ErrorCode::StatementNotAllowed, "result type is not served")
        }
        ExecError::SessionBusy => (ErrorCode::Overloaded, "data session is busy"),
        ExecError::RoleParameter
        | ExecError::RoleReuse
        | ExecError::Limit
        | ExecError::SessionClosed => (ErrorCode::SchemaInvalid, "query session is not usable"),
        ExecError::Database { .. } | ExecError::Protocol { .. } => {
            (ErrorCode::Internal, "the database rejected the statement")
        }
    }
}

fn guard_public(err: &GuardError) -> (ErrorCode, &'static str) {
    let message = match err {
        GuardError::Parse => "SQL parse failed",
        GuardError::FunctionNotAllowed => "function or operator is not allowed",
        GuardError::SignatureNotAllowed { operator: true, .. } => {
            "operator signature is not allowed"
        }
        GuardError::SignatureNotAllowed {
            operator: false, ..
        } => "function signature is not allowed",
        GuardError::RelationNotFound => "relation is not visible",
        GuardError::ColumnNotAllowed { .. } => "column is not allowed",
        GuardError::Unqualified { .. } => "relation must be schema-qualified",
        GuardError::CostTooHigh { .. } => "statement cost is too high",
        GuardError::Write => "write statement is not allowed",
        GuardError::Unsupported { .. } => "SQL construct is not supported",
        GuardError::IdentityMismatch | GuardError::CatalogInvalid { .. } => {
            "policy catalog does not match"
        }
        _ => "statement is not allowed",
    };
    (err.code(), message)
}

fn guard_hint(err: &GuardError) -> Option<String> {
    let hint = err.hint()?;
    if hint.contains("_oo") || hint.contains('\0') {
        None
    } else {
        Some(hint)
    }
}

fn error_text(text: &str) -> Result<ErrorText, ()> {
    serde_json::from_value(Value::String(text.to_owned())).map_err(|_| ())
}

fn new_server_id() -> Result<String, ()> {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut raw = [0u8; 26];
    getrandom::getrandom(&mut raw).map_err(|_| ())?;
    let mut text = String::from("s_");
    for byte in raw {
        text.push(ALPHABET[(byte as usize) % ALPHABET.len()] as char);
    }
    if !server_id_ok(&text) {
        return Err(());
    }
    Ok(text)
}

fn server_id_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("s_") else {
        return false;
    };
    (6..=40).contains(&rest.len()) && rest.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn new_query_id() -> Result<QueryId, ()> {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut raw = [0u8; 26];
    getrandom::getrandom(&mut raw).map_err(|_| ())?;
    let mut text = String::from("q_");
    for byte in raw {
        text.push(ALPHABET[(byte as usize) % ALPHABET.len()] as char);
    }
    QueryId::new(text).map_err(|_| ())
}

pub(crate) fn error_response(
    code: ErrorCode,
    message: &str,
    hint: Option<String>,
    query_id: Option<&str>,
) -> Response {
    let message = match error_text(message) {
        Ok(text) => text,
        Err(()) => error_text("request refused").expect("static error text"),
    };
    let hint = hint.and_then(|text| error_text(&text).ok());
    let query_id = query_id.and_then(|text| QueryId::new(text).ok());
    let envelope = ErrorEnvelope {
        error: ErrorBody::new(code, message, hint, query_id),
    };
    let mut value = serde_json::to_value(&envelope).unwrap_or_else(|_| serde_json::json!({"error":{"code":"INTERNAL","message":"request refused","retryable":true}}));
    omit_nulls(&mut value);
    let bytes = serde_json::to_vec(&value).unwrap_or_else(|_| {
        br#"{"error":{"code":"INTERNAL","message":"request refused","retryable":true}}"#.to_vec()
    });
    json_bytes(code.http_status(), bytes)
}

fn json_ok(value: Value) -> Response {
    let bytes = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
    json_bytes(200, bytes)
}

fn json_bytes(status: u16, body: Vec<u8>) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = code;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(JSON_MEDIA_TYPE),
    );
    apply_auth_stamp(&mut response);
    response
}

fn apply_auth_stamp(response: &mut Response) {
    let Ok(stamp) = AUTH_STAMP.try_with(|stamp| *stamp) else {
        return;
    };
    let headers = response.headers_mut();
    headers.insert(
        axum::http::HeaderName::from_static("x-oo-auth-cache-ttl-ms"),
        HeaderValue::from_static("5000"),
    );
    if let Ok(value) = HeaderValue::from_str(&stamp.remaining_ms.to_string()) {
        headers.insert(
            axum::http::HeaderName::from_static("x-oo-auth-cache-remaining-ms"),
            value,
        );
    }
    headers.insert(
        axum::http::HeaderName::from_static("x-oo-auth-cache"),
        HeaderValue::from_static(if stamp.hit { "hit" } else { "miss" }),
    );
}

fn omit_nulls(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|_, child| !child.is_null());
            for child in map.values_mut() {
                omit_nulls(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                omit_nulls(item);
            }
        }
        _ => {}
    }
}

fn table_key_ok(table: &str) -> bool {
    QualifiedTable::new(table).is_ok() && !table.contains('/') && !table.contains('\\')
}

fn text_field<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, &'static str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("config")
}

fn required(object: &Map<String, Value>, key: &str) -> Result<String, &'static str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or("reviewed_binding")
}

fn one_segment(value: &str) -> Result<&str, &'static str> {
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value == "."
        || value == ".."
    {
        Err("config")
    } else {
        Ok(value)
    }
}

fn login_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        && value != "oo_target_owner"
        && !value.starts_with("pg_")
}

fn read_endpoint(path: &Path, user: &str) -> Result<TlsEndpoint, &'static str> {
    let text = fs::read_to_string(path).map_err(|_| "data_endpoint")?;
    let value: Value = serde_json::from_str(&text).map_err(|_| "data_endpoint")?;
    let object = value.as_object().ok_or("data_endpoint")?;
    for key in object.keys() {
        if key != "host" && key != "port" && key != "database" {
            return Err("data_endpoint");
        }
    }
    let host = text_field(object, "host")?.to_owned();
    let database = text_field(object, "database")?.to_owned();
    let port = object
        .get("port")
        .and_then(Value::as_u64)
        .ok_or("data_endpoint")?;
    let port = u16::try_from(port).map_err(|_| "data_endpoint")?;
    if host.is_empty() || database.is_empty() || port == 0 {
        return Err("data_endpoint");
    }
    Ok(TlsEndpoint {
        host,
        port,
        database,
        user: user.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_roles_reload_atomically_and_never_fall_back_to_stale_permissions() {
        let root = std::env::temp_dir().join(format!(
            "oo-role-reload-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let role = root.join("east");
        fs::create_dir_all(&role).unwrap();
        fs::write(role.join("policy.json"),
            r#"{"config_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","generation":2,"login":"oo_login","project":"depot","protocol":"oo.operator-preview.policy/v1","role":"east","table":{"columns":[{"name":"id","pii":{"class":"none"},"semantic_type":"identifier","status":"confirmed"}],"name":"items","namespace":"app"}}"#).unwrap();
        let original = r#"{"datasets":[{"columns":["id"],"dataset":"app.items"}],"knowledge":{"id":"k","version":"2"},"policy":{"id":"p","version":"2"},"project":"depot","protocol":"oo.builder.role-visibility/v1","role":"oo_r_east"}"#;
        fs::write(role.join("visibility.json"), original).unwrap();
        let cache = ReviewedRoles::load(root.clone()).unwrap();
        let before = cache.snapshot().unwrap();
        assert_eq!(before[0].knowledge_version, "2");
        if cfg!(unix) {
            assert!(Arc::ptr_eq(&before, &cache.snapshot().unwrap()));
        }
        let mut next: Value = serde_json::from_str(original).unwrap();
        next["knowledge"]["version"] = "3".into();
        fs::write(role.join("pending.json"), next.to_string()).unwrap();
        fs::rename(role.join("pending.json"), role.join("visibility.json")).unwrap();
        let after = cache.snapshot().unwrap();
        assert_eq!(after[0].knowledge_version, "3");
        assert_eq!(before[0].knowledge_version, "2");
        fs::write(role.join("visibility.json"), "{unfinished").unwrap();
        assert!(cache.snapshot().is_err());
        assert!(cache.snapshot().is_err());
        next["datasets"][0]["columns"] = serde_json::json!(["id", "hidden"]);
        fs::write(role.join("visibility.json"), next.to_string()).unwrap();
        assert!(cache.snapshot().is_err());
        fs::write(role.join("visibility.json"), original).unwrap();
        assert_eq!(cache.snapshot().unwrap()[0].knowledge_version, "2");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn describe_types_are_intersection_scoped_and_never_reveal_physical_names() {
        use crate::sql_guard::{
            LiteralTypes, PolicyCatalog, PolicyIdentity, ResolvedFunction, ResolvedOperator,
            VisibleColumn, VisibleRelation,
        };
        let relation = VisibleRelation {
            schema: "sales".into(),
            name: "payments".into(),
            physical_schema: "_oo".into(),
            physical_name: "private_table".into(),
            columns: vec![
                VisibleColumn {
                    name: "paid_time".into(),
                    physical_name: "private_time".into(),
                    type_oid: 1114,
                },
                VisibleColumn {
                    name: "hidden".into(),
                    physical_name: "private_hidden".into(),
                    type_oid: 25,
                },
            ],
            pending_unclassified: 0,
            estimated_rows: 0,
        };
        let details = describe_column_details(
            "sales.payments",
            &["paid_time".into(), "pack_only".into()],
            std::slice::from_ref(&relation),
        );
        assert_eq!(
            details,
            vec![serde_json::json!({
                "name": "paid_time", "type_oid": 1114, "type": "timestamp",
            })]
        );
        assert!(
            describe_column_details(
                "sales.other",
                &["paid_time".into()],
                std::slice::from_ref(&relation)
            )
            .is_empty()
        );
        let hint = guard_hint(&GuardError::FunctionNotAllowed).unwrap();
        assert!(hint.contains("TIMESTAMP"));
        assert_eq!(
            exec_hint(&ExecError::Guard(Box::new(GuardError::FunctionNotAllowed))),
            Some(hint)
        );

        let mut catalog = PolicyCatalog {
            identity: PolicyIdentity {
                project: "p".into(),
                role: "r".into(),
                policy_id: "v".into(),
            },
            relations: vec![relation],
            functions: vec![ResolvedFunction {
                oid: 1,
                schema: "pg_catalog".into(),
                name: "sum".into(),
                argument_type_oids: vec![1700],
                aggregate_star: false,
            }],
            operators: vec![ResolvedOperator {
                oid: 2,
                procedure_oid: 3,
                schema: "pg_catalog".into(),
                name: ">=".into(),
                left_type_oid: 1114,
                right_type_oid: 1114,
                result_type_oid: 16,
            }],
            literal_types: LiteralTypes {
                integer: Some(23),
                text: Some(25),
                boolean: Some(16),
            },
            null_test_allowed: true,
            large_table_row_threshold: 100,
        };
        let capabilities = query_capabilities(Some(&catalog));
        assert_eq!(capabilities["version"], "gateway.query-capabilities.v1");
        assert_eq!(capabilities["status"], "available");
        assert_eq!(capabilities["source"], "runtime_policy_catalog");
        assert_eq!(capabilities["functions"][0]["name"], "sum");
        assert_eq!(capabilities["functions"][0]["argument_types"][0], "numeric");
        assert_eq!(capabilities["operators"][0]["left_type"], "timestamp");
        assert_eq!(capabilities["operators"][0]["result_type"], "bool");
        assert_eq!(capabilities["operators"][0]["result_type_oid"], 16);
        assert!(!capabilities.to_string().contains("private_"));
        assert_eq!(capabilities["requires_validate"], true);
        assert_eq!(capabilities["truncated"], false);
        assert_eq!(capabilities["functions"][0]["argument_type_oids"][0], 1700);
        catalog.operators.push(ResolvedOperator {
            oid: 1759,
            procedure_oid: 1725,
            schema: "pg_catalog".into(),
            name: "-".into(),
            left_type_oid: 1700,
            right_type_oid: 1700,
            result_type_oid: 1700,
        });
        let arithmetic = query_capabilities(Some(&catalog));
        assert_eq!(arithmetic["status"], "available");
        assert_eq!(arithmetic["operators"][1]["name"], "-");
        assert_eq!(arithmetic["operators"][1]["result_type"], "numeric");
        assert_eq!(arithmetic["operators"][1]["result_type_oid"], 1700);
        for invalid in [0, 16, 25, u32::MAX] {
            catalog.operators[1].result_type_oid = invalid;
            assert_eq!(query_capabilities(Some(&catalog))["status"], "unavailable");
        }
        catalog.operators[1].result_type_oid = 1700;
        catalog.operators[1].left_type_oid = 25;
        assert_eq!(query_capabilities(Some(&catalog))["status"], "unavailable");
        catalog.operators.pop();
        catalog.functions[0].argument_type_oids = vec![20];
        assert_eq!(
            query_capabilities(Some(&catalog))["functions"][0]["argument_types"][0],
            "int8"
        );
        catalog.functions.clear();
        catalog.operators.clear();
        catalog.null_test_allowed = false;
        let empty = query_capabilities(Some(&catalog));
        assert_eq!(empty["functions"], serde_json::json!([]));
        assert_eq!(empty["operators"], serde_json::json!([]));
        assert_eq!(empty["null_test"], false);
        let absent = query_capabilities(None);
        assert_eq!(absent["status"], "unavailable");
        assert!(absent.get("functions").is_none());

        catalog.functions.push(ResolvedFunction {
            oid: 1,
            schema: "private_schema".into(),
            name: "private_function".into(),
            argument_type_oids: vec![1700],
            aggregate_star: false,
        });
        let private = query_capabilities(Some(&catalog));
        assert_eq!(private["status"], "unavailable");
        assert!(!private.to_string().contains("private_"));
        catalog.functions[0].schema = "pg_catalog".into();
        catalog.functions[0].name = "sum".into();
        catalog.functions[0].argument_type_oids = vec![u32::MAX];
        assert_eq!(query_capabilities(Some(&catalog))["status"], "unavailable");
        catalog.functions[0].argument_type_oids = vec![1700];
        catalog.functions =
            vec![catalog.functions[0].clone(); CATALOG_LIMITS.max_functions as usize + 1];
        assert_eq!(query_capabilities(Some(&catalog))["status"], "unavailable");
    }

    #[tokio::test]
    async fn publication_binding_failure_has_a_safe_non_sql_recovery_hint() {
        let failure = CatalogFailure::from(CatalogError::PublicationBindingUnavailable);
        let response = error_response(
            failure.code,
            "policy catalog is not available",
            failure.hint,
            None,
        );
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "SCHEMA_INVALID");
        assert_eq!(body["error"]["retryable"], false);
        assert!(body["error"].get("query_id").is_none());
        let hint = body["error"]["hint"].as_str().unwrap();
        assert!(hint.contains("phase=runtime_catalog"));
        assert!(hint.contains("reason=publication_binding_unavailable"));
        assert!(hint.contains("do not rewrite SQL or widen permissions"));
        // Never turn arbitrary database diagnostics into public hints.
        let other = CatalogFailure::from(CatalogError::Database {
            sqlstate: "private_canary".into(),
        });
        assert!(other.hint.is_none());
        assert!(
            CatalogFailure::from(ErrorCode::AuditUnavailable)
                .hint
                .is_none()
        );
    }

    #[tokio::test]
    async fn signature_diagnostic_survives_the_public_error_envelope() {
        for operator in [false, true] {
            let guard = GuardError::SignatureNotAllowed {
                operator,
                argument_type_oids: vec![1114, 25],
                position: 62,
            };
            let hint = guard_hint(&guard).unwrap();
            assert_eq!(
                exec_hint(&ExecError::Guard(Box::new(guard.clone()))),
                Some(hint.clone())
            );
            let (code, message) = guard_public(&guard);
            let response = error_response(code, message, Some(hint.clone()), None);
            assert_eq!(response.status(), 400);
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["error"]["hint"], hint);
            assert_eq!(body["error"]["code"], "FUNCTION_NOT_ALLOWED");
            assert_eq!(body["error"]["retryable"], false);
            assert!(body["error"].get("query_id").is_none());
        }
    }

    #[test]
    fn unsupported_syntax_has_the_same_actionable_hint_on_validate_and_query() {
        let guard = GuardError::Unsupported {
            construct: "HAVING",
        };
        let (code, message) = guard_public(&guard);
        assert_eq!(code, ErrorCode::StatementNotAllowed);
        assert_eq!(message, "SQL construct is not supported");
        let hint = guard_hint(&guard).expect("unsupported hint");
        assert!(hint.contains("HAVING"));
        assert!(hint.contains("not a permission denial"));
        assert_eq!(exec_hint(&ExecError::Guard(Box::new(guard))), Some(hint));
        let denied = GuardError::ColumnNotAllowed {
            available: vec![],
            pending: 0,
            ambiguous: false,
        };
        assert_eq!(guard_public(&denied).0, ErrorCode::ColumnNotAllowed);
        assert!(guard_hint(&denied).is_none());
    }

    #[test]
    fn native_quality_vector_projects_engine_from_sealed_collection_entry() {
        let vector = serde_json::json!([{
            "dataset": "sales.orders",
            "source_id": "dorispreview",
            "run_id": "run-1",
            "source_generation": 7,
            "target_generation": 9
        }]);
        let input = serde_json::json!({
            "catalog": {
                "protocol": "oo.pipeline.sealed-import-collection/v1",
                "binding_vector": vector,
                "entries": [{
                    "source_id": "dorispreview",
                    "engine": "doris",
                    "run_id": "run-1",
                    "source_generation": 7,
                    "target_generation": 9,
                    "tables": [{"logical_schema": "sales", "logical_table": "orders"}]
                }]
            }
        });
        let output = serde_json::json!({"provenance": {
            "source_engine": "doris",
            "binding_vector": vector
        }});

        let projected = authenticated_native_quality_vector(&input, &output, &vector).unwrap();
        assert_eq!(projected[0]["source_engine"], "doris");

        for (field, value) in [
            ("engine", serde_json::json!("oracle")),
            ("engine", serde_json::Value::Null),
            ("run_id", serde_json::json!("other-run")),
        ] {
            let mut bad = input.clone();
            if value.is_null() {
                bad["catalog"]["entries"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove(field);
            } else {
                bad["catalog"]["entries"][0][field] = value;
            }
            assert!(authenticated_native_quality_vector(&bad, &output, &vector).is_none());
        }

        let mut bad_table = input.clone();
        bad_table["catalog"]["entries"][0]["tables"][0]["logical_table"] = "customers".into();
        assert!(authenticated_native_quality_vector(&bad_table, &output, &vector).is_none());
    }

    #[test]
    fn data_keys_are_optional_and_named() {
        assert!(data_config_key("wal_dir"));
        assert!(data_config_key("audit_collector_dir"));
        assert!(!data_config_key("password"));
    }

    #[test]
    fn operator_policy_matches_visibility_and_refuses_a_wider_column() {
        let root = std::env::temp_dir().join(format!("oo-pub-policy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("policy.json"),
            r#"{"config_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","generation":2,"login":"oo_login","project":"depot","protocol":"oo.operator-preview.policy/v1","role":"east","table":{"columns":[{"name":"note_id","pii":{"class":"none"},"semantic_type":"identifier","status":"confirmed"}],"name":"delivery_notes","namespace":"customer_east"}}"#,
        )
        .unwrap();
        fs::write(
            root.join("visibility.json"),
            r#"{"datasets":[{"columns":["note_id"],"dataset":"customer_east.delivery_notes"}],"knowledge":{"id":"know-customer_east","version":"2"},"policy":{"id":"pol-customer_east","version":"2"},"project":"depot","protocol":"oo.builder.role-visibility/v1","role":"oo_r_east"}"#,
        )
        .unwrap();
        let published = load_operator_policy(&root).expect("policy");
        assert_eq!(published.role, "oo_r_east");
        assert_eq!(published.policy_version, "2");
        assert_eq!(published.knowledge_version, "2");
        fs::write(
            root.join("visibility.json"),
            r#"{"datasets":[{"columns":["note_id","hidden"],"dataset":"customer_east.delivery_notes"}],"knowledge":{"id":"know-customer_east","version":"2"},"policy":{"id":"pol-customer_east","version":"2"},"project":"depot","protocol":"oo.builder.role-visibility/v1","role":"oo_r_east"}"#,
        )
        .unwrap();
        assert!(load_operator_policy(&root).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_cancel_phase_does_not_reuse_the_first_wal_name() {
        let root = std::env::temp_dir().join(format!("oo-pub-cancel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert_eq!(fresh_audit_phase(&root, "q_abc", "cancel"), "cancel");
        fs::write(root.join("q_abc-cancel.wal"), b"{}\n").unwrap();
        assert_eq!(fresh_audit_phase(&root, "q_abc", "cancel"), "cancel-2");
        fs::write(root.join("q_abc-cancel-2.wal"), b"{}\n").unwrap();
        assert_eq!(fresh_audit_phase(&root, "q_abc", "cancel"), "cancel-3");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn current_quality_requires_complete_published_same_epoch_receipts() {
        use crate::query_executor::*;
        let mut s = QualitySnapshot::blank("read");
        s.probes.push(RelationProbe {
            name: "physical_mappings",
            state: "read",
        });
        s.capture_method = "c1_cdc".into();
        s.delete_detection = "native".into();
        s.mappings.push(MappingObserved {
            source: "shop".into(),
            source_engine: None,
            schema: "s".into(),
            table: "t".into(),
            run_generation: 5,
            bound: true,
        });
        s.freshness.push(FreshnessObserved {
            schema: "s".into(),
            table: "t".into(),
            state: "live".into(),
            source_committed_at: "2026-10-15T14:00:00.000000Z".into(),
            applied_at: "2026-10-15T14:00:01.000000Z".into(),
        });
        s.runs.push(ImportRunObserved {
            source: "shop".into(),
            source_generation: 5,
            target_generation: 5,
            state: "published".into(),
            sealed: true,
        });
        s.imports.push(ImportTableObserved {
            schema: "s".into(),
            table: "t".into(),
            run_generation: 5,
            applied_rows: 2,
            applied_pos: "37".into(),
            exhausted: true,
        });
        let lineage = "mysql.11111111-2222-4333-8444-555555555555";
        s.watermarks.push(WatermarkObserved {
            source: "shop".into(),
            consistency: "transactional".into(),
            consistency_grade: "T".into(),
            cross_table_consistent: true,
            applied_pos: Some("37".into()),
            state: "live".into(),
            codec: "mysql-gtid/v1".into(),
            lineage: lineage.into(),
            source_epoch: Some(1),
        });
        s.receipts.push(ReceiptObserved {
            source: "shop".into(),
            schema: "s".into(),
            table: "t".into(),
            run_generation: 5,
            applied_pos: "37".into(),
            codec: "mysql-gtid/v1".into(),
            lineage: lineage.into(),
            source_epoch: 1,
            cross_table_consistent: true,
            partial_txn: false,
            target_commit_at: "2026-10-15T14:00:01.000000Z".into(),
        });
        assert!(
            freshness_row(&s.freshness[0]).is_some(),
            "freshness row: {:?}",
            s.freshness[0]
        );
        assert!(ready_quality(&s));
        assert!(gap_warning(&s).is_none());
        let mut baseline = s.clone();
        baseline.mappings[0].source_engine = Some("mysql".into());
        baseline.freshness[0].source_committed_at.clear();
        assert!(ready_quality(&baseline));
        assert!(gap_warning(&baseline).is_none());
        let document = freshness_document(&baseline, "knowledge", "policy");
        assert_eq!(document["source_time"], "unmeasured");
        assert_eq!(document["freshness"], serde_json::json!([]));
        assert_eq!(
            document["target_observations"][0]["target_applied_at"],
            s.freshness[0].applied_at
        );
        assert!(document["target_observations"][0]["source_committed_at"].is_null());
        for kind in [
            "apply_time",
            "bad_source_time",
            "negative_lag",
            "other_engine",
            "other_codec",
            "epoch",
            "unpublished",
            "partial",
            "position",
        ] {
            let mut bad = baseline.clone();
            match kind {
                "apply_time" => bad.freshness[0].applied_at.clear(),
                "bad_source_time" => bad.freshness[0].source_committed_at = "not-a-time".into(),
                "negative_lag" => {
                    bad.freshness[0].source_committed_at = "2026-10-15T14:00:02Z".into()
                }
                "other_engine" => bad.mappings[0].source_engine = Some("mariadb".into()),
                "other_codec" => bad.watermarks[0].codec = "mariadb-gtid/v1".into(),
                "epoch" => bad.receipts[0].source_epoch = 2,
                "unpublished" => bad.runs[0].state = "awaiting_publish".into(),
                "partial" => bad.receipts[0].partial_txn = true,
                _ => bad.receipts[0].applied_pos = "36".into(),
            }
            assert!(!ready_quality(&bad), "unknown-time accepted {kind}");
        }
        for kind in [
            "unpublished",
            "epoch",
            "held",
            "missing_run",
            "missing_time",
            "partial",
            "position",
        ] {
            let mut bad = s.clone();
            match kind {
                "unpublished" => bad.runs[0].state = "awaiting_publish".into(),
                "epoch" => bad.receipts[0].source_epoch = 2,
                "held" => bad.mappings[0].bound = false,
                "missing_run" => bad.runs.clear(),
                "missing_time" => bad.freshness[0].source_committed_at.clear(),
                "partial" => bad.receipts[0].partial_txn = true,
                _ => bad.receipts[0].applied_pos = "36".into(),
            }
            assert!(!ready_quality(&bad), "{kind}");
            assert!(gap_warning(&bad).is_some(), "{kind}");
        }
    }

    #[test]
    fn snapshot_zero_per_table_requires_a_complete_matching_import_receipt() {
        use crate::query_executor::*;
        let snapshot = || {
            let mut s = QualitySnapshot::blank("read");
            s.probes.push(RelationProbe {
                name: "physical_mappings",
                state: "read",
            });
            s.capture_method = "c5_diff".into();
            s.delete_detection = "reconcile".into();
            s.mappings.push(MappingObserved {
                source: "oracle_oneclick_orders".into(),
                source_engine: Some("oracle".into()),
                schema: "s".into(),
                table: "t".into(),
                run_generation: 11,
                bound: true,
            });
            s.freshness.push(FreshnessObserved {
                schema: "s".into(),
                table: "t".into(),
                state: "live".into(),
                source_committed_at: String::new(),
                applied_at: "2026-10-16T01:50:46.000000Z".into(),
            });
            s.runs.push(ImportRunObserved {
                source: "oracle_oneclick_orders".into(),
                source_generation: 11,
                target_generation: 11,
                state: "published".into(),
                sealed: true,
            });
            s.imports.push(ImportTableObserved {
                schema: "s".into(),
                table: "t".into(),
                run_generation: 11,
                applied_rows: 4,
                applied_pos: "0".into(),
                exhausted: true,
            });
            s.watermarks.push(WatermarkObserved {
                source: "oracle_oneclick_orders".into(),
                consistency: "per_table".into(),
                consistency_grade: "S0".into(),
                cross_table_consistent: false,
                applied_pos: Some("0".into()),
                state: "live".into(),
                codec: "snapshot-zero/v1".into(),
                lineage: "configured.oracle".into(),
                source_epoch: Some(0),
            });
            s.receipts.push(ReceiptObserved {
                source: "oracle_oneclick_orders".into(),
                schema: "s".into(),
                table: "t".into(),
                run_generation: 11,
                applied_pos: "0".into(),
                codec: "snapshot-zero/v1".into(),
                lineage: "full-import.snapshot".into(),
                source_epoch: 0,
                cross_table_consistent: false,
                partial_txn: false,
                target_commit_at: "2026-10-16T01:50:46.000000Z".into(),
            });
            s
        };

        let good = snapshot();
        assert!(ready_quality(&good));
        assert!(gap_warning(&good).is_none());
        assert!(freshness_row(&good.freshness[0]).is_none());
        assert_eq!(source_time_status(&good), "unmeasured");
        assert_eq!(global_position_status(&good), "measured");

        for kind in [
            "no_engine",
            "unknown_engine",
            "watermark_lineage",
            "receipt_lineage",
            "codec",
            "position",
            "epoch",
            "watermark_cross_table",
            "receipt_cross_table",
            "partial",
            "unbound",
            "unpublished",
            "unsealed",
            "not_exhausted",
            "missing_receipt",
            "bad_apply_time",
            "missing_target_time",
        ] {
            let mut bad = snapshot();
            match kind {
                "no_engine" => bad.mappings[0].source_engine = None,
                "unknown_engine" => {
                    bad.mappings[0].source_engine = Some("not-an-engine".into());
                    bad.watermarks[0].lineage = "configured.not-an-engine".into();
                }
                "watermark_lineage" => bad.watermarks[0].lineage = "configured.mysql".into(),
                "receipt_lineage" => bad.receipts[0].lineage = "configured.oracle".into(),
                "codec" => bad.receipts[0].codec = "pg-commit-lsn/v1".into(),
                "position" => bad.watermarks[0].applied_pos = Some("1".into()),
                "epoch" => bad.receipts[0].source_epoch = 1,
                "watermark_cross_table" => bad.watermarks[0].cross_table_consistent = true,
                "receipt_cross_table" => bad.receipts[0].cross_table_consistent = true,
                "partial" => bad.receipts[0].partial_txn = true,
                "unbound" => bad.mappings[0].bound = false,
                "unpublished" => bad.runs[0].state = "awaiting_publish".into(),
                "unsealed" => bad.runs[0].sealed = false,
                "not_exhausted" => bad.imports[0].exhausted = false,
                "missing_receipt" => bad.receipts.clear(),
                "bad_apply_time" => bad.freshness[0].applied_at.clear(),
                _ => bad.receipts[0].target_commit_at.clear(),
            }
            assert!(!ready_quality(&bad), "{kind}");
            assert!(gap_warning(&bad).is_some(), "{kind}");
        }
    }

    fn native_mysql_c5_snapshot() -> QualitySnapshot {
        use crate::query_executor::*;
        let mut s = QualitySnapshot::blank("read");
        s.capture_method = "c5_diff".into();
        s.delete_detection = "reconcile".into();
        s.probes.push(RelationProbe {
            name: "physical_mappings",
            state: "read",
        });
        for (table, generation, stamp) in [
            ("orders", 1, "2026-10-17T01:00:00.000001Z"),
            ("departments", 2, "2026-10-17T01:01:00.000002Z"),
        ] {
            s.mappings.push(MappingObserved {
                source: "snapshot_source".into(),
                source_engine: Some("mysql".into()),
                schema: "sales".into(),
                table: table.into(),
                run_generation: generation,
                bound: true,
            });
            s.freshness.push(FreshnessObserved {
                schema: "sales".into(),
                table: table.into(),
                state: "live".into(),
                source_committed_at: String::new(),
                applied_at: "2026-10-17T01:02:00.000003Z".into(),
            });
            s.runs.push(ImportRunObserved {
                source: "snapshot_source".into(),
                source_generation: generation,
                target_generation: generation,
                state: "published".into(),
                sealed: true,
            });
            s.imports.push(ImportTableObserved {
                schema: "sales".into(),
                table: table.into(),
                run_generation: generation,
                applied_rows: 2,
                applied_pos: "0".into(),
                exhausted: true,
            });
            s.receipts.push(ReceiptObserved {
                source: "snapshot_source".into(),
                schema: "sales".into(),
                table: table.into(),
                run_generation: generation,
                applied_pos: "0".into(),
                codec: "snapshot-zero/v1".into(),
                lineage: format!("snapshot_source.{table}"),
                source_epoch: 0,
                cross_table_consistent: false,
                partial_txn: false,
                target_commit_at: stamp.into(),
            });
            s.table_quality.push(TableQualityObserved {
                schema: "sales".into(),
                table: table.into(),
                capture_method: "c5_diff".into(),
                delete_detection: "reconcile".into(),
                c5_mode: None,
                last_full_digest_scan_completed_at: None,
                recent_window_watermark: None,
            });
        }
        s.watermarks.push(WatermarkObserved {
            source: "snapshot_source".into(),
            consistency: "per_table".into(),
            consistency_grade: "B".into(),
            cross_table_consistent: false,
            applied_pos: Some("0".into()),
            state: "live".into(),
            codec: "snapshot-zero/v1".into(),
            lineage: "snapshot_source.departments".into(),
            source_epoch: None,
        });
        s
    }

    #[test]
    fn c5plus_observations_are_not_source_time_or_readiness() {
        let mut s = native_mysql_c5_snapshot();
        assert!(
            !project_quality(&s)
                .warnings
                .iter()
                .any(|w| w.code == WarningCode::DeletesLagged)
        );
        s.table_quality[0].c5_mode = Some("layered".into());
        s.table_quality[0].last_full_digest_scan_completed_at = Some("2026-10-19T01:02:03Z".into());
        s.table_quality[0].recent_window_watermark = Some("2026-10-19T01:00:00Z".into());
        assert!(ready_quality(&s));
        let encoded = serde_json::to_value(project_quality(&s).warnings).unwrap();
        let decoded: Vec<Warning> = serde_json::from_value(encoded).unwrap();
        assert!(decoded.iter().any(
            |w| w.code == WarningCode::DeletesLagged && w.c5_mode.as_deref() == Some("layered")
        ));
        let doc = freshness_document(&s, "knowledge", "policy");
        assert_eq!(doc["source_time"], "unmeasured");
        assert_eq!(doc["table_quality"][0]["c5_mode"], "layered");
        assert_eq!(
            doc["target_observations"][0]["recent_window_watermark"],
            "2026-10-19T01:00:00Z"
        );
        assert!(doc["target_observations"][0]["source_committed_at"].is_null());
        assert!(
            doc["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w["code"] == "DELETES_LAGGED"
                    && w["c5_mode"] == "layered"
                    && w["last_full_digest_scan_completed_at"] == "2026-10-19T01:02:03Z")
        );
        s.table_quality[0].last_full_digest_scan_completed_at = None;
        s.table_quality[0].recent_window_watermark = None;
        let doc = freshness_document(&s, "knowledge", "policy");
        assert!(doc["table_quality"][0]["recent_window_watermark"].is_null());
        assert!(ready_quality(&s));
        s.receipts.clear();
        assert!(!ready_quality(&s));
    }

    #[test]
    fn library_ready_receipt_is_not_stale_only_because_source_engine_is_unmeasured() {
        use crate::query_executor::LibraryObserved;
        let mut snapshot = native_mysql_c5_snapshot();
        for mapping in &mut snapshot.mappings {
            mapping.source_engine = None;
        }
        snapshot.libraries.push(LibraryObserved {
            source: "snapshot_source".into(),
            library_id: "library".into(),
            receipt_sha256: "a".repeat(64),
            ready: true,
        });
        assert!(ready_quality(&snapshot));
        assert!(gap_warning(&snapshot).is_none());
        let mut bad = snapshot.clone();
        bad.libraries[0].ready = false;
        assert!(!ready_quality(&bad));
        let mut bad = snapshot.clone();
        bad.imports[0].exhausted = false;
        assert!(!ready_quality(&bad));
        let mut bad = snapshot.clone();
        bad.runs[0].sealed = false;
        assert!(!ready_quality(&bad));
        let mut bad = snapshot;
        bad.freshness[0].applied_at = "invalid".into();
        assert!(!ready_quality(&bad));
    }

    #[test]
    fn library_readiness_is_closed_and_observations_keep_all_source_identities() {
        use crate::query_executor::{LibraryObserved, QualitySnapshot};
        let mut snapshot = QualitySnapshot::blank("read");
        snapshot.capture_method = "c5_diff".into();
        snapshot.delete_detection = "reconcile".into();
        for index in 0..20 {
            let mut part = native_mysql_c5_snapshot();
            let source = format!("shard_{index}");
            let schema = format!("domain_{index}");
            for m in &mut part.mappings {
                m.source = source.clone();
                m.schema = schema.clone();
            }
            for f in &mut part.freshness {
                f.schema = schema.clone();
            }
            for r in &mut part.runs {
                r.source = source.clone();
            }
            for r in &mut part.receipts {
                r.source = source.clone();
                r.schema = schema.clone();
                r.lineage = r.lineage.replace("snapshot_source", &source);
            }
            for i in &mut part.imports {
                i.schema = schema.clone();
            }
            for q in &mut part.table_quality {
                q.schema = schema.clone();
            }
            for w in &mut part.watermarks {
                w.source = source.clone();
                w.lineage = w.lineage.replace("snapshot_source", &source);
            }
            snapshot.mappings.extend(part.mappings);
            snapshot.freshness.extend(part.freshness);
            snapshot.runs.extend(part.runs);
            snapshot.receipts.extend(part.receipts);
            snapshot.imports.extend(part.imports);
            snapshot.table_quality.extend(part.table_quality);
            snapshot.watermarks.extend(part.watermarks);
            snapshot.libraries.push(LibraryObserved {
                source,
                library_id: "sample-library".into(),
                receipt_sha256: "a".repeat(64),
                ready: true,
            });
        }
        assert!(ready_quality(&snapshot));
        let doc = freshness_document(&snapshot, "knowledge", "policy");
        assert_eq!(doc["target_observations"].as_array().unwrap().len(), 40);
        assert_eq!(doc["source_ids"].as_array().unwrap().len(), 20);
        assert_eq!(doc["source_time"], "unmeasured");
        assert!(
            doc["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w["code"] == "CROSS_TABLE_SKEW")
        );
        snapshot.libraries[19].ready = false;
        assert!(!ready_quality(&snapshot));
        assert_eq!(publication_label(&snapshot), "not_ready");
        assert!(
            freshness_document(&snapshot, "knowledge", "policy")["target_observations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn native_mysql_c5_accepts_actual_per_table_shape_without_source_freshness() {
        let good = native_mysql_c5_snapshot();
        assert!(ready_quality(&good));
        assert!(gap_warning(&good).is_none());
        let document = freshness_document(&good, "knowledge", "policy");
        assert_eq!(document["global_position"], "unmeasured");
        assert_eq!(document["source_time"], "unmeasured");
        assert_eq!(document["freshness"], serde_json::json!([]));
        assert_eq!(document["target_observations"].as_array().unwrap().len(), 2);
        for (index, row) in good.freshness.iter().enumerate() {
            assert!(freshness_row(row).is_none());
            assert_eq!(
                document["target_observations"][index]["target_applied_at"],
                row.applied_at
            );
            assert!(document["target_observations"][index]["source_committed_at"].is_null());
        }
        assert!(
            document["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w["code"] == "CROSS_TABLE_SKEW")
        );
        let mut single_grade = good.clone();
        single_grade.watermarks[0].consistency_grade = "S0".into();
        assert!(ready_quality(&single_grade));
    }

    #[test]
    fn native_mysql_c5_accepts_live_receipt_and_publication_timestamps() {
        let mut snapshot = native_mysql_c5_snapshot();
        for (index, (applied, committed)) in [
            ("2026-10-18T13:30:41.799935Z", "2026-10-18T13:17:01.738021Z"),
            ("2026-10-18T13:30:41.800077Z", "2026-10-18T13:18:52.131502Z"),
        ]
        .into_iter()
        .enumerate()
        {
            snapshot.freshness[index].applied_at = applied.into();
            snapshot.receipts[index].target_commit_at = committed.into();
            snapshot.receipts[index].lineage = "full-import.snapshot".into();
        }
        assert!(ready_quality(&snapshot));
        assert_eq!(mysql_target_observations(&snapshot).len(), 2);
        snapshot.freshness[0].applied_at = "2026-10-18T13:17:01.736021Z".into();
        snapshot.freshness[1].applied_at = "2026-10-18T13:18:52.129502Z".into();
        assert!(ready_quality(&snapshot));
        assert!(gap_warning(&snapshot).is_none());
        assert_eq!(mysql_target_observations(&snapshot).len(), 2);
        for kind in [
            "lineage",
            "receipt_generation",
            "run_generation",
            "apply_time",
            "receipt_time",
        ] {
            let mut bad = snapshot.clone();
            match kind {
                "lineage" => bad.receipts[0].lineage = "arbitrary.snapshot".into(),
                "receipt_generation" => bad.receipts[0].run_generation = 2,
                "run_generation" => bad.runs[0].source_generation = 2,
                "apply_time" => bad.freshness[0].applied_at.clear(),
                "receipt_time" => bad.receipts[0].target_commit_at.clear(),
                _ => unreachable!(),
            }
            assert!(!ready_quality(&bad), "accepted {kind}");
        }
    }

    #[test]
    fn native_mysql_c5_refuses_crossed_incomplete_or_escalated_evidence() {
        let good = native_mysql_c5_snapshot();
        for kind in [
            "generation",
            "receipt_generation",
            "run_generation",
            "target_generation",
            "crossed_target_generation",
            "lineage",
            "crossed_lineage",
            "anchor",
            "old_anchor",
            "anchor_time",
            "epoch",
            "receipt_epoch",
            "position",
            "import_position",
            "codec",
            "unsealed",
            "unpublished",
            "unbound",
            "partial",
            "exhausted",
            "missing_receipt",
            "missing_run",
            "missing_quality",
            "quality",
            "delete",
            "capture",
            "engine",
            "T",
            "S1",
            "cross_table",
            "receipt_cross_table",
            "apply_time",
            "receipt_time",
            "paused",
            "duplicate_receipt",
        ] {
            let mut bad = good.clone();
            match kind {
                "generation" => bad.mappings[0].run_generation = 2,
                "receipt_generation" => bad.receipts[0].run_generation = 2,
                "run_generation" => bad.runs[0].source_generation = 2,
                "target_generation" => bad.runs[0].target_generation = 0,
                "crossed_target_generation" => bad.runs[0].target_generation = 3,
                "lineage" => bad.receipts[0].lineage = "arbitrary.orders".into(),
                "crossed_lineage" => bad.receipts[0].lineage = bad.receipts[1].lineage.clone(),
                "anchor" => bad.watermarks[0].lineage = "arbitrary.departments".into(),
                "old_anchor" => bad.watermarks[0].lineage = bad.receipts[0].lineage.clone(),
                "anchor_time" => {
                    bad.receipts[1].target_commit_at = "2026-10-17T00:00:00Z".into();
                    bad.freshness[1].applied_at = "2026-10-17T00:00:01Z".into();
                }
                "epoch" => bad.watermarks[0].source_epoch = Some(1),
                "receipt_epoch" => bad.receipts[0].source_epoch = 1,
                "position" => bad.receipts[0].applied_pos = "1".into(),
                "import_position" => bad.imports[0].applied_pos = "1".into(),
                "codec" => bad.watermarks[0].codec = "mysql-gtid/v1".into(),
                "unsealed" => bad.runs[0].sealed = false,
                "unpublished" => bad.runs[0].state = "awaiting_publish".into(),
                "unbound" => bad.mappings[0].bound = false,
                "partial" => bad.receipts[0].partial_txn = true,
                "exhausted" => bad.imports[0].exhausted = false,
                "missing_receipt" => {
                    bad.receipts.remove(0);
                }
                "missing_run" => {
                    bad.runs.remove(0);
                }
                "missing_quality" => {
                    bad.table_quality.remove(0);
                }
                "quality" => bad.table_quality[0].capture_method = "c1_cdc".into(),
                "delete" => bad.delete_detection = "native".into(),
                "capture" => bad.capture_method = "c1_cdc".into(),
                "engine" => bad.mappings[0].source_engine = None,
                "T" | "S1" => bad.watermarks[0].consistency_grade = kind.into(),
                "cross_table" => bad.watermarks[0].cross_table_consistent = true,
                "receipt_cross_table" => bad.receipts[0].cross_table_consistent = true,
                "apply_time" => bad.freshness[0].applied_at.clear(),
                "receipt_time" => bad.receipts[0].target_commit_at.clear(),
                "paused" => bad.freshness[0].state = "paused".into(),
                "duplicate_receipt" => bad.receipts.push(bad.receipts[0].clone()),
                _ => unreachable!(),
            }
            assert!(!ready_quality(&bad), "accepted {kind}");
            assert!(gap_warning(&bad).is_some(), "missing refusal {kind}");
            assert!(
                mysql_target_observations(&bad).is_empty(),
                "unauthenticated observation {kind}"
            );
        }
    }

    #[test]
    fn doris_per_table_quality_keeps_mixed_c4_c5_and_null_global_position() {
        use crate::query_executor::*;
        let tables = [
            ("agg_qty", 2, "c5_diff", "reconcile"),
            ("dec_exact", 3, "c5_diff", "reconcile"),
            ("dup_events", 4, "c5_diff", "reconcile"),
            ("nz_obs", 5, "c5_diff", "reconcile"),
            ("part_day", 9, "c4_partition", "partition_replace"),
            ("str_key", 6, "c5_diff", "reconcile"),
            ("uniq_acct", 7, "c5_diff", "reconcile"),
        ];
        let snapshot = || {
            let mut s = QualitySnapshot::blank("read");
            s.probes.extend([
                RelationProbe {
                    name: "physical_mappings",
                    state: "read",
                },
                RelationProbe {
                    name: "table_freshness",
                    state: "read",
                },
                RelationProbe {
                    name: "source_watermark",
                    state: "read",
                },
                RelationProbe {
                    name: "table_quality",
                    state: "read",
                },
                RelationProbe {
                    name: "apply_receipts",
                    state: "read",
                },
                RelationProbe {
                    name: "full_import_run",
                    state: "read",
                },
                RelationProbe {
                    name: "full_import_table",
                    state: "read",
                },
                RelationProbe {
                    name: "full_import_quality",
                    state: "read",
                },
            ]);
            s.capture_method = "mixed".into();
            s.delete_detection = "mixed".into();
            s.watermarks.push(WatermarkObserved {
                source: "dorispreview".into(),
                consistency: "per_table".into(),
                consistency_grade: "S0".into(),
                cross_table_consistent: false,
                applied_pos: None,
                state: "live".into(),
                codec: "partition-signature/v1".into(),
                lineage: "partition-map".into(),
                source_epoch: None,
            });
            for (table, generation, capture, deletion) in tables {
                s.mappings.push(MappingObserved {
                    source: "dorispreview".into(),
                    source_engine: Some("doris".into()),
                    schema: "oo_dl01w1_43".into(),
                    table: table.into(),
                    run_generation: generation,
                    bound: true,
                });
                s.freshness.push(FreshnessObserved {
                    schema: "oo_dl01w1_43".into(),
                    table: table.into(),
                    state: "live".into(),
                    source_committed_at: String::new(),
                    applied_at: "2026-10-16T02:00:00.000000Z".into(),
                });
                s.runs.push(ImportRunObserved {
                    source: "dorispreview".into(),
                    source_generation: generation,
                    target_generation: generation,
                    state: "published".into(),
                    sealed: true,
                });
                s.imports.push(ImportTableObserved {
                    schema: "oo_dl01w1_43".into(),
                    table: table.into(),
                    run_generation: generation,
                    applied_rows: 1,
                    applied_pos: String::new(),
                    exhausted: true,
                });
                s.table_quality.push(TableQualityObserved {
                    schema: "oo_dl01w1_43".into(),
                    table: table.into(),
                    capture_method: capture.into(),
                    delete_detection: deletion.into(),
                    c5_mode: None,
                    last_full_digest_scan_completed_at: None,
                    recent_window_watermark: None,
                });
            }
            s
        };
        let good = snapshot();
        assert!(ready_quality(&good));
        assert_eq!(source_time_status(&good), "unmeasured");
        assert_eq!(global_position_status(&good), "unmeasured");
        let document = freshness_document(&good, "knowledge-v", "policy-v");
        assert!(document["sources"].as_array().unwrap().is_empty());
        assert_eq!(
            document["source_watermarks"][0]["applied_pos"],
            serde_json::Value::Null
        );
        assert_eq!(
            document["source_watermarks"][0]["source_epoch"],
            serde_json::Value::Null
        );
        assert_eq!(document["source_watermarks"][0]["consistency_grade"], "S0");
        assert_eq!(document["table_quality"].as_array().unwrap().len(), 7);
        assert_eq!(
            document["table_quality"][4]["capture_method"],
            "c4_partition"
        );

        for mutate in [
            "missing_engine",
            "invented_position",
            "invented_epoch",
            "wrong_lineage",
            "missing_quality",
            "wrong_delete",
            "unpublished",
        ] {
            let mut bad = snapshot();
            match mutate {
                "missing_engine" => bad.mappings[0].source_engine = None,
                "invented_position" => bad.watermarks[0].applied_pos = Some("0".into()),
                "invented_epoch" => bad.watermarks[0].source_epoch = Some(0),
                "wrong_lineage" => bad.watermarks[0].lineage = "partition-map-copy".into(),
                "missing_quality" => bad.table_quality.retain(|q| q.table != "part_day"),
                "wrong_delete" => bad.table_quality[4].delete_detection = "reconcile".into(),
                _ => bad.runs[4].state = "awaiting_publish".into(),
            }
            assert!(!ready_quality(&bad), "accepted {mutate}");
        }
    }

    #[test]
    fn freshness_lag_uses_both_stamps() {
        let source = "2000-01-01T00:00:00.000000Z";
        let applied = "2000-01-01T00:00:01.500000Z";
        assert_eq!(lag_between(source, applied), Some(1500));
        assert!(lag_between(applied, source).is_none());
        assert_eq!(
            lag_between("2000-01-01T00:00:00Z", "2000-01-01T00:00:01.5Z"),
            Some(1500)
        );
    }
}

fn load_native_reviewed_role(dir: &Path) -> Result<Published, &'static str> {
    let input =
        fs::read_to_string(dir.join("native-input.json")).map_err(|_| "reviewed_native_input")?;
    let output =
        fs::read_to_string(dir.join("command.json")).map_err(|_| "reviewed_native_command")?;
    let binding_text =
        fs::read_to_string(dir.join("binding.json")).map_err(|_| "reviewed_binding")?;
    let binding = parse_binding(&binding_text)?;
    let reviewed =
        crate::reviewed_policy::compile_native_reviewed_policies(&input, &output, &binding)
            .map_err(|_| "reviewed_native_policy")?;
    if reviewed.policies.is_empty() {
        return Err("reviewed_native_policy");
    }
    let input_value: Value = serde_json::from_str(&input).map_err(|_| "reviewed_native_input")?;
    let output_value: Value =
        serde_json::from_str(&output).map_err(|_| "reviewed_native_command")?;
    let native_catalog = if binding.binding_vector.is_some() {
        Some(input_value["catalog"].clone())
    } else {
        None
    };
    let quality_vector = if let Some(vector) = &binding.binding_vector {
        Some(
            authenticated_native_quality_vector(&input_value, &output_value, vector)
                .ok_or("reviewed_native_quality_vector")?,
        )
    } else {
        single_import_quality_vector(&input_value, &output_value, &binding, &reviewed.policies)
    };
    Ok(Published {
        mapping_source: Some(binding.seal.source_id.clone()),
        native_vector: binding.binding_vector.clone(),
        native_catalog,
        quality_vector,
        role: reviewed.compiled.role().to_string(),
        project: binding.identity.project,
        policy_version: binding.identity.policy_version,
        knowledge_version: binding.identity.knowledge_version,
        config_digest: if binding.binding_vector.is_some() {
            reviewed
                .policies
                .first()
                .map(|policy| policy.config_digest.clone())
                .ok_or("reviewed_native_policy")?
        } else {
            binding.config_digest
        },
        policies: reviewed.policies,
    })
}

// The sealed collection binding vector carries run and table identity, while the
// closed catalog entries carry the engine. Join them only after reviewed-policy
// compilation has authenticated the collection and its generated command.
fn authenticated_native_quality_vector(
    input: &Value,
    output: &Value,
    vector: &Value,
) -> Option<Value> {
    let catalog = &input["catalog"];
    let provenance = &output["provenance"];
    let items = vector.as_array()?;
    let entries = catalog["entries"].as_array()?;
    let engine = provenance["source_engine"].as_str()?;
    if items.is_empty()
        || entries.len() != items.len()
        || catalog["protocol"] != "oo.pipeline.sealed-import-collection/v1"
        || catalog["binding_vector"] != *vector
        || provenance["binding_vector"] != *vector
        || !crate::query_executor::supported_source_engine(engine)
    {
        return None;
    }

    let mut datasets = std::collections::HashSet::new();
    let mut authenticated = Vec::with_capacity(items.len());
    for item in items {
        let dataset = item["dataset"].as_str()?;
        if !datasets.insert(dataset) {
            return None;
        }
        let (schema, table) = dataset.split_once('.')?;
        if schema.is_empty() || table.is_empty() || table.contains('.') {
            return None;
        }
        let source = item["source_id"].as_str()?;
        let run_id = item["run_id"].as_str()?;
        let source_generation = item["source_generation"].as_i64()?;
        let target_generation = item["target_generation"].as_i64()?;
        let matching: Vec<_> = entries
            .iter()
            .filter(|entry| {
                entry["source_id"] == source
                    && entry["run_id"] == run_id
                    && entry["source_generation"].as_i64() == Some(source_generation)
                    && entry["target_generation"].as_i64() == Some(target_generation)
                    && entry["tables"].as_array().is_some_and(|tables| {
                        tables.iter().any(|candidate| {
                            candidate["logical_schema"] == schema
                                && candidate["logical_table"] == table
                        })
                    })
            })
            .collect();
        if matching.len() != 1 || matching[0]["engine"].as_str() != Some(engine) {
            return None;
        }
        if item
            .get("source_engine")
            .is_some_and(|bound| bound.as_str() != Some(engine))
        {
            return None;
        }
        let mut binding = item.clone();
        binding["source_engine"] = Value::String(engine.to_owned());
        authenticated.push(binding);
    }
    Some(Value::Array(authenticated))
}

fn single_import_quality_vector(
    input: &Value,
    output: &Value,
    binding: &crate::reviewed_policy::ReviewedBinding,
    policies: &[ColumnAclPolicy],
) -> Option<Value> {
    let catalog = &input["catalog"];
    let parsed = oo_core::sealed_import::parse_sealed_import_catalog(&catalog.to_string()).ok()?;
    let table = parsed.tables.first()?;
    let dataset = format!("{}.{}", table.logical_schema, table.logical_table);
    let raw_table = catalog["tables"].as_array()?.first()?;
    let stored_namespace = raw_table["stored_namespace"].as_str()?;
    let stored_physical = raw_table["stored_physical"].as_str()?;
    let provenance = &output["provenance"];
    if input["protocol"] != "oo.builder.reviewed-sealed-import-command/v1"
        || parsed.tables.len() != 1
        || !crate::query_executor::supported_source_engine(&parsed.engine)
        || parsed.capture != "sealed_import"
        || catalog["state"] != "awaiting_publish"
        || parsed.source_id != binding.seal.source_id
        || parsed.source_generation != binding.generation
        || parsed.config_digest != binding.config_digest
        || parsed.schema_digest != binding.seal.schema_hash
        || provenance["source_engine"] != parsed.engine
        || provenance["source_id"] != parsed.source_id
        || provenance["source_generation"].as_i64() != Some(parsed.source_generation)
        || provenance["target_generation"].as_i64() != Some(parsed.target_generation)
        || provenance["config_digest"] != parsed.config_digest
        || provenance["schema_digest"] != parsed.schema_digest
        || provenance["type_digest"] != parsed.type_digest
        || provenance["input_digest"] != parsed.input_digest
        || policies.len() != 1
        || policies[0].table.namespace != table.logical_schema
        || policies[0].table.name != table.logical_table
        || binding.allow.is_empty()
        || binding.allow.iter().any(|allow| allow.dataset != dataset)
    {
        return None;
    }
    Some(serde_json::json!([{
        "dataset": dataset,
        "source_id": parsed.source_id,
        "source_engine": parsed.engine,
        "run_id": parsed.run_id,
        "source_generation": parsed.source_generation,
        "target_generation": parsed.target_generation,
        "config_digest": parsed.config_digest,
        "schema_digest": parsed.schema_digest,
        "type_digest": parsed.type_digest,
        "input_digest": parsed.input_digest,
        "stored_namespace": stored_namespace,
        "stored_physical": stored_physical,
    }]))
}

#[cfg(test)]
mod library_unknown_engine_freshness_tests {
    use super::*;
    #[test]
    fn imported_library_observations_do_not_invent_an_engine_or_source_timestamp() {
        let mut snapshot = QualitySnapshot::blank("read");
        snapshot.capture_method = "c5_diff".into();
        snapshot.delete_detection = "reconcile".into();
        snapshot.mappings.push(MappingObserved {
            source: "shard_a".into(),
            source_engine: None,
            schema: "app".into(),
            table: "items".into(),
            run_generation: 1,
            bound: true,
        });
        snapshot
            .freshness
            .push(crate::query_executor::FreshnessObserved {
                schema: "app".into(),
                table: "items".into(),
                state: "live".into(),
                source_committed_at: String::new(),
                applied_at: "2026-10-20T00:00:00.000000Z".into(),
            });
        snapshot
            .libraries
            .push(crate::query_executor::LibraryObserved {
                source: "shard_a".into(),
                library_id: "library".into(),
                receipt_sha256: "a".repeat(64),
                ready: true,
            });
        snapshot
            .watermarks
            .push(crate::query_executor::WatermarkObserved {
                source: "shard_a".into(),
                consistency: "per_table".into(),
                consistency_grade: "B".into(),
                cross_table_consistent: false,
                applied_pos: Some("0".into()),
                state: "live".into(),
                codec: "snapshot-zero/v1".into(),
                lineage: "shard_a.items".into(),
                source_epoch: None,
            });
        let doc = freshness_document(&snapshot, "1", "1");
        assert_eq!(doc["source_time"], "unmeasured");
        assert_eq!(doc["global_position"], "unmeasured");
        assert_eq!(doc["target_observations"].as_array().unwrap().len(), 1);
        assert!(doc["target_observations"][0]["source_committed_at"].is_null());
        assert!(
            doc["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w["code"] == "CROSS_TABLE_SKEW")
        );
    }
}
