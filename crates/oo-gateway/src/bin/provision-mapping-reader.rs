//! Stdin entry for one mapping-read provision.
//!
//! The operator document carries a reviewed-policy policy-set DTO, an explicit
//! login, and the paths of a trusted password file and CA file. This process
//! calls `provision_mapping_reader`. The login is an administrative binding.
//! It is not an authentication decision. Table `SELECT` on
//! `_oo.physical_mappings` exposes every mapping row in the database.
//! The process does not grant a business column, `PUBLIC`, a source object,
//! or membership in `oo_target_owner`.

#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use oo_core::json_scan::parse_json_value;
use oo_core::limits::DOCUMENT_MAX_BYTES;
use oo_gateway::mapping_reader_access::{
    MappingReaderEffect, MappingReaderError, provision_mapping_reader, provision_mapping_reader_native_vector,
};
use oo_gateway::policy::{compile_column_acl_set, compile_column_acl_native_vector_set};
use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass, PolicyError, RowFilter,
    TlsEndpoint, connect_require_tls,
};
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

const PROTOCOL: &str = "oo.gateway.provision-mapping-reader-cli/v1";
const NATIVE_PROTOCOL: &str = "oo.gateway.provision-native-vector-mapping-reader-cli/v1";
const PASSWORD_FILE_BYTES: usize = 1026;
const PASSWORD_BYTES: usize = 1024;
const CA_BYTES: usize = 1_048_576;

const EXIT_IO: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_BUDGET: u8 = 3;
const EXIT_SCAN: u8 = 4;
const EXIT_REQUEST: u8 = 5;
const EXIT_TLS: u8 = 6;
const EXIT_POLICY: u8 = 7;

fn main() -> ExitCode {
    match std::panic::catch_unwind(run) {
        Ok(code) => code,
        Err(_) => Fail::Io.emit(),
    }
}

fn run() -> ExitCode {
    if std::env::args().nth(1).is_some() {
        return Fail::Usage.emit();
    }
    let text = match read_stdin() {
        Ok(text) => text,
        Err(fail) => return fail.emit(),
    };
    let prepared = match prepare(&text) {
        Ok(prepared) => prepared,
        Err(fail) => return fail.emit(),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return Fail::Io.emit(),
    };
    match runtime.block_on(provision(prepared)) {
        Ok(body) => {
            let mut out = io::stdout().lock();
            if out.write_all(body.as_bytes()).is_err() || out.flush().is_err() {
                return Fail::Io.emit();
            }
            ExitCode::SUCCESS
        }
        Err(fail) => fail.emit(),
    }
}

fn read_stdin() -> Result<String, Fail> {
    let mut stdin = io::stdin().lock();
    let mut buf = Vec::new();
    let max = DOCUMENT_MAX_BYTES.saturating_add(1);
    stdin
        .by_ref()
        .take(max as u64)
        .read_to_end(&mut buf)
        .map_err(|_| Fail::Io)?;
    if buf.len() > DOCUMENT_MAX_BYTES {
        return Err(Fail::Budget);
    }
    let text = std::str::from_utf8(&buf).map_err(|_| Fail::Scan)?;
    Ok(text.to_string())
}

struct Prepared {
    native: bool,
    policies: Vec<ColumnAclPolicy>,
    login: String,
    endpoint: TlsEndpoint,
    password: String,
    ca: Vec<u8>,
}

fn prepare(text: &str) -> Result<Prepared, Fail> {
    let value = parse_json_value(text, PROTOCOL).map_err(|_| Fail::Scan)?;
    reject_null(&value)?;
    let request: RequestDoc = serde_json::from_value(value).map_err(|_| Fail::Request)?;
    let native=request.protocol==NATIVE_PROTOCOL;
    if request.protocol != PROTOCOL && !native {
        return Err(Fail::Request);
    }
    let endpoint = endpoint_from(request.endpoint)?;
    let mut policies = Vec::with_capacity(request.policies.len());
    for policy in &request.policies {
        policies.push(policy.to_acl()?);
    }
    (if native {compile_column_acl_native_vector_set(&policies)} else {compile_column_acl_set(&policies)}).map_err(from_policy)?;
    let Some(first) = policies.first() else {
        return Err(Fail::Policy("empty"));
    };
    if request.login != first.login {
        return Err(Fail::Policy("binding"));
    }
    let login = request.login;
    let password = read_password(Path::new(&request.password_file))?;
    if contains_bytes(text.as_bytes(), password.as_bytes()) {
        return Err(Fail::Request);
    }
    let ca = read_ca(Path::new(&request.ca_file))?;
    Ok(Prepared {
        native,
        policies,
        login,
        endpoint,
        password,
        ca,
    })
}

fn endpoint_from(doc: EndpointDoc) -> Result<TlsEndpoint, Fail> {
    if doc.port == 0
        || !host_ok(&doc.host)
        || !plain_ident(&doc.database)
        || !plain_ident(&doc.user)
    {
        return Err(Fail::Request);
    }
    Ok(TlsEndpoint {
        host: doc.host,
        port: doc.port,
        database: doc.database,
        user: doc.user,
    })
}

async fn provision(prepared: Prepared) -> Result<String, Fail> {
    let (mut client, connection) =
        connect_require_tls(&prepared.endpoint, &prepared.password, &prepared.ca)
            .await
            .map_err(from_connect)?;
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    let result = if prepared.native {provision_mapping_reader_native_vector(&mut client,&prepared.login,&prepared.policies).await} else {provision_mapping_reader(&mut client, &prepared.login, &prepared.policies).await};
    drop(client);
    let _ = driver.await;
    let outcome = result.map_err(from_mapping)?;
    let effect = match outcome.effect {
        MappingReaderEffect::Installed => "installed",
        MappingReaderEffect::Replay => "replay",
    };
    let body = OutcomeDoc {
        protocol: if prepared.native { NATIVE_PROTOCOL } else { PROTOCOL },
        effect,
        login: &outcome.login,
        role: &outcome.role,
    };
    serde_json::to_string(&body).map_err(|_| Fail::Io)
}

fn from_connect(err: PolicyError) -> Fail {
    match err {
        PolicyError::CertificateRejected
        | PolicyError::TlsRequired
        | PolicyError::Database { .. } => Fail::Tls,
        PolicyError::InvalidIdentifier { .. } => Fail::Request,
        other => from_policy(other),
    }
}

fn from_policy(err: PolicyError) -> Fail {
    match err {
        PolicyError::CertificateRejected | PolicyError::TlsRequired => Fail::Tls,
        PolicyError::RowFilterRefused => Fail::Policy("filter"),
        PolicyError::InvalidIdentifier { .. } => Fail::Policy("identifier"),
        PolicyError::ClassificationRefused { .. } => Fail::Policy("classification"),
        PolicyError::EmptyAllowlist | PolicyError::EmptyPolicySet => Fail::Policy("empty"),
        PolicyError::StaleGeneration { .. } => Fail::Policy("stale"),
        PolicyError::GrantorRefused => Fail::Policy("grantor"),
        PolicyError::RoleMismatch => Fail::Policy("role"),
        PolicyError::GrantMismatch => Fail::Policy("mismatch"),
        PolicyError::LoginAbsent => Fail::Policy("login"),
        PolicyError::MappingAbsent => Fail::Policy("mapping"),
        PolicyError::PhysicalAbsent => Fail::Policy("physical"),
        PolicyError::MixedIdentity => Fail::Policy("identity"),
        PolicyError::LimitExceeded => Fail::Policy("limit"),
        PolicyError::TypeRefused { .. } => Fail::Policy("type"),
        PolicyError::MetadataColumn { .. } => Fail::Policy("metadata"),
        PolicyError::MembershipMismatch => Fail::Policy("membership"),
        PolicyError::Database { .. } => Fail::Policy("database"),
    }
}

fn from_mapping(err: MappingReaderError) -> Fail {
    match err {
        MappingReaderError::Policy(err) => from_policy(err),
        MappingReaderError::LoginMismatch => Fail::Policy("binding"),
        MappingReaderError::TlsRequired => Fail::Tls,
        MappingReaderError::GrantorRefused => Fail::Policy("grantor"),
        MappingReaderError::StaleGeneration { .. } => Fail::Policy("stale"),
        MappingReaderError::LoginAbsent => Fail::Policy("login"),
        MappingReaderError::OwnerMembership => Fail::Policy("owner"),
        MappingReaderError::MappingAbsent => Fail::Policy("mapping"),
        MappingReaderError::PhysicalAbsent => Fail::Policy("physical"),
        MappingReaderError::Unsupported { .. } => Fail::Policy("type"),
        MappingReaderError::MembershipMismatch => Fail::Policy("membership"),
        MappingReaderError::RoleMismatch => Fail::Policy("role"),
        MappingReaderError::GrantMismatch => Fail::Policy("mismatch"),
        MappingReaderError::MetadataColumn => Fail::Policy("metadata"),
        MappingReaderError::UnexpectedPrivilege => Fail::Policy("privilege"),
        MappingReaderError::Database { .. } => Fail::Policy("database"),
    }
}

fn reject_null(value: &Value) -> Result<(), Fail> {
    match value {
        Value::Null => Err(Fail::Request),
        Value::Array(items) => {
            for item in items {
                reject_null(item)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for (key, child) in map {
                if key == "row_filter" && child.is_null() {
                    continue;
                }
                reject_null(child)?;
            }
            Ok(())
        }
        Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
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

fn plain_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first = bytes[0];
    if first != b'_' && !first.is_ascii_lowercase() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn read_password(path: &Path) -> Result<String, Fail> {
    let bytes = read_regular(path, PASSWORD_FILE_BYTES)?;
    let trimmed = if bytes.ends_with(b"\r\n") {
        &bytes[..bytes.len() - 2]
    } else if bytes.ends_with(b"\n") {
        &bytes[..bytes.len() - 1]
    } else {
        bytes.as_slice()
    };
    if trimmed.is_empty() || trimmed.len() > PASSWORD_BYTES || trimmed.contains(&0) {
        return Err(Fail::Request);
    }
    String::from_utf8(trimmed.to_vec()).map_err(|_| Fail::Request)
}

fn read_ca(path: &Path) -> Result<Vec<u8>, Fail> {
    let bytes = read_regular(path, CA_BYTES)?;
    if bytes.is_empty() || bytes.windows(11).any(|window| window == b"PRIVATE KEY") {
        return Err(Fail::Tls);
    }
    Ok(bytes)
}

fn read_regular(path: &Path, max: usize) -> Result<Vec<u8>, Fail> {
    if path.as_os_str().is_empty() || path.to_str().is_none_or(|text| text.contains('\0')) {
        return Err(Fail::Request);
    }
    let meta = fs::symlink_metadata(path).map_err(|_| Fail::Request)?;
    if !meta.file_type().is_file() || meta.len() > u64::try_from(max).unwrap_or(u64::MAX) {
        return Err(Fail::Request);
    }
    let mut file = File::open(path).map_err(|_| Fail::Request)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(u64::try_from(max).unwrap_or(0) + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Fail::Request)?;
    if bytes.len() > max {
        return Err(Fail::Request);
    }
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestDoc {
    protocol: String,
    policies: Vec<PolicyDoc>,
    login: String,
    endpoint: EndpointDoc,
    password_file: String,
    ca_file: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointDoc {
    host: String,
    port: u16,
    database: String,
    user: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDoc {
    project: String,
    config_digest: String,
    generation: i64,
    role: String,
    login: String,
    table: TableDoc,
    row_filter: Option<RowFilterDoc>,
}

impl PolicyDoc {
    fn to_acl(&self) -> Result<ColumnAclPolicy, Fail> {
        let row_filter = match &self.row_filter {
            None => None,
            Some(filter) => Some(filter.to_filter()?),
        };
        Ok(ColumnAclPolicy {
            project: self.project.clone(),
            config_digest: self.config_digest.clone(),
            generation: self.generation,
            role: self.role.clone(),
            login: self.login.clone(),
            table: LogicalTable {
                namespace: self.table.namespace.clone(),
                name: self.table.name.clone(),
                columns: self
                    .table
                    .columns
                    .iter()
                    .map(ColumnDoc::to_spec)
                    .collect::<Result<Vec<_>, _>>()?,
            },
            row_filter,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowFilterDoc {
    kind: String,
    column: String,
    claim: String,
}

impl RowFilterDoc {
    fn to_filter(&self) -> Result<RowFilter, Fail> {
        if self.kind != "claims" {
            return Err(Fail::Request);
        }
        Ok(RowFilter::Claims {
            column: self.column.clone(),
            claim: self.claim.clone(),
        })
    }
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

#[derive(Serialize)]
struct OutcomeDoc<'a> {
    protocol: &'static str,
    effect: &'static str,
    login: &'a str,
    role: &'a str,
}

enum Fail {
    Io,
    Usage,
    Budget,
    Scan,
    Request,
    Tls,
    Policy(&'static str),
}

impl Fail {
    fn emit(self) -> ExitCode {
        let (code, line) = match self {
            Self::Io => (EXIT_IO, "provision-mapping-reader: io".to_string()),
            Self::Usage => (EXIT_USAGE, "provision-mapping-reader: usage".to_string()),
            Self::Budget => (EXIT_BUDGET, "provision-mapping-reader: budget".to_string()),
            Self::Scan => (EXIT_SCAN, "provision-mapping-reader: scan".to_string()),
            Self::Request => (
                EXIT_REQUEST,
                "provision-mapping-reader: request".to_string(),
            ),
            Self::Tls => (EXIT_TLS, "provision-mapping-reader: tls".to_string()),
            Self::Policy(reason) => (
                EXIT_POLICY,
                format!("provision-mapping-reader: policy {reason}"),
            ),
        };
        eprintln!("{line}");
        ExitCode::from(code)
    }
}
