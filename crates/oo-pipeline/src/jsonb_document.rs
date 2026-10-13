//! Native BSON document import into PostgreSQL jsonb oid 3802.
//!
//! The capture is `oo.document.bson-jsonb/v1` plus one sealed-import catalog.
//! [`oo_core::jsonb_doc::seal_bson_jsonb_document`] checks the raw BSON, the
//! version-byte file, and the exact number lexemes. `0009` supplies the column
//! type through `_oo.full_import_type_sql`. Binary COPY writes those sealed
//! `jsonb_send` bytes. A refusal rolls the data transaction back and does not
//! mark the catalog ready. This is not a PostgreSQL source import.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use oo_core::jsonb_doc::{self, SealedJsonbDocument};
use oo_core::sealed_import::{self, SealedImportCatalog};
use serde_json::{Map, Value, json};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, PgConnection, Row};

use crate::physical_mapping::quote_physical_name;
use crate::target_installer::{InstallError, JsonbDocumentInstallOutcome, install_jsonb_document};

/// Public import command. Task 41 reads this receipt, not a private dialect.
pub const PROTOCOL: &str = "oo.pipeline.jsonb-document-import/v1";

const MAX_REQUEST: usize = 2_097_152;
const MAX_SECRET: u64 = 512;
const MAX_CA: u64 = 65_536;

/// One import that committed a verified jsonb row. `ready` stays false.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentReceipt {
    pub protocol: &'static str,
    pub outcome: &'static str,
    pub ready: bool,
    pub partial_ready: bool,
    pub pg_source_passed: bool,
    pub rolled_back: bool,
    pub code: Option<String>,
    pub install: String,
    pub source_engine: String,
    pub source_id: String,
    pub dataset: String,
    pub column: String,
    pub catalog_state: &'static str,
    pub schema_digest: String,
    pub raw_bson_sha256: String,
    pub pg_jsonb_sha256: String,
    pub identity_hash: String,
    pub rows: u64,
    pub atttypid: i64,
    pub atttypmod: i32,
    pub type_sql: String,
    pub type_114: String,
    pub typmod_sqlstate: String,
    pub document_text: String,
    pub send_matches: bool,
    pub stored_namespace: String,
    pub target_host: String,
    pub target_database: String,
    pub model: &'static str,
}

impl DocumentReceipt {
    /// Canonical receipt. The document text is the stored jsonb text, not raw BSON.
    pub fn to_json(&self) -> Value {
        json!({
            "protocol": self.protocol,
            "outcome": self.outcome,
            "ready": self.ready,
            "partial_ready": self.partial_ready,
            "pg_source_passed": self.pg_source_passed,
            "rolled_back": self.rolled_back,
            "code": self.code,
            "install": self.install,
            "source_engine": self.source_engine,
            "source_id": self.source_id,
            "dataset": self.dataset,
            "column": self.column,
            "catalog_state": self.catalog_state,
            "schema_digest": self.schema_digest,
            "raw_bson_sha256": self.raw_bson_sha256,
            "pg_jsonb_sha256": self.pg_jsonb_sha256,
            "identity_hash": self.identity_hash,
            "rows": self.rows,
            "atttypid": self.atttypid,
            "atttypmod": self.atttypmod,
            "type_sql": self.type_sql,
            "type_114": self.type_114,
            "typmod_sqlstate": self.typmod_sqlstate,
            "document_text": self.document_text,
            "send_matches": self.send_matches,
            "stored_namespace": self.stored_namespace,
            "target_host": self.target_host,
            "target_database": self.target_database,
            "model": self.model,
        })
    }
}

/// Refusal. `rolled_back` is true only after a target transaction started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentImportError {
    pub code: String,
    pub message: &'static str,
    pub rolled_back: bool,
}

impl DocumentImportError {
    fn named(name: &'static str, message: &'static str, rolled_back: bool) -> Self {
        Self {
            code: format!("{PROTOCOL}/{name}"),
            message,
            rolled_back,
        }
    }

    fn forwarded(code: String, message: &'static str, rolled_back: bool) -> Self {
        Self {
            code,
            message,
            rolled_back,
        }
    }

    /// Refusal receipt. There is no Ready marker and no stored document text.
    pub fn to_json(&self) -> Value {
        json!({
            "protocol": PROTOCOL,
            "outcome": "refused",
            "ready": false,
            "partial_ready": false,
            "pg_source_passed": false,
            "rolled_back": self.rolled_back,
            "code": self.code,
            "rows": 0,
            "catalog_state": Value::Null,
            "model": "not_called",
        })
    }
}

/// Checked capture. `pg_bytes` is the sealed `jsonb_send` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDocument {
    pub source_engine: String,
    pub source_id: String,
    pub dataset: String,
    pub column: String,
    pub schema_digest: String,
    pub raw_bson_sha256: String,
    pub pg_jsonb_sha256: String,
    pub identity_hash: String,
    pub pg_text: String,
    pub pg_bytes: Vec<u8>,
    pub stored_namespace: String,
    pub stored_table: String,
    pub id_column: String,
    pub doc_column: String,
}

/// Validate one request. This does not open a database.
pub fn prepare_jsonb_document(request_json: &str) -> Result<PreparedDocument, DocumentImportError> {
    if request_json.is_empty() || request_json.len() > MAX_REQUEST {
        return Err(DocumentImportError::named(
            "schema.invalid",
            "document import request is outside the byte budget",
            false,
        ));
    }
    let request: Value = serde_json::from_str(request_json).map_err(|_| {
        DocumentImportError::named(
            "schema.invalid",
            "document import request is not JSON",
            false,
        )
    })?;
    let root = request.as_object().ok_or_else(|| {
        DocumentImportError::named(
            "schema.invalid",
            "document import request is not an object",
            false,
        )
    })?;
    if root.get("password").is_some() {
        return Err(DocumentImportError::named(
            "secret.rejected",
            "document import request carries a secret",
            false,
        ));
    }
    require_protocol(root)?;
    let engine = require_token(root, "engine")?;
    let source_id = require_token(root, "source_id")?;
    if engine != "mongodb" && engine != "mysql" {
        return Err(DocumentImportError::named(
            "cross_source",
            "document import engine is not mongodb or mysql",
            false,
        ));
    }
    let catalog_value = root.get("catalog").ok_or_else(|| {
        DocumentImportError::named("field.missing", "document import catalog is absent", false)
    })?;
    let document_value = root.get("document").ok_or_else(|| {
        DocumentImportError::named("field.missing", "document import envelope is absent", false)
    })?;
    if !document_value.is_object() {
        return Err(DocumentImportError::named(
            "schema.invalid",
            "document envelope is not an object",
            false,
        ));
    }
    refuse_fake_capture(catalog_value)?;
    let catalog_json = serde_json::to_string(catalog_value).map_err(|_| {
        DocumentImportError::named(
            "schema.invalid",
            "document import catalog is not JSON",
            false,
        )
    })?;
    let document_json = serde_json::to_string(document_value).map_err(|_| {
        DocumentImportError::named("schema.invalid", "document envelope is not JSON", false)
    })?;
    let catalog = sealed_import::parse_sealed_import_catalog(&catalog_json)
        .map_err(|err| DocumentImportError::forwarded(err.code, err.message, false))?;
    if catalog.engine != engine || catalog.source_id != source_id {
        return Err(DocumentImportError::named(
            "cross_source",
            "document import source does not match the sealed catalog",
            false,
        ));
    }
    let sealed = jsonb_doc::seal_bson_jsonb_document(&document_json)
        .map_err(|err| DocumentImportError::forwarded(err.code, err.message, false))?;
    if sealed.source_type.as_deref() != Some("jsonb") {
        return Err(DocumentImportError::named(
            "source_type.rejected",
            "document source type is not jsonb",
            false,
        ));
    }
    bind_catalog(&catalog, catalog_value, document_value, &sealed)
}

/// Import one prepared document. Seal failures return before a target session.
pub async fn import_jsonb_document(
    request_json: &str,
) -> Result<DocumentReceipt, DocumentImportError> {
    let prepared = prepare_jsonb_document(request_json)?;
    let target = read_target(request_json)?;
    load(&prepared, &target).await
}

/// Read one request from stdin and write one receipt to stdout.
pub async fn import_jsonb_document_cli() -> Result<(), u8> {
    let mut stdin = Vec::new();
    std::io::stdin().read_to_end(&mut stdin).map_err(|_| 2u8)?;
    if stdin.is_empty() || stdin.len() > MAX_REQUEST {
        eprintln!("oo-pipeline: usage");
        return Err(2);
    }
    let text = std::str::from_utf8(&stdin).map_err(|_| {
        eprintln!("oo-pipeline: usage");
        2u8
    })?;
    let value = match import_jsonb_document(text).await {
        Ok(receipt) => receipt.to_json(),
        Err(err) => {
            eprintln!("{}", err.code);
            write_stdout(&err.to_json())?;
            return Err(1);
        }
    };
    write_stdout(&value)?;
    Ok(())
}

fn write_stdout(value: &Value) -> Result<(), u8> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| 1u8)?;
    bytes.push(b'\n');
    std::io::stdout().write_all(&bytes).map_err(|_| 1u8)
}

struct Target {
    host: String,
    port: u16,
    database: String,
    user: String,
    password: String,
    ca_pem: Vec<u8>,
}

fn require_protocol(root: &Map<String, Value>) -> Result<(), DocumentImportError> {
    match root.get("protocol").and_then(Value::as_str) {
        Some(PROTOCOL) => Ok(()),
        Some(_) => Err(DocumentImportError::named(
            "protocol.mismatch",
            "document import protocol is not oo.pipeline.jsonb-document-import/v1",
            false,
        )),
        None => Err(DocumentImportError::named(
            "field.missing",
            "document import protocol is absent",
            false,
        )),
    }
}

fn require_token(root: &Map<String, Value>, key: &str) -> Result<String, DocumentImportError> {
    let value = root.get(key).and_then(Value::as_str).ok_or_else(|| {
        DocumentImportError::named(
            "field.missing",
            "document import source field is absent",
            false,
        )
    })?;
    if !closed_token(value) {
        return Err(DocumentImportError::named(
            "schema.invalid",
            "document import source field is not a closed token",
            false,
        ));
    }
    Ok(value.to_string())
}

fn closed_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn refuse_fake_capture(catalog: &Value) -> Result<(), DocumentImportError> {
    match catalog.get("capture").and_then(Value::as_str) {
        Some("sealed_import") => {}
        _ => {
            return Err(DocumentImportError::named(
                "fake_capture",
                "capture is not sealed_import",
                false,
            ));
        }
    }
    match catalog.get("state").and_then(Value::as_str) {
        Some("awaiting_publish") => Ok(()),
        Some("ready") | Some("closed") => Err(DocumentImportError::named(
            "fake_capture",
            "catalog arrived already ready",
            false,
        )),
        _ => Ok(()),
    }
}

fn bind_catalog(
    catalog: &SealedImportCatalog,
    catalog_value: &Value,
    document_value: &Value,
    sealed: &SealedJsonbDocument,
) -> Result<PreparedDocument, DocumentImportError> {
    let table = catalog
        .tables
        .iter()
        .find(|item| format!("{}.{}", item.logical_schema, item.logical_table) == sealed.dataset)
        .ok_or_else(|| {
            DocumentImportError::named(
                "cross_source",
                "document dataset is not in the sealed catalog",
                false,
            )
        })?;
    if table.accepted_rows != 1 {
        return Err(DocumentImportError::named(
            "schema.invalid",
            "document import accepts one sealed row",
            false,
        ));
    }
    let column = table
        .columns
        .iter()
        .find(|item| item.logical_name == sealed.column)
        .ok_or_else(|| {
            DocumentImportError::named(
                "column.absent",
                "document column is not in the sealed catalog",
                false,
            )
        })?;
    if column.target_type != "jsonb"
        || column.oid != jsonb_doc::JSONB_OID
        || column.typmod != jsonb_doc::JSONB_TYPMOD
        || column.source_type.as_deref() != Some("jsonb")
        || column.key
    {
        return Err(DocumentImportError::named(
            "type.ambiguous",
            "document column is not jsonb oid 3802 typmod -1",
            false,
        ));
    }
    let keys = table.columns.iter().filter(|item| item.key).count();
    let key = table.columns.iter().find(|item| item.key).ok_or_else(|| {
        DocumentImportError::named(
            "schema.invalid",
            "document import requires one int4 key",
            false,
        )
    })?;
    if keys != 1 || key.oid != 23 || key.typmod != -1 || key.target_type != "int4" {
        return Err(DocumentImportError::named(
            "schema.invalid",
            "document import requires one int4 key",
            false,
        ));
    }
    let raw = catalog_value
        .get("tables")
        .and_then(Value::as_array)
        .and_then(|tables| {
            tables.iter().find(|item| {
                item.get("logical_schema").and_then(Value::as_str)
                    == Some(table.logical_schema.as_str())
                    && item.get("logical_table").and_then(Value::as_str)
                        == Some(table.logical_table.as_str())
            })
        })
        .ok_or_else(|| {
            DocumentImportError::named(
                "schema.invalid",
                "document import catalog table is absent",
                false,
            )
        })?;
    let namespace = shadow_name(raw, "stored_namespace")?;
    let stored_table = shadow_name(raw, "stored_physical")?;
    let id_column = column_shadow(raw, &key.logical_name)?;
    let doc_column = column_shadow(raw, &column.logical_name)?;
    if namespace == stored_table || id_column == doc_column {
        return Err(DocumentImportError::named(
            "schema.invalid",
            "document import shadow names collide",
            false,
        ));
    }
    let pg_text = document_value
        .get("pg_jsonb_text")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            DocumentImportError::named("field.missing", "jsonb text is absent", false)
        })?;
    let pg_hex = document_value
        .get("pg_jsonb_hex")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            DocumentImportError::named("field.missing", "jsonb send hex is absent", false)
        })?;
    let pg_bytes = decode_hex(pg_hex)?;
    if pg_bytes.first().copied() != Some(jsonb_doc::JSONB_SEND_VERSION)
        || pg_bytes
            .get(1..)
            .is_none_or(|rest| rest != pg_text.as_bytes())
    {
        return Err(DocumentImportError::named(
            "digest.mismatch",
            "jsonb send bytes do not match the sealed text",
            false,
        ));
    }
    Ok(PreparedDocument {
        source_engine: catalog.engine.clone(),
        source_id: catalog.source_id.clone(),
        dataset: sealed.dataset.clone(),
        column: sealed.column.clone(),
        schema_digest: catalog.schema_digest.clone(),
        raw_bson_sha256: sealed.raw_bson_sha256.clone(),
        pg_jsonb_sha256: sealed.pg_jsonb_sha256.clone(),
        identity_hash: sealed.identity_hash.clone(),
        pg_text: pg_text.to_string(),
        pg_bytes,
        stored_namespace: namespace.to_string(),
        stored_table: stored_table.to_string(),
        id_column: id_column.to_string(),
        doc_column: doc_column.to_string(),
    })
}

fn shadow_name<'a>(value: &'a Value, key: &str) -> Result<&'a str, DocumentImportError> {
    let name = value.get(key).and_then(Value::as_str).ok_or_else(|| {
        DocumentImportError::named(
            "schema.invalid",
            "document import shadow name is absent",
            false,
        )
    })?;
    if quote_physical_name(name).is_err() {
        return Err(DocumentImportError::named(
            "schema.invalid",
            "document import shadow name is closed",
            false,
        ));
    }
    Ok(name)
}

fn column_shadow<'a>(table: &'a Value, logical: &str) -> Result<&'a str, DocumentImportError> {
    let columns = table
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            DocumentImportError::named(
                "schema.invalid",
                "document import catalog columns are absent",
                false,
            )
        })?;
    let column = columns
        .iter()
        .find(|item| item.get("logical_name").and_then(Value::as_str) == Some(logical))
        .ok_or_else(|| {
            DocumentImportError::named("column.absent", "document column shadow is absent", false)
        })?;
    shadow_name(column, "stored_physical")
}

fn decode_hex(text: &str) -> Result<Vec<u8>, DocumentImportError> {
    if text.is_empty()
        || !text.len().is_multiple_of(2)
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(DocumentImportError::named(
            "schema.invalid",
            "hex payload is not lowercase even-length hex",
            false,
        ));
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let pair = std::str::from_utf8(&bytes[index..index + 2]).unwrap_or("");
        let value = u8::from_str_radix(pair, 16).map_err(|_| {
            DocumentImportError::named(
                "schema.invalid",
                "hex payload is not lowercase even-length hex",
                false,
            )
        })?;
        out.push(value);
        index += 2;
    }
    Ok(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn read_target(request_json: &str) -> Result<Target, DocumentImportError> {
    let request: Value = serde_json::from_str(request_json).map_err(|_| {
        DocumentImportError::named(
            "schema.invalid",
            "document import request is not JSON",
            false,
        )
    })?;
    let target = request.get("target").ok_or_else(|| {
        DocumentImportError::named("field.missing", "document import target is absent", false)
    })?;
    if target.get("password").is_some() {
        return Err(DocumentImportError::named(
            "secret.rejected",
            "document import target carries a secret",
            false,
        ));
    }
    let host = target.get("host").and_then(Value::as_str).ok_or_else(|| {
        DocumentImportError::named(
            "field.missing",
            "document import target host is absent",
            false,
        )
    })?;
    if !closed_host(host) {
        return Err(DocumentImportError::named(
            "schema.invalid",
            "document import target host is closed",
            false,
        ));
    }
    let port = target
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| {
            DocumentImportError::named(
                "schema.invalid",
                "document import target port is closed",
                false,
            )
        })?;
    let database = target_token(target, "database")?;
    let user = target_token(target, "user")?;
    let password_file = target
        .get("password_file")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            DocumentImportError::named(
                "field.missing",
                "document import password file is absent",
                false,
            )
        })?;
    let ca_file = target
        .get("ca_file")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            DocumentImportError::named("field.missing", "document import CA file is absent", false)
        })?;
    let password_bytes = read_limited(password_file, MAX_SECRET)?;
    let mut password = String::from_utf8(password_bytes).map_err(|_| {
        DocumentImportError::named(
            "connect",
            "document import password file is not text",
            false,
        )
    })?;
    while password.ends_with('\n') || password.ends_with('\r') {
        password.pop();
    }
    if password.is_empty() {
        return Err(DocumentImportError::named(
            "connect",
            "document import password file is empty",
            false,
        ));
    }
    let ca_pem = read_limited(ca_file, MAX_CA)?;
    Ok(Target {
        host: host.to_string(),
        port,
        database,
        user,
        password,
        ca_pem,
    })
}

fn target_token(target: &Value, key: &str) -> Result<String, DocumentImportError> {
    let value = target.get(key).and_then(Value::as_str).ok_or_else(|| {
        DocumentImportError::named(
            "field.missing",
            "document import target field is absent",
            false,
        )
    })?;
    if !closed_token(value) {
        return Err(DocumentImportError::named(
            "schema.invalid",
            "document import target field is closed",
            false,
        ));
    }
    Ok(value.to_string())
}

fn closed_host(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=253).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'-' | b'_'))
}

fn read_limited(path: &str, max: u64) -> Result<Vec<u8>, DocumentImportError> {
    if !path.starts_with('/') || path.split('/').any(|part| part == "..") {
        return Err(DocumentImportError::named(
            "schema.invalid",
            "document import target path is closed",
            false,
        ));
    }
    let meta = fs::metadata(Path::new(path)).map_err(|_| {
        DocumentImportError::named("connect", "document import target file is absent", false)
    })?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > max {
        return Err(DocumentImportError::named(
            "connect",
            "document import target file is not a regular file",
            false,
        ));
    }
    fs::read(path).map_err(|_| {
        DocumentImportError::named(
            "connect",
            "document import target file could not be read",
            false,
        )
    })
}

async fn load(
    prepared: &PreparedDocument,
    target: &Target,
) -> Result<DocumentReceipt, DocumentImportError> {
    let options = PgConnectOptions::from_explicit()
        .host(&target.host)
        .port(target.port)
        .username(&target.user)
        .password(target.password.as_str())
        .database(&target.database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(target.ca_pem.clone())
        .disable_statement_logging();
    let mut conn = options.connect().await.map_err(|_| {
        DocumentImportError::named("connect", "document import target connection failed", false)
    })?;
    sqlx::query("SET statement_timeout = '30s'")
        .execute(&mut conn)
        .await
        .map_err(|_| database(false))?;
    let install = install_jsonb_document(&mut conn)
        .await
        .map_err(install_error)?;
    let typmod_sqlstate = probe_typmod(&mut conn).await?;
    let mut tx = conn.begin().await.map_err(|_| database(false))?;
    let type_sql = scalar(&mut tx, "SELECT _oo.full_import_type_sql(3802, -1)").await?;
    let type_114 = scalar(&mut tx, "SELECT _oo.full_import_type_sql(114, -1)").await?;
    let id_sql = scalar(&mut tx, "SELECT _oo.full_import_type_sql(23, -1)").await?;
    if type_sql != "jsonb" || type_114 != "json" || id_sql != "int4" {
        return abort(
            tx,
            "type.ambiguous",
            "full_import_type_sql did not return jsonb, json, and int4",
        )
        .await;
    }
    let namespace = quote_physical_name(&prepared.stored_namespace).map_err(|_| {
        DocumentImportError::named(
            "schema.invalid",
            "document import shadow name is closed",
            true,
        )
    })?;
    let table = quote_physical_name(&prepared.stored_table).map_err(|_| {
        DocumentImportError::named(
            "schema.invalid",
            "document import shadow name is closed",
            true,
        )
    })?;
    let id_column = quote_physical_name(&prepared.id_column).map_err(|_| {
        DocumentImportError::named(
            "schema.invalid",
            "document import shadow name is closed",
            true,
        )
    })?;
    let doc_column = quote_physical_name(&prepared.doc_column).map_err(|_| {
        DocumentImportError::named(
            "schema.invalid",
            "document import shadow name is closed",
            true,
        )
    })?;
    if exec(&mut tx, "SET LOCAL ROLE oo_target_owner")
        .await
        .is_err()
    {
        return abort(
            tx,
            "privilege",
            "the document schema owner could not be assumed",
        )
        .await;
    }
    if exec(&mut tx, &format!("CREATE SCHEMA IF NOT EXISTS {namespace}"))
        .await
        .is_err()
    {
        return abort(tx, "database", "document schema was not created").await;
    }
    if exec(
        &mut tx,
        &format!(
            "CREATE TABLE {namespace}.{table} ({id_column} int4 NOT NULL, {doc_column} jsonb NOT NULL)"
        ),
    )
    .await
    .is_err()
    {
        return abort(tx, "target", "document table was not created").await;
    }
    let statement = format!(
        "COPY {namespace}.{table} ({id_column}, {doc_column}) FROM STDIN WITH (FORMAT binary)"
    );
    let bytes = copy_bytes(1, &prepared.pg_bytes)?;
    let copied = copy_in(&mut tx, &statement, &bytes).await?;
    if copied != 1 {
        return abort(tx, "schema.invalid", "document copy did not write one row").await;
    }
    let stored_text = scalar(
        &mut tx,
        &format!("SELECT {doc_column}::text FROM {namespace}.{table}"),
    )
    .await?;
    let stored_send = scalar(
        &mut tx,
        &format!(
            "SELECT encode(pg_catalog.jsonb_send({doc_column}), 'hex') FROM {namespace}.{table}"
        ),
    )
    .await?;
    let rows: i64 = scalar_i64(
        &mut tx,
        &format!("SELECT count(*)::int8 FROM {namespace}.{table}"),
    )
    .await?;
    let (atttypid, atttypmod) = column_type(&mut tx, prepared).await?;
    if stored_text != prepared.pg_text {
        return abort(
            tx,
            "pg.text_mismatch",
            "stored jsonb text does not match the sealed file",
        )
        .await;
    }
    if stored_send != hex_encode(&prepared.pg_bytes)
        || rows != 1
        || atttypid != jsonb_doc::JSONB_OID
        || atttypmod != jsonb_doc::JSONB_TYPMOD
    {
        return abort(
            tx,
            "pg.mismatch",
            "stored jsonb send does not match the sealed file",
        )
        .await;
    }
    tx.commit().await.map_err(|_| database(true))?;
    let install_name = match install {
        JsonbDocumentInstallOutcome::Installed => "installed",
        JsonbDocumentInstallOutcome::Replay => "replay",
    };
    Ok(DocumentReceipt {
        protocol: PROTOCOL,
        outcome: "imported",
        ready: false,
        partial_ready: false,
        pg_source_passed: false,
        rolled_back: false,
        code: None,
        install: install_name.to_string(),
        source_engine: prepared.source_engine.clone(),
        source_id: prepared.source_id.clone(),
        dataset: prepared.dataset.clone(),
        column: prepared.column.clone(),
        catalog_state: "awaiting_publish",
        schema_digest: prepared.schema_digest.clone(),
        raw_bson_sha256: prepared.raw_bson_sha256.clone(),
        pg_jsonb_sha256: prepared.pg_jsonb_sha256.clone(),
        identity_hash: prepared.identity_hash.clone(),
        rows: 1,
        atttypid,
        atttypmod,
        type_sql,
        type_114,
        typmod_sqlstate,
        document_text: stored_text,
        send_matches: true,
        stored_namespace: prepared.stored_namespace.clone(),
        target_host: target.host.clone(),
        target_database: target.database.clone(),
        model: "not_called",
    })
}

fn database(rolled_back: bool) -> DocumentImportError {
    DocumentImportError::named(
        "database",
        "the document import target rejected a statement",
        rolled_back,
    )
}

fn install_error(err: InstallError) -> DocumentImportError {
    match err {
        InstallError::NotAdministrator => DocumentImportError::named(
            "privilege",
            "the session cannot install the jsonb document type",
            false,
        ),
        InstallError::ChecksumMismatch { .. } => {
            DocumentImportError::named("checksum", "a stored migration checksum differs", true)
        }
        InstallError::MissingRelation { .. } | InstallError::Refused { .. } => {
            DocumentImportError::named(
                "install_refused",
                "jsonb document import did not install 0009",
                true,
            )
        }
        InstallError::Database { .. } => {
            DocumentImportError::named("database", "the jsonb document install was rejected", true)
        }
    }
}

async fn probe_typmod(conn: &mut PgConnection) -> Result<String, DocumentImportError> {
    match sqlx::query("SELECT _oo.full_import_type_sql(3802, 0)")
        .fetch_one(&mut *conn)
        .await
    {
        Ok(_) => Err(DocumentImportError::named(
            "typmod.rejected",
            "closed jsonb typmod was accepted",
            false,
        )),
        Err(err) => {
            let state = err
                .as_database_error()
                .and_then(|db| db.code().map(|code| code.to_string()))
                .unwrap_or_default();
            if state == "22023" {
                Ok(state)
            } else {
                Err(DocumentImportError::named(
                    "typmod.rejected",
                    "closed jsonb typmod was not rejected",
                    false,
                ))
            }
        }
    }
}

async fn abort(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    name: &'static str,
    message: &'static str,
) -> Result<DocumentReceipt, DocumentImportError> {
    let _ = tx.rollback().await;
    Err(DocumentImportError::named(name, message, true))
}

async fn exec(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    sql: &str,
) -> Result<(), DocumentImportError> {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut **tx)
        .await
        .map_err(|_| database(true))?;
    Ok(())
}

async fn scalar(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    sql: &str,
) -> Result<String, DocumentImportError> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| database(true))
}

async fn scalar_i64(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    sql: &str,
) -> Result<i64, DocumentImportError> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| database(true))
}

async fn column_type(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    prepared: &PreparedDocument,
) -> Result<(i64, i32), DocumentImportError> {
    let row = sqlx::query(
        "SELECT atttypid::int8, atttypmod::int4 FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relname = $2 AND a.attname = $3 AND a.attnum > 0 AND NOT a.attisdropped",
    )
    .bind(&prepared.stored_namespace)
    .bind(&prepared.stored_table)
    .bind(&prepared.doc_column)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| database(true))?;
    let oid: i64 = row.try_get(0).map_err(|_| database(true))?;
    let typmod: i32 = row.try_get(1).map_err(|_| database(true))?;
    Ok((oid, typmod))
}

async fn copy_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    statement: &str,
    bytes: &[u8],
) -> Result<u64, DocumentImportError> {
    let mut copy = match tx.copy_in_raw(statement).await {
        Ok(copy) => copy,
        Err(_) => return Err(database(true)),
    };
    if copy.send(bytes).await.is_err() {
        let _ = copy.abort("copy send failed").await;
        return Err(database(true));
    }
    copy.finish().await.map_err(|_| database(true))
}

fn copy_bytes(id: i32, pg: &[u8]) -> Result<Vec<u8>, DocumentImportError> {
    let doc_len = i32::try_from(pg.len()).map_err(|_| {
        DocumentImportError::named(
            "schema.invalid",
            "jsonb send is outside the copy budget",
            false,
        )
    })?;
    let mut out = Vec::with_capacity(19 + 6 + 4 + pg.len() + 2);
    out.extend_from_slice(b"PGCOPY\n\xff\r\n\0");
    out.extend_from_slice(&0i32.to_be_bytes());
    out.extend_from_slice(&0i32.to_be_bytes());
    out.extend_from_slice(&2i16.to_be_bytes());
    out.extend_from_slice(&4i32.to_be_bytes());
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&doc_len.to_be_bytes());
    out.extend_from_slice(pg);
    out.extend_from_slice(&(-1i16).to_be_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    use super::hex_encode;
    use crate::physical_mapping::quote_physical_name;
    use oo_core::api::payload_sha256;
    use oo_core::sealed_import::{push_schema_column, push_schema_table};
    use serde_json::{Value, json};
    use sqlx::postgres::{PgConnectOptions, PgSslMode};
    use sqlx::{ConnectOptions, PgConnection, Row};

    const RAW: &[u8] = include_bytes!("../../oo-core/tests/fixtures/jsonb3802/canonical-bson.bin");
    const PG: &[u8] = include_bytes!("../../oo-core/tests/fixtures/jsonb3802/canonical.bin");

    #[test]
    fn canonical_document_prepares_without_a_database() {
        let prepared = prepare_jsonb_document(&success_request("mongodb", "mongo_docs", "a1"))
            .expect("prepare");
        assert_eq!(prepared.dataset, "public.docs");
        assert_eq!(prepared.column, "doc");
        assert_eq!(prepared.source_engine, "mongodb");
        assert_eq!(prepared.raw_bson_sha256, payload_sha256(RAW));
        assert_eq!(prepared.pg_jsonb_sha256, payload_sha256(PG));
        assert_eq!(prepared.pg_bytes, PG);
        assert_eq!(prepared.pg_text.as_bytes(), &PG[1..]);
        assert!(prepared.pg_text.contains("9223372036854775807"));
        assert!(prepared.pg_text.contains("1.2300"));
        assert!(!prepared.pg_text.contains("gone"));
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires OO_JSONB_DOC_HOST and the preview database"]
    async fn jsonb_document_imports_canonical_and_rolls_back() {
        let mut live =
            serde_json::from_str::<Value>(&success_request("mongodb", "mongo_docs", "a1"))
                .expect("request");
        live["target"] = json!({
            "host": env::var("OO_JSONB_DOC_HOST").expect("host"),
            "port": env::var("OO_JSONB_DOC_PORT").expect("port").parse::<u16>().expect("port"),
            "database": env::var("OO_JSONB_DOC_DB").expect("db"),
            "user": env::var("OO_JSONB_DOC_USER").expect("user"),
            "password_file": env::var("OO_JSONB_DOC_PASSWORD_FILE").expect("password file"),
            "ca_file": env::var("OO_JSONB_DOC_CA").expect("ca"),
        });
        let text = live.to_string();
        if let Ok(path) = env::var("OO_JSONB_DOC_REQUEST") {
            fs::write(path, &text).expect("request file");
        }
        let receipt = import_jsonb_document(&text).await.expect("import");
        assert!(!receipt.ready);
        assert!(!receipt.partial_ready);
        assert!(!receipt.pg_source_passed);
        assert!(!receipt.rolled_back);
        assert_eq!(receipt.outcome, "imported");
        assert_eq!(receipt.catalog_state, "awaiting_publish");
        assert_eq!(receipt.rows, 1);
        assert_eq!(receipt.atttypid, 3802);
        assert_eq!(receipt.atttypmod, -1);
        assert_eq!(receipt.type_sql, "jsonb");
        assert_eq!(receipt.type_114, "json");
        assert_eq!(receipt.typmod_sqlstate, "22023");
        assert!(receipt.send_matches);
        assert_eq!(receipt.document_text.as_bytes(), &PG[1..]);
        assert_eq!(receipt.model, "not_called");
        assert_eq!(receipt.raw_bson_sha256, payload_sha256(RAW));
        let again = import_jsonb_document(&text).await.expect_err("second");
        assert!(again.rolled_back);
        assert!(again.code.ends_with("/target"), "{}", again.code);
        assert!(!again.to_json()["ready"].as_bool().unwrap());
        let mut conn = connect().await;
        let namespace = shadow("pn_", "a1", 0);
        let table = shadow("pt_", "a1", 1);
        let column = shadow("pc_", "a1", 3);
        let quoted_ns = quote_physical_name(&namespace).expect("ns");
        let quoted_table = quote_physical_name(&table).expect("table");
        let quoted_column = quote_physical_name(&column).expect("column");
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*)::int8 FROM {quoted_ns}.{quoted_table}"
        )))
        .fetch_one(&mut conn)
        .await
        .expect("count");
        assert_eq!(count, 1);
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT ({quoted_column} ? 'gone')::text, jsonb_typeof({quoted_column}->'note'), {quoted_column} #>> '{{d,$numberDecimal}}', {quoted_column} #>> '{{negd,$numberDecimal}}', {quoted_column} #>> '{{when,$date,$numberLong}}', {quoted_column} #>> '{{b,$binary,base64}}', {quoted_column} #>> '{{b,$binary,subType}}', {quoted_column} #>> '{{u,$uuid}}', {quoted_column} #>> '{{big,$numberLong}}', {quoted_column} #>> '{{_id,$oid}}', {quoted_column} #>> '{{title}}' FROM {quoted_ns}.{quoted_table}"
        )))
        .fetch_one(&mut conn)
        .await
        .expect("fields");
        let fields: Vec<String> = (0..11)
            .map(|index| row.try_get(index).expect("field"))
            .collect();
        assert_eq!(fields[0], "false");
        assert_eq!(fields[1], "null");
        assert_eq!(fields[2], "1.2300");
        assert_eq!(fields[3], "-1.50");
        assert_eq!(fields[4], "-61000");
        assert_eq!(fields[5], "AA==");
        assert_eq!(fields[6], "00");
        assert_eq!(fields[7], "00112233-4455-4677-8899-aabbccddeeff");
        assert_eq!(fields[8], "9223372036854775807");
        assert_eq!(fields[9], "507f1f77bcf86cd799439011");
        assert_eq!(fields[10], "雪");
        if let Ok(path) = env::var("OO_JSONB_DOC_OUT") {
            let mut body = receipt.to_json();
            body["local_doc"] = json!({
                "gone": fields[0],
                "note": fields[1],
                "decimal": fields[2],
                "neg_decimal": fields[3],
                "date": fields[4],
                "binary_base64": fields[5],
                "binary_subtype": fields[6],
                "uuid": fields[7],
                "int64": fields[8],
                "oid": fields[9],
                "title": fields[10],
                "second_import": again.code,
                "second_rolled_back": again.rolled_back,
            });
            fs::write(path, serde_json::to_vec_pretty(&body).expect("json")).expect("out");
        }
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {quoted_ns} CASCADE"
        )))
        .execute(&mut conn)
        .await
        .expect("drop");
        let mut refused = serde_json::from_str::<Value>(&request(
            "mongodb",
            "mongo_docs",
            &docs_catalog("mongodb", "mongo_docs", 3802, -1, Some("jsonb"), "e5"),
            &{
                let mut document = envelope();
                document["oid"] = json!(114);
                document
            },
        ))
        .expect("refused request");
        refused["target"] = live["target"].clone();
        let err = import_jsonb_document(&refused.to_string())
            .await
            .expect_err("114");
        assert!(err.code.ends_with("/json114_not_jsonb"), "{}", err.code);
        assert!(!err.rolled_back);
        let absent: Option<i32> =
            sqlx::query_scalar("SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = $1")
                .bind(shadow("pn_", "e5", 0))
                .fetch_optional(&mut conn)
                .await
                .expect("namespace");
        assert!(absent.is_none());
    }

    async fn connect() -> PgConnection {
        let password =
            fs::read_to_string(env::var("OO_JSONB_DOC_PASSWORD_FILE").expect("password"))
                .expect("password")
                .trim_end_matches(['\n', '\r'])
                .to_string();
        let ca = fs::read(env::var("OO_JSONB_DOC_CA").expect("ca")).expect("ca");
        let options = PgConnectOptions::from_explicit()
            .host(&env::var("OO_JSONB_DOC_HOST").expect("host"))
            .port(
                env::var("OO_JSONB_DOC_PORT")
                    .expect("port")
                    .parse()
                    .expect("port"),
            )
            .username(&env::var("OO_JSONB_DOC_USER").expect("user"))
            .password(password.as_str())
            .database(&env::var("OO_JSONB_DOC_DB").expect("db"))
            .ssl_mode(PgSslMode::VerifyFull)
            .ssl_root_cert_from_pem(ca)
            .disable_statement_logging();
        options.connect().await.expect("connect")
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires OO_JSONB_DOC_HOST and the preview database"]
    async fn jsonb_document_drops_dedicated_namespace() {
        let mut conn = connect().await;
        let namespace = shadow("pn_", "a1", 0);
        let quoted = quote_physical_name(&namespace).expect("ns");
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {quoted} CASCADE"
        )))
        .execute(&mut conn)
        .await
        .expect("drop");
    }

    #[test]
    fn seal_and_capture_refusals_do_not_roll_back() {
        assert_refused(
            &mutated(|document, _| document["oid"] = json!(114)),
            "/json114_not_jsonb",
        );
        assert_refused(
            &mutated(|document, _| document["typmod"] = json!(0)),
            "/typmod.rejected",
        );
        assert_refused(
            &mutated(|document, _| {
                document["raw_bson_dropped"] = json!(true);
                document["raw_bson_hex"] = json!("");
            }),
            "/raw_bson_dropped",
        );
        assert_refused(
            &mutated(|document, _| {
                let mut hash = document["raw_bson_sha256"].as_str().unwrap().to_string();
                let flipped = if hash.ends_with('0') { '1' } else { '0' };
                hash.pop();
                hash.push(flipped);
                document["raw_bson_sha256"] = json!(hash);
            }),
            "/digest.mismatch",
        );
        assert_refused(
            &mutated(|document, _| document["missing_is_null"] = json!(true)),
            "/missing_is_null",
        );
        assert_refused(
            &mutated(|document, _| document["loss_concealed"] = json!(true)),
            "/loss_concealed",
        );
        assert_refused(
            &mutated(|document, _| {
                document["pg_jsonb_text"] = json!("not-json");
                document["pg_jsonb_hex"] = json!(hex_encode(&[
                    1, b'n', b'o', b't', b'-', b'j', b's', b'o', b'n'
                ]));
            }),
            "/pg.truncated",
        );
        assert_refused(
            &mutated(|document, _| {
                document.as_object_mut().unwrap().remove("source_type");
            }),
            "/source_type.rejected",
        );
        assert_refused(
            &mutated(|_, catalog| catalog["capture"] = json!("copy_probe")),
            "/fake_capture",
        );
        assert_refused(
            &mutated(|_, catalog| catalog["state"] = json!("ready")),
            "/fake_capture",
        );
        assert_refused(
            &request(
                "mysql",
                "mongo_docs",
                &docs_catalog("mongodb", "mongo_docs", 3802, -1, Some("jsonb"), "a1"),
                &envelope(),
            ),
            "/cross_source",
        );
        assert_refused(
            &request(
                "postgresql",
                "mongo_docs",
                &docs_catalog("postgresql", "mongo_docs", 3802, -1, Some("jsonb"), "a1"),
                &envelope(),
            ),
            "/cross_source",
        );
        assert_refused(
            &request(
                "mongodb",
                "mongo_docs",
                &docs_catalog("mongodb", "mongo_docs", 114, -1, Some("json"), "b2"),
                &envelope(),
            ),
            "/type.ambiguous",
        );
    }

    fn assert_refused(request_json: &str, suffix: &str) {
        let err = prepare_jsonb_document(request_json).expect_err(suffix);
        assert!(err.code.ends_with(suffix), "{}", err.code);
        assert!(!err.rolled_back);
        let body = err.to_json();
        assert_eq!(body["ready"], false);
        assert_eq!(body["partial_ready"], false);
        assert_eq!(body["rows"], 0);
        assert!(body["catalog_state"].is_null());
    }

    fn success_request(engine: &str, source_id: &str, mark: &str) -> String {
        request(
            engine,
            source_id,
            &docs_catalog(engine, source_id, 3802, -1, Some("jsonb"), mark),
            &envelope(),
        )
    }

    fn mutated(edit: impl FnOnce(&mut Value, &mut Value)) -> String {
        let mut document = envelope();
        let mut catalog = docs_catalog("mongodb", "mongo_docs", 3802, -1, Some("jsonb"), "a1");
        edit(&mut document, &mut catalog);
        request("mongodb", "mongo_docs", &catalog, &document)
    }

    fn request(engine: &str, source_id: &str, catalog: &Value, document: &Value) -> String {
        json!({
            "protocol": PROTOCOL,
            "engine": engine,
            "source_id": source_id,
            "catalog": catalog,
            "document": document,
        })
        .to_string()
    }

    fn envelope() -> Value {
        let text = std::str::from_utf8(&PG[1..]).expect("jsonb text");
        json!({
            "protocol": "oo.document.bson-jsonb/v1",
            "dataset": "public.docs",
            "column": "doc",
            "oid": 3802,
            "typmod": -1,
            "source_type": "jsonb",
            "raw_bson_dropped": false,
            "raw_bson_hex": hex_encode(RAW),
            "raw_bson_sha256": payload_sha256(RAW),
            "pg_jsonb_hex": hex_encode(PG),
            "pg_jsonb_text": text,
            "loss_concealed": false,
            "missing_is_null": false
        })
    }

    fn docs_catalog(
        engine: &str,
        source_id: &str,
        doc_oid: i64,
        doc_typmod: i32,
        source_type: Option<&str>,
        mark: &str,
    ) -> Value {
        let mut preimage = String::new();
        push_schema_table(&mut preimage, "public", "docs");
        push_schema_column(&mut preimage, "id", 23, -1, None);
        push_schema_column(&mut preimage, "doc", doc_oid, doc_typmod, None);
        let schema_digest = payload_sha256(preimage.as_bytes());
        let digest = "a".repeat(64);
        let namespace = shadow("pn_", mark, 0);
        let table = shadow("pt_", mark, 1);
        let id_shadow = shadow("pc_", mark, 2);
        let doc_shadow = shadow("pc_", mark, 3);
        json!({
            "capture": "sealed_import",
            "config_digest": digest,
            "engine": engine,
            "hook_checked": false,
            "input_digest": digest,
            "protocol": "oo.pipeline.sealed-import-catalog/v1",
            "quality": {
                "loss_concealed": false,
                "manifest_reject_count": 0,
                "payload_includes_reject_lines": false,
                "stored_omits_manifest_rejects": false,
                "stored_rejected_rows": 0,
                "stored_rows": [{
                    "accepted_rows": 1,
                    "attnum": null,
                    "batch_id": "t0.b0",
                    "key_digest": null,
                    "kind": "batch",
                    "physical_name": table,
                    "reason": null,
                    "rejected_rows": 0,
                    "row_index": null,
                    "value_len": null
                }]
            },
            "run_id": "beecbbc7-8dda-4779-81ba-6ff0383f72ec",
            "schema_digest": schema_digest,
            "shadows_queried": false,
            "source_generation": 1,
            "source_id": source_id,
            "source_introspection": "not_used",
            "state": "awaiting_publish",
            "tables": [{
                "accepted_rows": 1,
                "canonical_digest": digest,
                "columns": [
                    {
                        "caveat": null,
                        "key": true,
                        "logical_name": "id",
                        "oid": 23,
                        "source_type": null,
                        "stored_physical": id_shadow,
                        "typmod": -1
                    },
                    {
                        "caveat": null,
                        "key": false,
                        "logical_name": "doc",
                        "oid": doc_oid,
                        "source_type": source_type,
                        "stored_physical": doc_shadow,
                        "typmod": doc_typmod
                    }
                ],
                "logical_schema": "public",
                "logical_table": "docs",
                "rejects": [],
                "stored_namespace": namespace,
                "stored_physical": table
            }],
            "target_generation": 1,
            "tool_hook_digest": digest,
            "type_digest": digest
        })
    }

    fn shadow(prefix: &str, mark: &str, salt: u8) -> String {
        let hex = format!("{mark}{salt:02x}");
        format!(
            "{prefix}{}",
            hex.chars().cycle().take(60).collect::<String>()
        )
    }
}
