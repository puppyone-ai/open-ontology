//! Private capture-binding registry.
//!
//! `oo.source-admission.decision/v1` stays unchanged and does not name Arrow
//! batches. The operator registry is outside the data manifest. Its digest and
//! the frozen `oo-source-admission` binary are process environment, never
//! fields a public client can set. This module does not mint `admitted`.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

/// Where the operator binding is read from.
///
/// `Process` is the import command. `Absent` is the historical empty-pin path.
/// `Fixed` is the same three values a process environment would supply. The
/// manifest cannot construct this value.
#[derive(Clone, Debug)]
pub enum OperatorEnv {
    /// Read `OO_NATIVE_ADMISSION_BIN` and the two registry variables.
    Process,
    /// Ignore those variables and keep the empty-pin path.
    Absent,
    /// Explicit operator values. Missing parts are the partial-env refusal.
    Fixed {
        /// Absolute path of the frozen validator, when the operator set one.
        bin: Option<String>,
        /// Absolute path of the binding registry, when the operator set one.
        registry: Option<String>,
        /// Expected SHA-256 of that registry file.
        registry_sha256: Option<String>,
    },
}

thread_local! {
    static OPERATOR: RefCell<OperatorEnv> = const { RefCell::new(OperatorEnv::Process) };
}

/// Run `body` with one operator source. Restores the previous source afterward.
pub fn with_operator_env<T>(operator: OperatorEnv, body: impl FnOnce() -> T) -> T {
    OPERATOR.with(|slot| {
        let previous = slot.replace(operator);
        let restore = OperatorRestore { slot, previous };
        let result = body();
        drop(restore);
        result
    })
}

struct OperatorRestore<'a> {
    slot: &'a RefCell<OperatorEnv>,
    previous: OperatorEnv,
}

impl Drop for OperatorRestore<'_> {
    fn drop(&mut self) {
        self.slot.replace(self.previous.clone());
    }
}

fn operator_values() -> (Option<String>, Option<String>, Option<String>) {
    let mode = OPERATOR.with(|slot| slot.borrow().clone());
    match mode {
        OperatorEnv::Absent => (None, None, None),
        OperatorEnv::Fixed {
            bin,
            registry,
            registry_sha256,
        } => (bin, registry, registry_sha256),
        OperatorEnv::Process => (
            std::env::var(ENV_BIN).ok(),
            std::env::var(ENV_REGISTRY).ok(),
            std::env::var(ENV_REGISTRY_SHA).ok(),
        ),
    }
}

/// Read the operator's immutable occurrence registry, never a caller path.
pub(crate) fn read_operator_registry() -> Result<(String, Vec<u8>), &'static str> {
    let (bin, path, digest) = operator_values();
    let (Some(bin), Some(path), Some(digest)) = (bin, path, digest) else { return Err("nonadmitted"); };
    if !bin.starts_with('/') || !path.starts_with('/') || !hex64(&digest) { return Err("nonadmitted"); }
    let bytes = read_capped(Path::new(&path))?;
    if hex_bytes(&Sha256::digest(&bytes)) != digest { return Err("digest_mismatch"); }
    Ok((path, bytes))
}

/// Capture binding produced from a real capture and a real admission run.
pub const CAPTURE_BINDING_PROTOCOL: &str = "oo.source-admission.capture-binding/v1";
/// Operator registry of capture bindings. Not a decision document.
pub const BINDING_REGISTRY_PROTOCOL: &str = "oo.source-admission.binding-registry/v1";
/// Source-bound capture receipt. The file hash is the binding field, not a global allowlist.
pub const CAPTURE_RECEIPT_PROTOCOL: &str = "oo.source-admission.capture-receipt/v1";
/// SHA-256 of the frozen clickhouse/opensearch completion receipt.
pub const HISTORICAL_ADMISSION_RECEIPT_SHA256: &str =
    "65b7f22fe26686f8a1e61d705a78d5a1a69f2a123de0b9dfce827cce2ffb59ef";
const HISTORICAL_RECEIPT_SCHEMA: &str = "oo.native-readonly-completion/receipt/v1";
const HISTORICAL_CONTENT_COMMIT: &str = "5df40d24eefa4fbcfa00c4415cfcfde5d368db89";
/// SHA-256 of the frozen `5df40d24` `oo-source-admission` binary.
pub const ADMISSION_BIN_SHA256: &str =
    "a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2";
/// SHA-256 of the frozen `3c55ea8b` `oo-source-admission` binary.
pub const ADMISSION_BIN_SHA256_3C55: &str =
    "ae7dc727181ae101bab84e328d39c38c3fdd6613255f79062404cbc9c8069577";
/// SHA-256 of the `cbc88efdc` / `b0df708e4` `oo-source-admission` binary.
///
/// That validator's public decision has `capture_methods` and no `secret_ref`.
/// The registry preimage stays the one-way stamp in [`registry_preimage`].
pub const ADMISSION_BIN_SHA256_E465: &str =
    "e465241fc1292b6eb5a13cf12efbffe360a9fbf1cbb03c02affb0241a4e58e1f";
/// Linux Rust 1.94 validator with original Date/List and source parity support.
pub const ADMISSION_BIN_SHA256_NATIVE_20261012: &str =
    "0e18dec38d41d04b00650c613902e4ff21ffab700795208916679699718ca90f";
/// Successor also admits original Mongo, SQL Server and PostgreSQL page schemas.
pub const ADMISSION_BIN_SHA256_NATIVE_FINAL2: &str =
    "f3353f1d13e68990693818660cb9eec180961dde66dd39f84d94ef7d80366081";
/// Narrow DBA-owned existing-object R1 capture validator, Linux Rust 1.94.
pub const ADMISSION_BIN_SHA256_NATIVE_R1: &str =
    "1628c96da4bb3e71f17ab3fba4385c27d810e5cac89e424f6e777bc69dcbcfc0";
/// Original default scale/empty batch and narrowly bound R1 successor.
pub const ADMISSION_BIN_SHA256_NATIVE_43BDE: &str =
    "43bde81c78726c9c3aed6bbb9e2d5eaf0a34f3b45d00fb861886f7841d161523";
/// Fixed observed PostgreSQL source OID metadata and original native captures.
pub const ADMISSION_BIN_SHA256_NATIVE_MINIMAL: &str =
    "0b584453877a4c5ec3e7c311c199ade8af723229ea2ab56745858e37f7a1e274";
/// Sealed ClickHouse partition scope and BSON-tag binding validator, Linux 1.94.
pub const ADMISSION_BIN_SHA256_C4_SCOPE: &str =
    "c20a577b2e718b6d766af78dd94a029f08dbcdd545b8b7460ce71aa34cafabaf";
/// Sealed MongoDB raw BSON tag replay validator, Linux 1.94.
pub const ADMISSION_BIN_SHA256_MONGO_BSON: &str =
    "dfbf38a73c047567cc7183fcc47932874dc7f531bb691054ebd7551b80bbd805";
/// Sealed Arrow Duration microsecond validator, Linux Rust 1.94.
pub const ADMISSION_BIN_SHA256_DURATION_MICROS: &str =
    "cfc11a13d0ff14e377e29b687dbb60adb4ced05476c48143fed1637b841bab67";
/// Source-bound Doris/StarRocks OLAP C4 partition scope, Linux Rust 1.94.
pub const ADMISSION_BIN_SHA256_OLAP_C4: &str =
    "85fb10d91fa169ad85dc7b7c4116a6354614fcd69e20c010b67b2536f6170d85";
/// Sealed PostgreSQL multi-table transaction scope, Linux Rust 1.94.
pub const ADMISSION_BIN_SHA256_PG_TRANSACTION: &str =
    "9ae6dd2808576a17f45e76f1b3577291f758f29cbf31698f1338e8ace4979473";
/// Exact S1 per-table PostgreSQL capture alias validator, Linux Rust 1.94.
pub const ADMISSION_BIN_SHA256_PG_PER_TABLE: &str =
    "07e7651a98b4ef76582f67666e1245065546db204a50899554981619efc05dbf";

/// Trusted runtime allowlist. A manifest field cannot add a program.
pub fn approved_admission_bin(digest: &str) -> bool {
    digest == ADMISSION_BIN_SHA256
        || digest == ADMISSION_BIN_SHA256_3C55
        || digest == ADMISSION_BIN_SHA256_E465
        || digest == ADMISSION_BIN_SHA256_NATIVE_20261012
        || digest == ADMISSION_BIN_SHA256_NATIVE_FINAL2
        || digest == ADMISSION_BIN_SHA256_NATIVE_R1
        || digest == ADMISSION_BIN_SHA256_NATIVE_43BDE
        || digest == ADMISSION_BIN_SHA256_NATIVE_MINIMAL
        || digest == ADMISSION_BIN_SHA256_C4_SCOPE
        || digest == ADMISSION_BIN_SHA256_MONGO_BSON
        || digest == ADMISSION_BIN_SHA256_DURATION_MICROS
        || digest == ADMISSION_BIN_SHA256_OLAP_C4
        || digest == ADMISSION_BIN_SHA256_PG_TRANSACTION
        || digest == ADMISSION_BIN_SHA256_PG_PER_TABLE
}

const ENV_BIN: &str = "OO_NATIVE_ADMISSION_BIN";
const ENV_REGISTRY: &str = "OO_NATIVE_BINDING_REGISTRY";
const ENV_REGISTRY_SHA: &str = "OO_NATIVE_BINDING_REGISTRY_SHA256";
const DECISION_PROTOCOL: &str = "oo.source-admission.decision/v1";
const REPORT_PROTOCOL: &str = "oo.source-admission.report/v1";
pub const CATALOG_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";
/// Registry preimage uses this stamp. The sealed file replaces it with the receipt hash.
pub const REGISTRY_STAMP_BLANK: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";
const MAX_JSON: usize = 1024 * 1024;

/// One selected Arrow object named by a capture binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundObject {
    /// Logical namespace.
    pub logical_schema: String,
    /// Logical table.
    pub logical_table: String,
    /// Captured rows.
    pub row_count: u64,
    /// SHA-256 of the Arrow stream.
    pub arrow_sha256: String,
}

/// One field the binding requires the stream to contain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundField {
    /// Arrow field name.
    pub name: String,
    /// Closed arrow type token.
    pub arrow_type: String,
    /// Flatbuffer nullable bit.
    pub nullable: bool,
    /// Decimal precision or integer bit width, when the type has one.
    pub precision: Option<i32>,
    /// Decimal scale.
    pub scale: Option<i32>,
    /// Timestamp timezone.
    pub timezone: Option<String>,
    /// CHAR(n) width when the capture binding records one. Absent is not varchar.
    pub char_length: Option<i32>,
    /// Explicit binding key. Absent is false. A table name is not a key.
    pub key: bool,
    /// Utf8 full document stored as jsonb oid 3802 typmod -1. Absent is false.
    pub document: bool,
    /// Original PostgreSQL metadata, bound by the capture validator.
    pub source_oid: Option<u32>,
    /// Original typmod; present together with OID and format.
    pub source_typmod: Option<i32>,
    /// Observed format_type, never used as SQL text.
    pub source_format: Option<String>,
    /// Raw BSON type from the admitted Mongo schema/type preimage.
    pub bson_type: Option<BsonType>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BsonType {
    pub variant: String,
    pub subtype: Option<String>,
}

/// A registry entry that matched the manifest and the recomputed decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedCapture {
    /// SHA-256 of the frozen validator stdout.
    pub decision_sha256: String,
    /// `identity.source_id` from that decision.
    pub source_id: String,
    /// Arrow digests the binding names. These are not decision fields.
    pub batches: Vec<String>,
    /// SHA-256 of the operator registry file.
    pub registry_sha256: String,
    /// Driver digest from the admission report.
    pub driver_digest: String,
    /// Selected objects.
    pub objects: Vec<BoundObject>,
    /// Selected fields, in stream order.
    pub fields: Vec<BoundField>,
    /// Engine recorded on the matched binding. The request cannot supply this.
    pub source_engine: String,
    /// True only for `capture-receipt/v1`. The historical pin stays false.
    pub source_bound: bool,
    /// SHA-256 of the admission file that matched. Not an `admitted` bit.
    pub receipt_sha256: String,
    /// Sealed catalog schema preimage. Core rejects any other value.
    pub schema_digest: String,
    /// SHA-256 of the Arrow schema message. Empty on the historical pin.
    pub arrow_schema_sha256: String,
    /// Source-observed C4 scope, equal to the hash-bound capturer payload.
    pub partition_scope: Option<serde_json::Value>,
    pub transaction_scope: Option<serde_json::Value>,
}

/// Closed native products admitted by the real source capture boundary.
///
/// This list matches `native_arrow::recognized_engine`. A request cannot add one.
pub(crate) fn sealed_product(engine: &str) -> Result<(), &'static str> {
    if matches!(
        engine,
        "mysql" | "postgresql" | "postgres" | "sqlserver" | "oracle"
            | "clickhouse"
            | "opensearch"
            | "doris"
            | "mongodb"
            | "mongo"
            | "starrocks"
            | "mariadb"
    ) {
        Ok(())
    } else {
        Err("source_engine_unknown")
    }
}

/// `Ok(None)` keeps the historical empty-pin path. `Err` is closed.
#[allow(clippy::too_many_arguments)]
pub fn resolve(
    engine: &str,
    source_id: &str,
    generation: i64,
    snapshot_kind: &str,
    snapshot_value: &str,
    config_digest: &str,
    schema_digest: &str,
    type_digest: &str,
    input_digest: &str,
) -> Result<Option<ResolvedCapture>, &'static str> {
    let (bin, registry, registry_sha) = operator_values();
    if bin.is_none() && registry.is_none() && registry_sha.is_none() {
        return Ok(None);
    }
    let (Some(bin), Some(registry), Some(registry_sha)) = (bin, registry, registry_sha) else {
        return Err("nonadmitted");
    };
    if bin == "UNTRUSTED" || registry == "UNTRUSTED" || registry_sha == "UNTRUSTED" {
        return Err("nonadmitted");
    }
    if !hex64(&registry_sha) || !bin.starts_with('/') || !registry.starts_with('/') {
        return Err("nonadmitted");
    }
    let registry_bytes = read_capped(Path::new(&registry))?;
    if hex_bytes(&Sha256::digest(&registry_bytes)) != registry_sha {
        return Err("nonadmitted");
    }
    let document: serde_json::Value =
        serde_json::from_slice(&registry_bytes).map_err(|_| "nonadmitted")?;
    if contains_secret(&document) {
        return Err("nonadmitted");
    }
    if document.get("protocol").and_then(|value| value.as_str()) != Some(BINDING_REGISTRY_PROTOCOL)
    {
        return Err("nonadmitted");
    }
    let bindings = document
        .get("bindings")
        .and_then(|value| value.as_array())
        .ok_or("nonadmitted")?;
    let mut matched = None;
    for binding in bindings {
        if binding_matches(
            binding,
            engine,
            source_id,
            generation,
            snapshot_kind,
            snapshot_value,
            config_digest,
            schema_digest,
            type_digest,
            input_digest,
        )? {
            if matched.is_some() {
                return Err("nonadmitted");
            }
            matched = Some(binding);
        }
    }
    let Some(binding) = matched else {
        return Err("nonadmitted");
    };
    let parsed = parse_binding(binding)?;
    let kind = verify_inputs(&parsed, &registry_bytes)?;
    verify_binary(&bin)?;
    let decision = run_validator(&bin, &parsed.report_path, &parsed.config_path)?;
    if hex_bytes(&Sha256::digest(&decision)) != parsed.decision_sha256 {
        return Err("nonadmitted");
    }
    let decision_json: serde_json::Value =
        serde_json::from_slice(&decision).map_err(|_| "nonadmitted")?;
    if decision_json
        .get("protocol")
        .and_then(|value| value.as_str())
        != Some(DECISION_PROTOCOL)
    {
        return Err("nonadmitted");
    }
    if decision_json
        .get("admitted")
        .and_then(|value| value.as_bool())
        != Some(true)
    {
        return Err("nonadmitted");
    }
    if decision_json
        .get("matrix_mutated")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        return Err("nonadmitted");
    }
    if decision_json
        .pointer("/identity/source_id")
        .and_then(|value| value.as_str())
        != Some(source_id)
    {
        return Err("nonadmitted");
    }
    let blocks = decision_json
        .get("blocks")
        .and_then(|value| value.as_array())
        .ok_or("nonadmitted")?;
    if !blocks.is_empty() || !write_class_allowed(&decision_json, &parsed.source_engine) {
        return Err("nonadmitted");
    }
    Ok(Some(resolved_capture(
        &parsed,
        source_id,
        &registry_sha,
        kind,
    )))
}

/// Match a binding without a caller engine. An empty pin is `nonadmitted`.
///
/// The engine gate runs after the registry digest check and before
/// `verify_binary` or `Command::new`. MySQL and PostgreSQL cannot retag.
#[allow(clippy::too_many_arguments)]
pub fn resolve_sealed(
    source_id: &str,
    generation: i64,
    snapshot_kind: &str,
    snapshot_value: &str,
    config_digest: &str,
    schema_digest: &str,
    type_digest: &str,
    input_digest: &str,
) -> Result<ResolvedCapture, &'static str> {
    let (bin, registry, registry_sha) = operator_values();
    let (Some(bin), Some(registry), Some(registry_sha)) = (bin, registry, registry_sha) else {
        return Err("nonadmitted");
    };
    if bin == "UNTRUSTED" || registry == "UNTRUSTED" || registry_sha == "UNTRUSTED" {
        return Err("nonadmitted");
    }
    if !hex64(&registry_sha) || !bin.starts_with('/') || !registry.starts_with('/') {
        return Err("nonadmitted");
    }
    let registry_bytes = read_capped(Path::new(&registry))?;
    if hex_bytes(&Sha256::digest(&registry_bytes)) != registry_sha {
        return Err("nonadmitted");
    }
    let document: serde_json::Value =
        serde_json::from_slice(&registry_bytes).map_err(|_| "nonadmitted")?;
    if contains_secret(&document) {
        return Err("nonadmitted");
    }
    if document.get("protocol").and_then(|value| value.as_str()) != Some(BINDING_REGISTRY_PROTOCOL)
    {
        return Err("nonadmitted");
    }
    let bindings = document
        .get("bindings")
        .and_then(|value| value.as_array())
        .ok_or("nonadmitted")?;
    let mut matched = None;
    for binding in bindings {
        if binding_matches_sealed(
            binding,
            source_id,
            generation,
            snapshot_kind,
            snapshot_value,
            config_digest,
            schema_digest,
            type_digest,
            input_digest,
        )? {
            if matched.is_some() {
                return Err("nonadmitted");
            }
            matched = Some(binding);
        }
    }
    let Some(binding) = matched else {
        return Err("nonadmitted");
    };
    let engine = binding
        .get("source_engine")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    sealed_product(engine)?;
    let parsed = parse_binding(binding)?;
    if parsed.source_engine != engine {
        return Err("nonadmitted");
    }
    let kind = verify_inputs(&parsed, &registry_bytes)?;
    verify_binary(&bin)?;
    let decision = run_validator(&bin, &parsed.report_path, &parsed.config_path)?;
    if hex_bytes(&Sha256::digest(&decision)) != parsed.decision_sha256 {
        return Err("nonadmitted");
    }
    let decision_json: serde_json::Value =
        serde_json::from_slice(&decision).map_err(|_| "nonadmitted")?;
    if decision_json
        .get("protocol")
        .and_then(|value| value.as_str())
        != Some(DECISION_PROTOCOL)
    {
        return Err("nonadmitted");
    }
    if decision_json
        .get("admitted")
        .and_then(|value| value.as_bool())
        != Some(true)
    {
        return Err("nonadmitted");
    }
    if decision_json
        .get("matrix_mutated")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        return Err("nonadmitted");
    }
    if decision_json
        .pointer("/identity/source_id")
        .and_then(|value| value.as_str())
        != Some(source_id)
    {
        return Err("nonadmitted");
    }
    let blocks = decision_json
        .get("blocks")
        .and_then(|value| value.as_array())
        .ok_or("nonadmitted")?;
    if !blocks.is_empty() || !write_class_allowed(&decision_json, &parsed.source_engine) {
        return Err("nonadmitted");
    }
    Ok(resolved_capture(&parsed, source_id, &registry_sha, kind))
}

fn resolved_capture(
    parsed: &ParsedBinding,
    source_id: &str,
    registry_sha: &str,
    kind: AdmissionReceiptKind,
) -> ResolvedCapture {
    let (source_bound, receipt_sha256) = match kind {
        AdmissionReceiptKind::HistoricalReadonly => {
            (false, HISTORICAL_ADMISSION_RECEIPT_SHA256.to_string())
        }
        AdmissionReceiptKind::CaptureMatched { receipt_sha256 } => (true, receipt_sha256),
    };
    ResolvedCapture {
        decision_sha256: parsed.decision_sha256.clone(),
        source_id: source_id.to_string(),
        batches: parsed
            .objects
            .iter()
            .map(|item| item.arrow_sha256.clone())
            .collect(),
        registry_sha256: registry_sha.to_string(),
        driver_digest: parsed.driver_digest.clone(),
        objects: parsed.objects.clone(),
        fields: parsed.fields.clone(),
        source_engine: parsed.source_engine.clone(),
        source_bound,
        receipt_sha256,
        schema_digest: parsed.schema_digest.clone(),
        arrow_schema_sha256: parsed.arrow_schema_sha256.clone().unwrap_or_default(),
        partition_scope: parsed.partition_scope.clone(),
        transaction_scope: parsed.transaction_scope.clone(),
    }
}

struct ParsedBinding {
    decision_sha256: String,
    report_path: String,
    config_path: String,
    report_sha256: String,
    config_sha256: String,
    driver_digest: String,
    driver_kind: String,
    driver_name: String,
    driver_version: String,
    capture_receipt_path: String,
    capture_receipt_sha256: String,
    catalog_path: String,
    catalog_sha256: String,
    receipt_path: String,
    schema_digest: String,
    type_digest: String,
    config_digest: String,
    input_digest: String,
    source_id: String,
    source_engine: String,
    source_generation: i64,
    snapshot_kind: String,
    snapshot_value: String,
    run_id: Option<String>,
    admission_receipt_sha256: Option<String>,
    arrow_schema_sha256: Option<String>,
    objects: Vec<BoundObject>,
    fields: Vec<BoundField>,
    partition_scope: Option<serde_json::Value>,
    transaction_scope: Option<serde_json::Value>,
}

/// How an admission file matched a binding. Neither variant is `admitted`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdmissionReceiptKind {
    /// Frozen completion receipt. Only clickhouse and opensearch.
    HistoricalReadonly,
    /// `capture-receipt/v1` matched this binding. The validator is separate.
    CaptureMatched {
        /// SHA-256 of the receipt file.
        receipt_sha256: String,
    },
}

/// Values the receipt body must equal. The manifest cannot replace them.
pub struct CaptureReceipt<'a> {
    /// Binding `source_engine`.
    pub source_engine: &'a str,
    /// Binding `source_id`.
    pub source_id: &'a str,
    /// Binding generation.
    pub source_generation: i64,
    /// Snapshot kind.
    pub snapshot_kind: &'a str,
    /// Snapshot value.
    pub snapshot_value: &'a str,
    /// Binding run id, when the binding recorded one.
    pub run_id: Option<&'a str>,
    /// Binding schema digest.
    pub schema_digest: &'a str,
    /// Binding type digest.
    pub type_digest: &'a str,
    /// Binding config digest.
    pub config_digest: &'a str,
    /// Binding input digest.
    pub input_digest: &'a str,
    /// SHA-256 of the sealed catalog file.
    pub catalog_sha256: &'a str,
    /// SHA-256 of the operator registry file.
    pub registry_sha256: &'a str,
    /// Arrow batch digests in binding order.
    pub arrow_sha256: &'a [String],
    /// Driver digest.
    pub driver_digest: &'a str,
    /// SHA-256 of the Arrow schema message. Not the catalog preimage and not the batch hash.
    pub arrow_schema_sha256: &'a str,
    /// `admission_receipt_sha256` from the binding. Absent on the historical pin.
    pub declared_receipt_sha256: Option<&'a str>,
}

/// Match one admission file to one binding.
///
/// The historical receipt hash is not an allowlist entry for any other engine.
/// A capture receipt is matched by the hash declared on that binding.
pub fn check_admission_receipt(
    bytes: &[u8],
    expect: &CaptureReceipt<'_>,
) -> Result<AdmissionReceiptKind, &'static str> {
    if bytes.is_empty() || bytes.len() > MAX_JSON {
        return Err("nonadmitted");
    }
    let hash = hex_bytes(&Sha256::digest(bytes));
    if hash == HISTORICAL_ADMISSION_RECEIPT_SHA256 {
        let document: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| "nonadmitted")?;
        if document.get("schema").and_then(|value| value.as_str())
            != Some(HISTORICAL_RECEIPT_SCHEMA)
            || document
                .get("content_commit")
                .and_then(|value| value.as_str())
                != Some(HISTORICAL_CONTENT_COMMIT)
        {
            return Err("nonadmitted");
        }
        if matches!(expect.source_engine, "clickhouse" | "opensearch") {
            return Ok(AdmissionReceiptKind::HistoricalReadonly);
        }
        return Err("cross_source");
    }
    let Some(declared) = expect.declared_receipt_sha256 else {
        return Err("digest_mismatch");
    };
    if !hex64(declared) || hash != declared {
        return Err("digest_mismatch");
    }
    let document: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| "nonadmitted")?;
    if contains_secret(&document) {
        return Err("nonadmitted");
    }
    if document.get("schema").and_then(|value| value.as_str()) != Some(CAPTURE_RECEIPT_PROTOCOL) {
        return Err("nonadmitted");
    }
    if document
        .get("source_engine")
        .and_then(|value| value.as_str())
        != Some(expect.source_engine)
        || document.get("source_id").and_then(|value| value.as_str()) != Some(expect.source_id)
        || document
            .get("source_generation")
            .and_then(|value| value.as_i64())
            != Some(expect.source_generation)
        || document
            .pointer("/snapshot/kind")
            .and_then(|value| value.as_str())
            != Some(expect.snapshot_kind)
        || document
            .pointer("/snapshot/value")
            .and_then(|value| value.as_str())
            != Some(expect.snapshot_value)
    {
        return Err("cross_source");
    }
    let run_id = document
        .get("run_id")
        .and_then(|value| value.as_str())
        .ok_or("nonadmitted")?;
    if run_id.is_empty() || run_id.len() > 128 || run_id.bytes().any(|byte| byte < 0x20) {
        return Err("nonadmitted");
    }
    if expect.run_id.is_some_and(|expected| expected != run_id) {
        return Err("cross_source");
    }
    for (name, want) in [
        ("schema_digest", expect.schema_digest),
        ("type_digest", expect.type_digest),
        ("config_digest", expect.config_digest),
        ("input_digest", expect.input_digest),
        ("catalog_sha256", expect.catalog_sha256),
        ("registry_sha256", expect.registry_sha256),
        ("driver_digest", expect.driver_digest),
        ("arrow_schema_sha256", expect.arrow_schema_sha256),
    ] {
        let got = document
            .get(name)
            .and_then(|value| value.as_str())
            .ok_or("digest_mismatch")?;
        if !hex64(got) || got != want {
            return Err("digest_mismatch");
        }
    }
    same_arrow_list(
        document.get("arrow_sha256").ok_or("digest_mismatch")?,
        expect.arrow_sha256,
    )?;
    Ok(AdmissionReceiptKind::CaptureMatched {
        receipt_sha256: hash,
    })
}

fn same_arrow_list(got: &serde_json::Value, expect: &[String]) -> Result<(), &'static str> {
    let items = got.as_array().ok_or("digest_mismatch")?;
    if items.len() != expect.len() {
        return Err("digest_mismatch");
    }
    for (item, want) in items.iter().zip(expect.iter()) {
        let text = item.as_str().ok_or("digest_mismatch")?;
        if !hex64(text) || text != want {
            return Err("digest_mismatch");
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn binding_matches(
    binding: &serde_json::Value,
    engine: &str,
    source_id: &str,
    generation: i64,
    snapshot_kind: &str,
    snapshot_value: &str,
    config_digest: &str,
    schema_digest: &str,
    type_digest: &str,
    input_digest: &str,
) -> Result<bool, &'static str> {
    if binding.get("protocol").and_then(|value| value.as_str()) != Some(CAPTURE_BINDING_PROTOCOL) {
        return Err("nonadmitted");
    }
    let same = binding
        .get("source_engine")
        .and_then(|value| value.as_str())
        == Some(engine)
        && binding.get("source_id").and_then(|value| value.as_str()) == Some(source_id)
        && binding
            .get("source_generation")
            .and_then(|value| value.as_i64())
            == Some(generation)
        && binding
            .pointer("/snapshot/kind")
            .and_then(|value| value.as_str())
            == Some(snapshot_kind)
        && binding
            .pointer("/snapshot/value")
            .and_then(|value| value.as_str())
            == Some(snapshot_value)
        && binding
            .get("config_digest")
            .and_then(|value| value.as_str())
            == Some(config_digest)
        && binding
            .get("schema_digest")
            .and_then(|value| value.as_str())
            == Some(schema_digest)
        && binding.get("type_digest").and_then(|value| value.as_str()) == Some(type_digest)
        && binding.get("input_digest").and_then(|value| value.as_str()) == Some(input_digest);
    Ok(same)
}

#[allow(clippy::too_many_arguments)]
fn binding_matches_sealed(
    binding: &serde_json::Value,
    source_id: &str,
    generation: i64,
    snapshot_kind: &str,
    snapshot_value: &str,
    config_digest: &str,
    schema_digest: &str,
    type_digest: &str,
    input_digest: &str,
) -> Result<bool, &'static str> {
    if binding.get("protocol").and_then(|value| value.as_str()) != Some(CAPTURE_BINDING_PROTOCOL) {
        return Err("nonadmitted");
    }
    let same = binding.get("source_id").and_then(|value| value.as_str()) == Some(source_id)
        && binding
            .get("source_generation")
            .and_then(|value| value.as_i64())
            == Some(generation)
        && binding
            .pointer("/snapshot/kind")
            .and_then(|value| value.as_str())
            == Some(snapshot_kind)
        && binding
            .pointer("/snapshot/value")
            .and_then(|value| value.as_str())
            == Some(snapshot_value)
        && binding
            .get("config_digest")
            .and_then(|value| value.as_str())
            == Some(config_digest)
        && binding
            .get("schema_digest")
            .and_then(|value| value.as_str())
            == Some(schema_digest)
        && binding.get("type_digest").and_then(|value| value.as_str()) == Some(type_digest)
        && binding.get("input_digest").and_then(|value| value.as_str()) == Some(input_digest);
    Ok(same)
}

fn parse_binding(binding: &serde_json::Value) -> Result<ParsedBinding, &'static str> {
    let objects = binding
        .get("objects")
        .and_then(|value| value.as_array())
        .ok_or("nonadmitted")?;
    if objects.is_empty() {
        return Err("nonadmitted");
    }
    let mut parsed_objects = Vec::with_capacity(objects.len());
    for object in objects {
        let arrow = object
            .get("arrow_sha256")
            .and_then(|value| value.as_str())
            .ok_or("digest_mismatch")?;
        if !hex64(arrow) {
            return Err("digest_mismatch");
        }
        let row_count = object
            .get("row_count")
            .and_then(|value| value.as_u64())
            .ok_or("schema")?;
        parsed_objects.push(BoundObject {
            logical_schema: required_token(object, "logical_schema")?,
            logical_table: required_token(object, "logical_table")?,
            row_count,
            arrow_sha256: arrow.to_string(),
        });
    }
    let fields = binding
        .get("fields")
        .and_then(|value| value.as_array())
        .ok_or("schema")?;
    if fields.is_empty() {
        return Err("schema");
    }
    let partition_scope = binding.get("partition_scope").cloned();
    if let Some(scope) = partition_scope.as_ref() {
        if !matches!(binding.get("source_engine").and_then(serde_json::Value::as_str),
            Some("clickhouse" | "doris" | "starrocks")) {
            return Err("schema");
        }
        validate_partition_scope(scope, binding, &parsed_objects)?;
    }
    let transaction_scope = binding.get("transaction_scope").cloned();
    if transaction_scope.is_some() && (!matches!(binding.get("source_engine").and_then(serde_json::Value::as_str), Some("postgres" | "postgresql"))
        || partition_scope.is_some()) { return Err("schema"); }
    let mut parsed_fields = Vec::with_capacity(fields.len());
    for field in fields {
        let source_oid = match field.get("source_oid") {
            None => None,
            Some(value) => Some(u32::try_from(value.as_u64().ok_or("schema")?).map_err(|_| "schema")?),
        };
        let source_typmod = opt_i32(field, "source_typmod")?;
        let source_format = match field.get("source_format") {
            None => None,
            Some(value) => Some(value.as_str().filter(|value| !value.is_empty()).ok_or("schema")?.to_owned()),
        };
        if source_oid.is_some() || source_typmod.is_some() || source_format.is_some() {
            if !matches!(binding.get("source_engine").and_then(serde_json::Value::as_str), Some("postgres" | "postgresql"))
                || source_oid.is_none() || source_typmod.is_none() || source_format.is_none() {
                return Err("schema");
            }
        }
        let bson_type = match field.get("bson_type") {
            None => None,
            Some(value) => {
                if binding.get("source_engine").and_then(serde_json::Value::as_str) != Some("mongodb") {
                    return Err("schema");
                }
                let variant = value.get("variant").and_then(serde_json::Value::as_str).ok_or("schema")?;
                let subtype = value.get("subtype").and_then(serde_json::Value::as_str);
                if variant.len() != 2 || !variant.bytes().all(|byte| byte.is_ascii_hexdigit())
                    || subtype.is_some_and(|item| item.len() != 2 || !item.bytes().all(|byte| byte.is_ascii_hexdigit()))
                    || (variant == "05") != subtype.is_some() {
                    return Err("schema");
                }
                Some(BsonType { variant: variant.to_owned(), subtype: subtype.map(str::to_owned) })
            }
        };
        if binding.get("source_engine").and_then(serde_json::Value::as_str) == Some("mongodb")
            && matches!(required_token(field, "arrow_type")?.as_str(), "fixedsizebinary(12)" | "fixedsizebinary(16)")
            && bson_type.is_none() {
            return Err("schema");
        }
        parsed_fields.push(BoundField {
            name: required_token(field, "name")?,
            arrow_type: required_token(field, "arrow_type")?,
            nullable: field
                .get("nullable")
                .and_then(|value| value.as_bool())
                .ok_or("schema")?,
            precision: opt_i32(field, "precision")?,
            scale: opt_i32(field, "scale")?,
            timezone: field
                .get("timezone")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            char_length: opt_i32(field, "char_length")?,
            key: opt_key(field)?,
            document: opt_document(field)?,
            source_oid,
            source_typmod,
            source_format,
            bson_type,
        });
    }
    Ok(ParsedBinding {
        decision_sha256: required_hex(binding, "decision_sha256")?,
        report_path: required_abs(binding, "report_path")?,
        config_path: required_abs(binding, "config_path")?,
        report_sha256: required_hex(binding, "report_sha256")?,
        config_sha256: required_hex(binding, "config_sha256")?,
        driver_digest: required_hex(binding, "driver_digest")?,
        driver_kind: required_token(binding, "driver_kind")?,
        driver_name: required_token(binding, "driver_name")?,
        driver_version: required_token(binding, "driver_version")?,
        capture_receipt_path: required_abs(binding, "capture_receipt_path")?,
        capture_receipt_sha256: required_hex(binding, "capture_receipt_sha256")?,
        catalog_path: required_abs(binding, "catalog_path")?,
        catalog_sha256: required_hex(binding, "catalog_sha256")?,
        receipt_path: required_abs(binding, "admission_receipt_path")?,
        schema_digest: required_hex(binding, "schema_digest")?,
        type_digest: required_hex(binding, "type_digest")?,
        config_digest: required_hex(binding, "config_digest")?,
        input_digest: required_hex(binding, "input_digest")?,
        source_id: required_token(binding, "source_id")?,
        source_engine: required_token(binding, "source_engine")?,
        source_generation: binding
            .get("source_generation")
            .and_then(|value| value.as_i64())
            .ok_or("schema")?,
        snapshot_kind: required_token(
            binding
                .pointer("/snapshot")
                .unwrap_or(&serde_json::Value::Null),
            "kind",
        )?,
        snapshot_value: required_token(
            binding
                .pointer("/snapshot")
                .unwrap_or(&serde_json::Value::Null),
            "value",
        )?,
        run_id: optional_token(binding, "run_id")?,
        admission_receipt_sha256: optional_hex(binding, "admission_receipt_sha256")?,
        arrow_schema_sha256: optional_hex(binding, "arrow_schema_sha256")?,
        objects: parsed_objects,
        fields: parsed_fields,
        partition_scope,
        transaction_scope,
    })
}

fn validate_partition_scope(
    scope: &serde_json::Value,
    binding: &serde_json::Value,
    objects: &[BoundObject],
) -> Result<(), &'static str> {
    const KEYS: &[&str] = &[
        "source_id", "source_generation", "logical_schema", "logical_table",
        "partition_id", "observed_partition_ids", "complete", "table_epoch",
        "schema_fingerprint", "schema_digest", "type_digest", "arrow_sha256",
        "row_count", "current_signature", "mutations_pending",
    ];
    const OLAP_KEYS: &[&str] = &[
        "source_engine", "key_model", "routing_column", "partition_range",
        "native_partition_id", "native_partition_version",
    ];
    let olap = matches!(binding["source_engine"].as_str(), Some("doris" | "starrocks"));
    if !olap && binding["source_engine"] != "clickhouse" { return Err("schema"); }
    let object = scope.as_object().ok_or("schema")?;
    if object.len() != KEYS.len() + if olap { OLAP_KEYS.len() } else { 0 }
        || KEYS.iter().any(|key| !object.contains_key(*key))
        || (olap && OLAP_KEYS.iter().any(|key| !object.contains_key(*key)))
        || objects.len() != 1 || scope["source_id"] != binding["source_id"]
        || scope["source_generation"] != binding["source_generation"]
        || scope["schema_digest"] != binding["schema_digest"]
        || scope["type_digest"] != binding["type_digest"]
        || scope["logical_schema"] != objects[0].logical_schema
        || scope["logical_table"] != objects[0].logical_table
        || scope["arrow_sha256"] != objects[0].arrow_sha256
        || scope["row_count"].as_u64() != Some(objects[0].row_count)
        || scope["complete"] != true || scope["mutations_pending"] != false {
        return Err("schema");
    }
    for key in ["partition_id", "table_epoch", "schema_fingerprint"] {
        if scope[key].as_str().is_none_or(str::is_empty) { return Err("schema"); }
    }
    if olap {
        let native_id = scope["native_partition_id"].as_str().ok_or("schema")?;
        let signature = scope["current_signature"].as_str().ok_or("schema")?;
        if scope["source_engine"] != binding["source_engine"]
            || !matches!(scope["key_model"].as_str(), Some("duplicate" | "unique" | "primary"))
            || scope["routing_column"].as_str().is_none_or(|value| value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
            || !partition_decimal(native_id)
            || !scope["native_partition_version"].as_str().is_some_and(partition_decimal)
            || !scope["table_epoch"].as_str().is_some_and(partition_digest)
            || !scope["schema_fingerprint"].as_str().is_some_and(partition_digest)
            || !scope["partition_range"].as_object().is_some_and(partition_range)
            || !signature.strip_prefix(&format!("{native_id}|")).is_some_and(partition_digest)
        { return Err("schema"); }
    }
    if !scope["current_signature"].is_null()
        && scope["current_signature"].as_str().is_none_or(str::is_empty) {
        return Err("schema");
    }
    if scope["current_signature"].is_null() && objects[0].row_count != 0 {
        return Err("schema");
    }
    let ids = scope["observed_partition_ids"].as_array().ok_or("schema")?;
    if ids.is_empty() || ids.len() > 1024 { return Err("schema"); }
    let mut previous = "";
    for id in ids {
        let value = id.as_str().filter(|value| !value.is_empty()).ok_or("schema")?;
        if value <= previous { return Err("schema"); }
        previous = value;
    }
    if !ids.iter().any(|id| id == &scope["partition_id"]) { return Err("schema"); }
    Ok(())
}

fn partition_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn partition_decimal(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|number| number > 0)
}

fn partition_range(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    if object.len() != 2 { return false; }
    let (Some(lower), Some(upper)) = (object.get("lower").and_then(serde_json::Value::as_str),
        object.get("upper").and_then(serde_json::Value::as_str)) else { return false; };
    let iso = |date: &str| date.len() == 10 && date.bytes().enumerate().all(|(index, byte)|
        if matches!(index, 4 | 7) { byte == b'-' } else { byte.is_ascii_digit() });
    iso(lower) && iso(upper) && lower < upper
}

/// Grade and audit taken from a frozen server registration, not from the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerRegistration {
    pub fields: Vec<BoundField>,
    pub partition_scope: Option<serde_json::Value>,
    pub transaction_scope: Option<serde_json::Value>,
    /// SHA-256 of the registry with the receipt stamp replaced by 64 zero digits.
    pub preimage_sha256: String,
    /// SHA-256 of the receipt file named by the registry stamp.
    pub receipt_sha256: String,
    /// SHA-256 of the catalog file the registry names.
    pub catalog_sha256: String,
    /// SHA-256 of the capturer file. This is the audit the document must name.
    pub audit_sha256: String,
    /// `consistency` on the receipt.
    pub consistency: String,
    /// True only when the receipt's cross-table token is `T` and the grade is `T`.
    pub cross_table: bool,
}

/// Open the registry, the receipt, the catalog, the driver, and the audit.
///
/// `registry_sha256` on the receipt is the preimage hash. The registry file
/// stores the receipt hash in `admission_receipt_sha256`. Comparing the receipt
/// field to the full registry file hash is the circular check and is refused.
pub fn open_server_registration(
    registry: &[u8],
    receipt: &[u8],
    arrow_sha256: &str,
    arrow_schema_sha256: Option<&str>,
) -> Result<ServerRegistration, &'static str> {
    if registry.is_empty()
        || receipt.is_empty()
        || registry.len() > MAX_JSON
        || receipt.len() > MAX_JSON
    {
        return Err("nonadmitted");
    }
    let (preimage, stamp) = registry_preimage(registry)?;
    let receipt_sha = hex_bytes(&Sha256::digest(receipt));
    if stamp != receipt_sha || hex_bytes(&Sha256::digest(registry)) == preimage {
        return Err("digest_mismatch");
    }
    let document: serde_json::Value =
        serde_json::from_slice(registry).map_err(|_| "nonadmitted")?;
    if contains_secret(&document) {
        return Err("nonadmitted");
    }
    let binding = document
        .get("bindings")
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .ok_or("nonadmitted")?;
    if binding.get("protocol").and_then(|value| value.as_str()) != Some(CAPTURE_BINDING_PROTOCOL) {
        return Err("nonadmitted");
    }
    let receipt_json: serde_json::Value =
        serde_json::from_slice(receipt).map_err(|_| "nonadmitted")?;
    if contains_secret(&receipt_json) {
        return Err("nonadmitted");
    }
    if receipt_json.get("schema").and_then(|value| value.as_str()) != Some(CAPTURE_RECEIPT_PROTOCOL)
    {
        return Err("nonadmitted");
    }
    if receipt_json
        .get("registry_sha256")
        .and_then(|value| value.as_str())
        != Some(preimage.as_str())
    {
        return Err("digest_mismatch");
    }
    let catalog_sha = required_hex(binding, "catalog_sha256")?;
    let driver_digest = required_hex(binding, "driver_digest")?;
    let driver_sha = required_hex(binding, "driver_sha256")?;
    if driver_sha != driver_digest {
        return Err("digest_mismatch");
    }
    let audit_sha = required_hex(binding, "capture_receipt_sha256")?;
    let input_digest = required_hex(binding, "input_digest")?;
    let schema_digest = required_hex(binding, "schema_digest")?;
    let type_digest = required_hex(binding, "type_digest")?;
    let config_digest = required_hex(binding, "config_digest")?;
    let arrow_schema = required_hex(binding, "arrow_schema_sha256")?;
    if arrow_sha256 != input_digest || arrow_schema_sha256 != Some(arrow_schema.as_str()) {
        return Err("digest_mismatch");
    }
    if arrow_schema == schema_digest {
        return Err("digest_mismatch");
    }
    same_hex_field(&receipt_json, "catalog_sha256", &catalog_sha)?;
    same_hex_field(&receipt_json, "driver_digest", &driver_digest)?;
    same_hex_field(&receipt_json, "input_digest", &input_digest)?;
    same_hex_field(&receipt_json, "schema_digest", &schema_digest)?;
    same_hex_field(&receipt_json, "type_digest", &type_digest)?;
    same_hex_field(&receipt_json, "config_digest", &config_digest)?;
    same_hex_field(&receipt_json, "arrow_schema_sha256", &arrow_schema)?;
    let object_arrow = binding
        .pointer("/objects/0/arrow_sha256")
        .and_then(|value| value.as_str())
        .ok_or("digest_mismatch")?;
    if !hex64(object_arrow) || object_arrow != input_digest {
        return Err("digest_mismatch");
    }
    let catalog_bytes = read_hashed(&required_abs(binding, "catalog_path")?, &catalog_sha)?;
    let catalog: serde_json::Value =
        serde_json::from_slice(&catalog_bytes).map_err(|_| "nonadmitted")?;
    if catalog.get("protocol").and_then(|value| value.as_str()) != Some(CATALOG_PROTOCOL) {
        return Err("nonadmitted");
    }
    for (name, want) in [
        ("schema_digest", schema_digest.as_str()),
        ("type_digest", type_digest.as_str()),
        ("config_digest", config_digest.as_str()),
        ("input_digest", input_digest.as_str()),
    ] {
        if catalog.get(name).and_then(|value| value.as_str()) != Some(want) {
            return Err("digest_mismatch");
        }
    }
    let engine = required_token(binding, "source_engine")?;
    let source_id = required_token(binding, "source_id")?;
    let generation = binding
        .get("source_generation")
        .and_then(|value| value.as_i64())
        .ok_or("nonadmitted")?;
    if catalog.get("engine").and_then(|value| value.as_str()) != Some(engine.as_str())
        || catalog.get("source_id").and_then(|value| value.as_str()) != Some(source_id.as_str())
        || catalog
            .get("source_generation")
            .and_then(|value| value.as_i64())
            != Some(generation)
        || receipt_json
            .get("source_engine")
            .and_then(|value| value.as_str())
            != Some(engine.as_str())
        || receipt_json
            .get("source_id")
            .and_then(|value| value.as_str())
            != Some(source_id.as_str())
        || receipt_json
            .get("source_generation")
            .and_then(|value| value.as_i64())
            != Some(generation)
    {
        return Err("cross_source");
    }
    read_driver(&required_abs(binding, "driver_path")?, &driver_digest)?;
    let _audit = read_capture_receipt(&required_abs(binding, "capture_receipt_path")?, &audit_sha)?;
    let consistency = receipt_json
        .get("consistency")
        .and_then(|value| value.as_str())
        .filter(|token| matches!(*token, "T" | "S1" | "S0" | "B"))
        .ok_or("consistency")?
        .to_string();
    let cross_table = match receipt_json.get("cross_table_consistency") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::String(text)) if text == "T" && consistency == "T" => true,
        Some(serde_json::Value::String(text)) if text == "T" => return Err("consistency"),
        Some(serde_json::Value::String(text)) if matches!(text.as_str(), "S1" | "S0" | "B") => {
            false
        }
        _ => return Err("consistency"),
    };
    Ok(ServerRegistration {
        fields: Vec::new(),
        partition_scope: None,
        transaction_scope: binding.get("transaction_scope").cloned(),
        preimage_sha256: preimage,
        receipt_sha256: receipt_sha,
        catalog_sha256: catalog_sha,
        audit_sha256: audit_sha,
        consistency,
        cross_table,
    })
}

fn same_hex_field(
    document: &serde_json::Value,
    name: &str,
    want: &str,
) -> Result<(), &'static str> {
    let got = document
        .get(name)
        .and_then(|value| value.as_str())
        .ok_or("digest_mismatch")?;
    if hex64(got) && got == want {
        Ok(())
    } else {
        Err("digest_mismatch")
    }
}

/// `(preimage sha256, receipt stamp)`. The stamp is the one 64-hex value that
/// the sealed registry substituted for [`REGISTRY_STAMP_BLANK`].
pub fn registry_preimage(registry: &[u8]) -> Result<(String, String), &'static str> {
    let document: serde_json::Value =
        serde_json::from_slice(registry).map_err(|_| "nonadmitted")?;
    let bindings = document
        .get("bindings")
        .and_then(|value| value.as_array())
        .ok_or("nonadmitted")?;
    if bindings.len() != 1 {
        return Err("nonadmitted");
    }
    let stamp = bindings[0]
        .get("admission_receipt_sha256")
        .and_then(|value| value.as_str())
        .ok_or("digest_mismatch")?;
    if !hex64(stamp) || stamp == REGISTRY_STAMP_BLANK {
        return Err("digest_mismatch");
    }
    if count_slice(registry, stamp.as_bytes()) != 1 {
        return Err("digest_mismatch");
    }
    let preimage = replace_once(registry, stamp.as_bytes(), REGISTRY_STAMP_BLANK.as_bytes())?;
    let hash = hex_bytes(&Sha256::digest(&preimage));
    if hash == REGISTRY_STAMP_BLANK || hash == hex_bytes(&Sha256::digest(registry)) {
        return Err("digest_mismatch");
    }
    Ok((hash, stamp.to_string()))
}

fn count_slice(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut count = 0;
    let mut index = 0;
    while index + needle.len() <= haystack.len() {
        if &haystack[index..index + needle.len()] == needle {
            count += 1;
            index += needle.len();
        } else {
            index += 1;
        }
    }
    count
}

fn replace_once(
    haystack: &[u8],
    needle: &[u8],
    replacement: &[u8],
) -> Result<Vec<u8>, &'static str> {
    let Some(position) = haystack
        .windows(needle.len())
        .position(|window| window == needle)
    else {
        return Err("digest_mismatch");
    };
    let mut out = Vec::with_capacity(haystack.len() - needle.len() + replacement.len());
    out.extend_from_slice(&haystack[..position]);
    out.extend_from_slice(replacement);
    out.extend_from_slice(&haystack[position + needle.len()..]);
    Ok(out)
}

const MAX_DRIVER: u64 = 32 * 1024 * 1024;

/// Approved driver resources. Each pair is a digest and its exact byte length.
///
/// A file above [`MAX_DRIVER`] is read only when both match. Any other digest
/// stays under that cap. The declaration is not a caller-chosen size.
const APPROVED_DRIVER_RESOURCES: &[(&str, u64)] = &[
    (
        "8b88485c0940e298a8edad500731f0a7688c61a135a943b217f9597e191c7f93",
        87_004_088,
    ),
    (
        "7ee551ecadd2fb66e3dea5564dab7336cafcce0e6fb0dd57d5816a7685338474",
        314_875_904,
    ),
    (
        "613612c3245a8bbb83d7534d75b6babc2aedd351ed91c9d7948c8df2b5570e31",
        115_391_888,
    ),
    (
        "1bb33227c6012cd83bc25cebaa292cb8cbda6dbdf4217085b65524bd1935a08f",
        113_963_816,
    ),
    (
        "231fdce4f6da57e8c6f980a718bb72e9cdccda51f1e3fe5e7eb56ea584572b2f",
        123_868_624,
    ),
    (
        "21bbdc81d436bbffb2951b2245068c1667dccd02ed32a6667b3770b4b7bd1aee",
        113_935_600,
    ),
    (
        "efc9979c1f1f87140ce070eb9f2f398675ca8fb84dcb0e103a696711d6687e3f",
        81_806_136,
    ),
    (
        "d5dc8555ad76ecc9d4550ac5d2abd6b00bb2fee6ca5a17c7c206d294b206010c",
        115_155_008,
    ),
    (
        "874d2588595d63b597139358b47e3097fcc5f45f772ffb5000ecd371e8e88b2e",
        116_863_584,
    ),
    (
        "683194dab7fd5ba58efb6da4009e82a8683ea3d07913153e8a7ac931a1caef4e",
        113_981_880,
    ),
    (
        "29fbbeb07880dccffbfe149262f99422e96b643c597b645bdaf43794b1c2734d",
        116_576_104,
    ),
];

pub(crate) fn driver_length_allowed(digest: &str, len: u64) -> bool {
    if len == 0 {
        return false;
    }
    match APPROVED_DRIVER_RESOURCES
        .iter()
        .find(|(item, _)| *item == digest)
    {
        Some((_, declared)) => len == *declared,
        None => len <= MAX_DRIVER,
    }
}

fn read_driver(path: &str, expect: &str) -> Result<(), &'static str> {
    let meta = fs::metadata(path).map_err(|_| "digest_missing")?;
    if !meta.is_file() || !driver_length_allowed(expect, meta.len()) {
        return Err("digest_mismatch");
    }
    let bytes = fs::read(path).map_err(|_| "digest_missing")?;
    if bytes.len() as u64 != meta.len() {
        return Err("digest_mismatch");
    }
    if hex_bytes(&Sha256::digest(&bytes)) == expect {
        Ok(())
    } else {
        Err("digest_mismatch")
    }
}

fn verify_inputs(
    binding: &ParsedBinding,
    registry_bytes: &[u8],
) -> Result<AdmissionReceiptKind, &'static str> {
    let (preimage, stamp) = registry_preimage(registry_bytes)?;
    let report_bytes = read_hashed(&binding.report_path, &binding.report_sha256)?;
    let config_bytes = read_hashed(&binding.config_path, &binding.config_sha256)?;
    let _receipt = read_capture_receipt(
        &binding.capture_receipt_path,
        &binding.capture_receipt_sha256,
    )?;
    if let Some(scope) = binding.partition_scope.as_ref() {
        let capturer: serde_json::Value = serde_json::from_slice(&_receipt).map_err(|_| "nonadmitted")?;
        if capturer.get("partition_scope") != Some(scope) {
            return Err("nonadmitted");
        }
    }
    if let Some(scope) = binding.transaction_scope.as_ref() {
        let capturer: serde_json::Value = serde_json::from_slice(&_receipt).map_err(|_| "nonadmitted")?;
        if capturer.get("transaction_scope") != Some(scope) { return Err("nonadmitted"); }
    }
    let catalog_bytes = read_hashed(&binding.catalog_path, &binding.catalog_sha256)?;
    let admission = read_capped(Path::new(&binding.receipt_path))?;
    let admission_hash = hex_bytes(&Sha256::digest(&admission));
    if stamp != admission_hash
        || binding.admission_receipt_sha256.as_deref() != Some(stamp.as_str())
    {
        return Err("digest_mismatch");
    }
    let arrows: Vec<String> = binding
        .objects
        .iter()
        .map(|item| item.arrow_sha256.clone())
        .collect();
    let kind = check_admission_receipt(
        &admission,
        &CaptureReceipt {
            source_engine: &binding.source_engine,
            source_id: &binding.source_id,
            source_generation: binding.source_generation,
            snapshot_kind: &binding.snapshot_kind,
            snapshot_value: &binding.snapshot_value,
            run_id: binding.run_id.as_deref(),
            schema_digest: &binding.schema_digest,
            type_digest: &binding.type_digest,
            config_digest: &binding.config_digest,
            input_digest: &binding.input_digest,
            catalog_sha256: &binding.catalog_sha256,
            registry_sha256: &preimage,
            arrow_sha256: &arrows,
            driver_digest: &binding.driver_digest,
            arrow_schema_sha256: binding.arrow_schema_sha256.as_deref().unwrap_or(""),
            declared_receipt_sha256: binding.admission_receipt_sha256.as_deref(),
        },
    )?;
    let report: serde_json::Value =
        serde_json::from_slice(&report_bytes).map_err(|_| "nonadmitted")?;
    if report.get("protocol").and_then(|value| value.as_str()) != Some(REPORT_PROTOCOL)
        || report.get("source_id").and_then(|value| value.as_str())
            != Some(binding.source_id.as_str())
    {
        return Err("nonadmitted");
    }
    let driver = report.get("driver").ok_or("nonadmitted")?;
    if driver.get("kind").and_then(|value| value.as_str()) != Some(binding.driver_kind.as_str())
        || driver.get("name").and_then(|value| value.as_str()) != Some(binding.driver_name.as_str())
        || driver.get("version").and_then(|value| value.as_str())
            != Some(binding.driver_version.as_str())
        || driver.get("digest").and_then(|value| value.as_str())
            != Some(binding.driver_digest.as_str())
    {
        return Err("nonadmitted");
    }
    let config: serde_json::Value =
        serde_json::from_slice(&config_bytes).map_err(|_| "nonadmitted")?;
    if config.get("source_id").and_then(|value| value.as_str()) != Some(binding.source_id.as_str())
    {
        return Err("nonadmitted");
    }
    verify_config(&config, &binding.source_engine)?;
    verify_r1_capture(&config, &binding.source_engine, &admission, &_receipt)?;
    let catalog: serde_json::Value =
        serde_json::from_slice(&catalog_bytes).map_err(|_| "nonadmitted")?;
    if catalog.get("protocol").and_then(|value| value.as_str()) != Some(CATALOG_PROTOCOL)
        || catalog.get("engine").and_then(|value| value.as_str())
            != Some(binding.source_engine.as_str())
        || catalog.get("source_id").and_then(|value| value.as_str())
            != Some(binding.source_id.as_str())
        || catalog
            .get("source_generation")
            .and_then(|value| value.as_i64())
            != Some(binding.source_generation)
        || catalog
            .get("schema_digest")
            .and_then(|value| value.as_str())
            != Some(binding.schema_digest.as_str())
        || catalog.get("type_digest").and_then(|value| value.as_str())
            != Some(binding.type_digest.as_str())
        || catalog
            .get("config_digest")
            .and_then(|value| value.as_str())
            != Some(binding.config_digest.as_str())
        || catalog.get("input_digest").and_then(|value| value.as_str())
            != Some(binding.input_digest.as_str())
    {
        return Err("digest_mismatch");
    }
    let _ = catalog.get("state");
    Ok(kind)
}

fn verify_config(config: &serde_json::Value, engine: &str) -> Result<(), &'static str> {
    if config
        .get("requested_write_class")
        .and_then(|value| value.as_str())
        .is_none_or(|class| class != "R0" && !(class == "R1" && matches!(engine, "postgresql" | "postgres" | "sqlserver")))
    {
        return Err("nonadmitted");
    }
    if !config.get("r1w").is_some_and(serde_json::Value::is_null) {
        return Err("nonadmitted");
    }
    let options = config
        .get("write_options")
        .and_then(|value| value.as_object())
        .ok_or("nonadmitted")?;
    for key in [
        "signal_table",
        "heartbeat_write",
        "create_stream",
        "create_trigger",
    ] {
        if options.get(key).and_then(|value| value.as_bool()) != Some(false) {
            return Err("nonadmitted");
        }
    }
    if options.values().any(|value| value.as_bool() == Some(true)) {
        return Err("nonadmitted");
    }
    let budget = config
        .get("budget")
        .and_then(|value| value.as_object())
        .ok_or("nonadmitted")?;
    if budget.get("unit").and_then(|value| value.as_str()) != Some("Rows")
        || budget
            .get("hard_limit")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            == 0
    {
        return Err("nonadmitted");
    }
    let debezium = config.get("debezium").ok_or("nonadmitted")?;
    if debezium
        .get("snapshot_mode")
        .and_then(|value| value.as_str())
        != Some("never")
        || debezium
            .get("create_stream")
            .and_then(|value| value.as_bool())
            != Some(false)
        || debezium
            .get("create_trigger")
            .and_then(|value| value.as_bool())
            != Some(false)
    {
        return Err("nonadmitted");
    }
    if engine == "clickhouse" {
        let readonly = config
            .pointer("/native/clickhouse/readonly")
            .and_then(|value| value.as_str());
        if readonly != Some("1") {
            return Err("nonadmitted");
        }
        if config.pointer("/native/clickhouse/quota").is_none() {
            return Err("nonadmitted");
        }
    }
    Ok(())
}

fn verify_binary(path: &str) -> Result<(), &'static str> {
    let bytes = fs::read(path).map_err(|_| "nonadmitted")?;
    let digest = hex_bytes(&Sha256::digest(&bytes));
    if !approved_admission_bin(&digest) {
        return Err("nonadmitted");
    }
    Ok(())
}

fn run_validator(bin: &str, report: &str, config: &str) -> Result<Vec<u8>, &'static str> {
    let output = Command::new(bin)
        .arg("validate")
        .arg("--report")
        .arg(report)
        .arg("--config")
        .arg(config)
        .env_clear()
        .output()
        .map_err(|_| "nonadmitted")?;
    if output.stdout.len() > MAX_JSON || output.stdout.is_empty() {
        return Err("nonadmitted");
    }
    if output.status.code() == Some(1) {
        return Err("nonadmitted");
    }
    Ok(output.stdout)
}

fn write_class_allowed(decision: &serde_json::Value, engine: &str) -> bool {
    let Some(capabilities) = decision
        .pointer("/model/capabilities")
        .and_then(|value| value.as_array())
    else {
        return false;
    };
    let Some(methods) = decision
        .pointer("/model/capture_methods")
        .and_then(|value| value.as_array())
    else {
        return false;
    };
    if capabilities.is_empty() || methods.is_empty() {
        return false;
    }
    capabilities.iter().all(|item| {
        matches!(item.get("write_class").and_then(|value| value.as_str()), Some("R0"))
            || (item.get("write_class").and_then(|value| value.as_str()) == Some("R1")
                && ((matches!(engine, "postgresql" | "postgres") && item.get("cdc").and_then(|v| v.as_str()) == Some("LogicalDecoding"))
                    || (engine == "sqlserver" && item.get("feed").and_then(|v| v.as_str()) == Some("ChangeTracking"))))
    }) && methods.iter().all(|item| {
        item.get("write_class").and_then(|value| value.as_str()) == Some("R0")
            || (item.get("write_class").and_then(|value| value.as_str()) == Some("R1")
                && ((matches!(engine, "postgresql" | "postgres") && item.get("method").and_then(|v| v.as_str()) == Some("C1") && item.get("cdc").and_then(|v| v.as_str()) == Some("LogicalDecoding"))
                    || (engine == "sqlserver" && item.get("method").and_then(|v| v.as_str()) == Some("C2") && item.get("feed").and_then(|v| v.as_str()) == Some("ChangeTracking"))))
    })
}

fn verify_r1_capture(config: &serde_json::Value, engine: &str, admission: &[u8], capturer: &[u8]) -> Result<(), &'static str> {
    if config.get("requested_write_class").and_then(|v| v.as_str()) != Some("R1") { return Ok(()); }
    let receipt: serde_json::Value = serde_json::from_slice(admission).map_err(|_| "nonadmitted")?;
    let captured: serde_json::Value = serde_json::from_slice(capturer).map_err(|_| "nonadmitted")?;
    let kinds: &[&str] = match engine {
        "postgresql" | "postgres" if receipt.get("capture_method").and_then(|v| v.as_str()) == Some("C1") => &["publication", "replication_slot"],
        "sqlserver" if receipt.get("capture_method").and_then(|v| v.as_str()) == Some("C2") => &["ct_database", "ct_table"],
        _ => return Err("nonadmitted"),
    };
    if receipt.get("write_class").and_then(|v| v.as_str()) != Some("R1")
        || receipt.get("write_attempts").and_then(|v| v.as_u64()) != Some(0)
        || receipt.get("dba_objects") != captured.get("dba_objects") { return Err("nonadmitted"); }
    let objects = receipt.get("dba_objects").and_then(|v| v.as_array()).ok_or("nonadmitted")?;
    let mut seen_sha: Option<String> = None;
    for kind in kinds {
        let matching: Vec<_> = objects.iter().filter(|v| v.get("kind").and_then(|v| v.as_str()) == Some(*kind)).collect();
        if matching.len() != 1 { return Err("nonadmitted"); }
        let item = matching[0];
        if item.get("created_by").and_then(|v| v.as_str()) != Some("dba")
            || item.get("observed").and_then(|v| v.as_bool()) != Some(true)
            || item.get("platform_created").and_then(|v| v.as_bool()) != Some(false)
            || item.get("platform_write_attempts").and_then(|v| v.as_u64()) != Some(0)
            || ["name", "risk", "rollback"].iter().any(|k| item.get(*k).and_then(|v| v.as_str()).is_none_or(str::is_empty)) { return Err("nonadmitted"); }
        let sha = required_hex(item, "observation_sha256")?;
        let bytes = read_hashed(&required_abs(item, "observation_path")?, &sha)?;
        if bytes.is_empty() { return Err("nonadmitted"); }
        if engine == "sqlserver" {
            if seen_sha.as_ref().is_some_and(|prior| prior != &sha) { return Err("nonadmitted"); }
            seen_sha = Some(sha);
            let facts: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "nonadmitted")?;
            ct_anchor(kind, item, &facts, &captured)?;
        }
    }
    Ok(())
}

fn ct_anchor(kind: &str, item: &serde_json::Value, facts: &serde_json::Value, captured: &serde_json::Value) -> Result<(), &'static str> {
    if facts.get("tds_query_executed").and_then(|v| v.as_bool()) != Some(true) { return Err("nonadmitted"); }
    let database = facts.get("database").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).ok_or("nonadmitted")?;
    let schema = facts.get("schema").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).ok_or("nonadmitted")?;
    let table = facts.get("table").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).ok_or("nonadmitted")?;
    let guid = facts.get("database_guid").and_then(|v| v.as_str()).ok_or("nonadmitted")?;
    let fork = facts.get("recovery_fork_guid").and_then(|v| v.as_str()).ok_or("nonadmitted")?;
    if !uuid_text(guid) || !uuid_text(fork) { return Err("nonadmitted"); }
    let object_id = facts.get("object_id").and_then(|v| v.as_i64()).filter(|n| *n > 0).ok_or("nonadmitted")?;
    let current = facts.get("current_version").and_then(|v| v.as_i64()).filter(|n| *n >= 0).ok_or("nonadmitted")?;
    let minimum = facts.get("min_valid_version").and_then(|v| v.as_i64()).filter(|n| *n >= 0).ok_or("nonadmitted")?;
    if minimum > current { return Err("nonadmitted"); }
    let name = item.get("name").and_then(|v| v.as_str()).ok_or("nonadmitted")?;
    let expect = if kind == "ct_database" { database.to_string() } else { format!("{schema}.{table}") };
    if name != expect { return Err("nonadmitted"); }
    let candidate = captured.get("candidate_position").ok_or("nonadmitted")?;
    if candidate.get("protocol").and_then(|v| v.as_str()) != Some("oo.mssql-preview.ct-position/v1")
        || candidate.get("database").and_then(|v| v.as_str()) != Some(database)
        || candidate.get("database_guid").and_then(|v| v.as_str()) != Some(guid)
        || candidate.get("recovery_fork_guid").and_then(|v| v.as_str()) != Some(fork) {
        return Err("nonadmitted");
    }
    let tables = candidate.get("tables").and_then(|v| v.as_array()).ok_or("nonadmitted")?;
    let matching: Vec<_> = tables.iter().filter(|row| row.get("schema").and_then(|v| v.as_str()) == Some(schema) && row.get("name").and_then(|v| v.as_str()) == Some(table)).collect();
    if matching.len() != 1 { return Err("nonadmitted"); }
    if matching[0].get("object_id").and_then(|v| v.as_i64()) != Some(object_id) || matching[0].get("version").and_then(|v| v.as_i64()) != Some(current) {
        return Err("nonadmitted");
    }
    Ok(())
}

fn uuid_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36 && bytes.iter().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => *byte == b'-',
        _ => byte.is_ascii_hexdigit(),
    })
}

fn contains_secret(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => text.contains("secret://"),
        serde_json::Value::Array(items) => items.iter().any(contains_secret),
        serde_json::Value::Object(map) => {
            map.contains_key("secret_ref") || map.values().any(contains_secret)
        }
        _ => false,
    }
}

fn required_hex(value: &serde_json::Value, name: &str) -> Result<String, &'static str> {
    let text = value
        .get(name)
        .and_then(|item| item.as_str())
        .ok_or("nonadmitted")?;
    if hex64(text) {
        Ok(text.to_string())
    } else {
        Err("digest_mismatch")
    }
}

fn required_abs(value: &serde_json::Value, name: &str) -> Result<String, &'static str> {
    let text = value
        .get(name)
        .and_then(|item| item.as_str())
        .ok_or("nonadmitted")?;
    if text.starts_with('/') && !text.contains('\0') {
        Ok(text.to_string())
    } else {
        Err("nonadmitted")
    }
}

fn optional_hex(value: &serde_json::Value, name: &str) -> Result<Option<String>, &'static str> {
    match value.get(name) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(item) => {
            let text = item.as_str().ok_or("digest_mismatch")?;
            if hex64(text) {
                Ok(Some(text.to_string()))
            } else {
                Err("digest_mismatch")
            }
        }
    }
}

fn optional_token(value: &serde_json::Value, name: &str) -> Result<Option<String>, &'static str> {
    match value.get(name) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(item) => Ok(Some(required_token_text(item.as_str().ok_or("schema")?)?)),
    }
}

fn required_token_text(text: &str) -> Result<String, &'static str> {
    if text.is_empty() || text.len() > 128 || text.bytes().any(|byte| byte < 0x20) {
        return Err("schema");
    }
    Ok(text.to_string())
}

fn required_token(value: &serde_json::Value, name: &str) -> Result<String, &'static str> {
    required_token_text(
        value
            .get(name)
            .and_then(|item| item.as_str())
            .ok_or("schema")?,
    )
}

fn opt_key(value: &serde_json::Value) -> Result<bool, &'static str> {
    match value.get("key") {
        None | Some(serde_json::Value::Null) => Ok(false),
        Some(item) => item.as_bool().ok_or("schema"),
    }
}

fn opt_document(value: &serde_json::Value) -> Result<bool, &'static str> {
    match value.get("document") {
        None | Some(serde_json::Value::Null) => Ok(false),
        Some(item) => item.as_bool().ok_or("schema"),
    }
}

fn opt_i32(value: &serde_json::Value, name: &str) -> Result<Option<i32>, &'static str> {
    match value.get(name) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(item) => item
            .as_i64()
            .and_then(|number| i32::try_from(number).ok())
            .map(Some)
            .ok_or("precision"),
    }
}

fn read_hashed(path: &str, expect: &str) -> Result<Vec<u8>, &'static str> {
    read_hashed_limit(path, expect, MAX_JSON)
}

fn read_capture_receipt(path: &str, expect: &str) -> Result<Vec<u8>, &'static str> {
    read_hashed_limit(path, expect, 4 * MAX_JSON)
}

fn read_hashed_limit(path: &str, expect: &str, limit: usize) -> Result<Vec<u8>, &'static str> {
    let bytes = read_capped_limit(Path::new(path), limit)?;
    if hex_bytes(&Sha256::digest(&bytes)) == expect {
        Ok(bytes)
    } else {
        Err("digest_mismatch")
    }
}

fn read_capped(path: &Path) -> Result<Vec<u8>, &'static str> {
    read_capped_limit(path, MAX_JSON)
}

fn read_capped_limit(path: &Path, limit: usize) -> Result<Vec<u8>, &'static str> {
    let meta = fs::metadata(path).map_err(|_| "nonadmitted")?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > limit as u64 {
        return Err("nonadmitted");
    }
    fs::read(path).map_err(|_| "nonadmitted")
}

fn hex64(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn sqlserver_r1_uses_admission_ct_kinds_without_weakening_observation() {
        let path = std::env::temp_dir().join(format!("oo-ct-binding-{}", std::process::id()));
        let facts = serde_json::json!({"database":"oo_dl01w1_09","database_guid":"EB4FDD2C-55EB-4858-9620-390EF5387B84","recovery_fork_guid":"645E9A69-D84E-4D9C-AFC1-DC615B29E71A","schema":"dbo","table":"oracle_row","object_id":901578250,"current_version":1,"min_valid_version":0,"snapshot_isolation":true,"tds_query_executed":true});
        let raw = serde_json::to_vec(&facts).unwrap();
        std::fs::write(&path, &raw).unwrap();
        let hash = hex_bytes(&Sha256::digest(&raw));
        let objects: Vec<_> = [("ct_database","oo_dl01w1_09"),("ct_table","dbo.oracle_row")].into_iter().map(|(kind,name)|
            serde_json::json!({"kind":kind,"created_by":"dba","observed":true,"platform_created":false,"platform_write_attempts":0,
                "name":name,"risk":"finite retention","rollback":"DBA disable after pause",
                "observation_path":&path,"observation_sha256":hash})).collect();
        let candidate = serde_json::json!({"protocol":"oo.mssql-preview.ct-position/v1","database":"oo_dl01w1_09","database_guid":"EB4FDD2C-55EB-4858-9620-390EF5387B84","recovery_fork_guid":"645E9A69-D84E-4D9C-AFC1-DC615B29E71A","tables":[{"schema":"dbo","name":"oracle_row","object_id":901578250,"version":1}]});
        let config=serde_json::json!({"requested_write_class":"R1"});
        let receipt=serde_json::json!({"capture_method":"C2","write_class":"R1","write_attempts":0,"dba_objects":objects,"candidate_position":candidate});
        let bytes=serde_json::to_vec(&receipt).unwrap();
        assert_eq!(verify_r1_capture(&config,"sqlserver",&bytes,&bytes),Ok(()));
        let mut missing=receipt.clone();missing["dba_objects"][0]["kind"]=serde_json::json!("anything");
        assert_eq!(verify_r1_capture(&config,"sqlserver",&serde_json::to_vec(&missing).unwrap(),&serde_json::to_vec(&missing).unwrap()),Err("nonadmitted"));
        missing=receipt.clone();missing["dba_objects"][0]["observation_sha256"]=serde_json::json!("00".repeat(32));
        assert!(verify_r1_capture(&config,"sqlserver",&serde_json::to_vec(&missing).unwrap(),&serde_json::to_vec(&missing).unwrap()).is_err());
        let junk = path.with_extension("junk");
        std::fs::write(&junk, b"actual-test-observation").unwrap();
        let junk_hash = hex_bytes(&Sha256::digest(b"actual-test-observation"));
        missing=receipt.clone();
        for index in 0..2 {
            missing["dba_objects"][index]["observation_path"]=serde_json::json!(&junk);
            missing["dba_objects"][index]["observation_sha256"]=serde_json::json!(&junk_hash);
        }
        assert_eq!(verify_r1_capture(&config,"sqlserver",&serde_json::to_vec(&missing).unwrap(),&serde_json::to_vec(&missing).unwrap()),Err("nonadmitted"));
        let mut drifted = facts.clone();
        drifted["current_version"]=serde_json::json!(9);
        let drifted_raw = serde_json::to_vec(&drifted).unwrap();
        std::fs::write(&path, &drifted_raw).unwrap();
        let drifted_hash = hex_bytes(&Sha256::digest(&drifted_raw));
        missing=receipt.clone();
        for index in 0..2 { missing["dba_objects"][index]["observation_sha256"]=serde_json::json!(&drifted_hash); }
        assert_eq!(verify_r1_capture(&config,"sqlserver",&serde_json::to_vec(&missing).unwrap(),&serde_json::to_vec(&missing).unwrap()),Err("nonadmitted"));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(junk).unwrap();
    }
    use super::*;

    const DIGEST: &str = "abababababababababababababababababababababababababababababababab";

    #[test]
    fn large_capture_receipt_keeps_other_control_files_capped() {
        let path = std::env::temp_dir().join(format!("oo-capture-receipt-{}", std::process::id()));
        let bytes = vec![b'x'; 2_329_469];
        std::fs::write(&path, &bytes).unwrap();
        let digest = hex_bytes(&Sha256::digest(&bytes));
        assert_eq!(read_hashed(path.to_str().unwrap(), &digest), Err("nonadmitted"));
        assert_eq!(read_capture_receipt(path.to_str().unwrap(), &digest), Ok(bytes));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn maria_tracking_capture_receipt_uses_the_scoped_limit() {
        let Ok(path) = std::env::var("OO_TEST_MARIA_CAPTURE_RECEIPT") else { return; };
        let digest = "34434c139e5c106cc99b41614cb775ee25b9d3db35bc42878761febdbfbbfa32";
        assert_eq!(read_hashed(&path, digest), Err("nonadmitted"));
        assert_eq!(read_capture_receipt(&path, digest).unwrap().len(), 2_329_469);
        assert_eq!(read_capture_receipt(&path, DIGEST), Err("digest_mismatch"));
    }

    #[test]
    fn c4_scope_rejects_partial_or_crossed_observation() {
        let digest = "ab".repeat(32);
        let binding = serde_json::json!({"source_engine":"clickhouse","source_id":"clickhouse_268","source_generation":1,"schema_digest":digest,"type_digest":digest});
        let objects = vec![BoundObject { logical_schema:"live".to_string(), logical_table:"readings".to_string(), row_count:2, arrow_sha256:digest.clone() }];
        let mut scope = serde_json::json!({
            "source_id":"clickhouse_268","source_generation":1,"logical_schema":"live","logical_table":"readings",
            "partition_id":"202401","observed_partition_ids":["202401","202403"],"complete":true,
            "table_epoch":"3cad2d69-4353-4ab2-88b0-dd2ef92ee466","schema_fingerprint":"reading_id UInt64",
            "schema_digest":digest,"type_digest":digest,"arrow_sha256":digest,"row_count":2,
            "current_signature":"202401|2|parts","mutations_pending":false
        });
        assert_eq!(validate_partition_scope(&scope, &binding, &objects), Ok(()));
        scope["observed_partition_ids"] = serde_json::json!(["202403","202401"]);
        assert_eq!(validate_partition_scope(&scope, &binding, &objects), Err("schema"));
        scope["observed_partition_ids"] = serde_json::json!(["202401","202403"]);
        scope["mutations_pending"] = serde_json::json!(true);
        assert_eq!(validate_partition_scope(&scope, &binding, &objects), Err("schema"));
        scope["mutations_pending"] = serde_json::json!(false);
        scope["arrow_sha256"] = serde_json::json!("cd".repeat(32));
        assert_eq!(validate_partition_scope(&scope, &binding, &objects), Err("schema"));
    }

    #[test]
    fn approved_driver_resource_is_the_declared_doris_reader() {
        const DORIS: &str = "d5dc8555ad76ecc9d4550ac5d2abd6b00bb2fee6ca5a17c7c206d294b206010c";
        assert!(driver_length_allowed(DORIS, 115_155_008));
        assert!(!driver_length_allowed(DORIS, 115_155_007));
        assert!(!driver_length_allowed(DORIS, 32 * 1024 * 1024));
        assert!(driver_length_allowed(DIGEST, 32 * 1024 * 1024));
        assert!(!driver_length_allowed(DIGEST, 32 * 1024 * 1024 + 1));
        assert!(!driver_length_allowed(DIGEST, 115_155_008));
        assert!(!driver_length_allowed(DORIS, 0));
        const OPENSEARCH: &str =
            "874d2588595d63b597139358b47e3097fcc5f45f772ffb5000ecd371e8e88b2e";
        assert!(driver_length_allowed(OPENSEARCH, 116_863_584));
        assert!(!driver_length_allowed(OPENSEARCH, 116_863_583));
        assert!(!driver_length_allowed(OPENSEARCH, 115_155_008));
        assert!(!driver_length_allowed(DIGEST, 116_863_584));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_E465));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_NATIVE_MINIMAL));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_PG_TRANSACTION));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_PG_PER_TABLE));
        const MARIA: &str = "7ee551ecadd2fb66e3dea5564dab7336cafcce0e6fb0dd57d5816a7685338474";
        assert!(driver_length_allowed(MARIA, 314_875_904));
        assert!(!driver_length_allowed(MARIA, 314_875_903));
        assert!(!driver_length_allowed(DIGEST, 314_875_904));
        const STAR_C4: &str = "683194dab7fd5ba58efb6da4009e82a8683ea3d07913153e8a7ac931a1caef4e";
        const DORIS_BASELINE: &str = "29fbbeb07880dccffbfe149262f99422e96b643c597b645bdaf43794b1c2734d";
        assert!(driver_length_allowed(STAR_C4, 113_981_880));
        assert!(!driver_length_allowed(STAR_C4, 113_981_879));
        assert!(driver_length_allowed(DORIS_BASELINE, 116_576_104));
        assert!(!driver_length_allowed(DORIS_BASELINE, 116_576_105));
        assert!(!driver_length_allowed(DIGEST, 113_981_880));
    }
    const ARROW: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
    const ARROW_SCHEMA: &str = "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef";

    fn arrows() -> Vec<String> {
        vec![ARROW.to_string()]
    }

    fn expect<'a>(
        engine: &'a str,
        arrows: &'a [String],
        declared: Option<&'a str>,
        run_id: Option<&'a str>,
    ) -> CaptureReceipt<'a> {
        CaptureReceipt {
            source_engine: engine,
            source_id: "oo04_t_batch",
            source_generation: 1,
            snapshot_kind: "scn",
            snapshot_value: "2175138",
            run_id,
            schema_digest: DIGEST,
            type_digest: DIGEST,
            config_digest: DIGEST,
            input_digest: DIGEST,
            catalog_sha256: DIGEST,
            registry_sha256: DIGEST,
            arrow_sha256: arrows,
            driver_digest: DIGEST,
            arrow_schema_sha256: ARROW_SCHEMA,
            declared_receipt_sha256: declared,
        }
    }

    fn capture_body(engine: &str, run_id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema": CAPTURE_RECEIPT_PROTOCOL,
            "source_engine": engine,
            "source_id": "oo04_t_batch",
            "source_generation": 1,
            "snapshot": {"kind": "scn", "value": "2175138"},
            "run_id": run_id,
            "schema_digest": DIGEST,
            "type_digest": DIGEST,
            "config_digest": DIGEST,
            "input_digest": DIGEST,
            "catalog_sha256": DIGEST,
            "registry_sha256": DIGEST,
            "arrow_sha256": [ARROW],
            "driver_digest": DIGEST,
            "arrow_schema_sha256": ARROW_SCHEMA
        }))
        .expect("receipt")
    }

    #[test]
    fn historical_receipt_is_clickhouse_or_opensearch_only() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../evidence/2ecaf08e-eb23-4b7a-a720-e07459e6f2a6/DL01/budget-report/native-readonly-completion/receipt.json",
        );
        let bytes = fs::read(&path).unwrap_or_else(|_| panic!("missing {}", path.display()));
        assert_eq!(
            hex_bytes(&Sha256::digest(&bytes)),
            HISTORICAL_ADMISSION_RECEIPT_SHA256
        );
        let listed = arrows();
        assert_eq!(
            check_admission_receipt(
                &bytes,
                &expect(
                    "oracle",
                    &listed,
                    Some(HISTORICAL_ADMISSION_RECEIPT_SHA256),
                    None
                )
            ),
            Err("cross_source")
        );
        assert_eq!(
            check_admission_receipt(&bytes, &expect("doris", &listed, None, None)),
            Err("cross_source")
        );
        assert_eq!(
            check_admission_receipt(&bytes, &expect("clickhouse", &listed, None, None)),
            Ok(AdmissionReceiptKind::HistoricalReadonly)
        );
        assert_eq!(
            check_admission_receipt(&bytes, &expect("opensearch", &listed, None, None)),
            Ok(AdmissionReceiptKind::HistoricalReadonly)
        );
    }

    #[test]
    fn capture_receipt_matches_one_source_and_refuses_reuse() {
        let body = capture_body("oracle", "run-2175138");
        let hash = hex_bytes(&Sha256::digest(&body));
        let listed = arrows();
        assert_eq!(
            check_admission_receipt(
                &body,
                &expect("oracle", &listed, Some(&hash), Some("run-2175138"))
            ),
            Ok(AdmissionReceiptKind::CaptureMatched {
                receipt_sha256: hash.clone()
            })
        );
        assert_eq!(
            check_admission_receipt(
                &body,
                &expect("clickhouse", &listed, Some(&hash), Some("run-2175138"))
            ),
            Err("cross_source")
        );
        assert_eq!(
            check_admission_receipt(
                &body,
                &expect("oracle", &listed, Some(&hash), Some("other-run"))
            ),
            Err("cross_source")
        );
        let drifted_digest = format!("ff{}", &DIGEST[2..]);
        let drifted = CaptureReceipt {
            source_engine: "oracle",
            source_id: "oo04_t_batch",
            source_generation: 1,
            snapshot_kind: "scn",
            snapshot_value: "2175138",
            run_id: Some("run-2175138"),
            schema_digest: &drifted_digest,
            type_digest: DIGEST,
            config_digest: DIGEST,
            input_digest: DIGEST,
            catalog_sha256: DIGEST,
            registry_sha256: DIGEST,
            arrow_sha256: &listed,
            driver_digest: DIGEST,
            arrow_schema_sha256: ARROW_SCHEMA,
            declared_receipt_sha256: Some(&hash),
        };
        assert_eq!(
            check_admission_receipt(&body, &drifted),
            Err("digest_mismatch")
        );
        let wrong_schema = "12".repeat(32);
        let schema_only = CaptureReceipt {
            source_engine: "oracle",
            source_id: "oo04_t_batch",
            source_generation: 1,
            snapshot_kind: "scn",
            snapshot_value: "2175138",
            run_id: Some("run-2175138"),
            schema_digest: DIGEST,
            type_digest: DIGEST,
            config_digest: DIGEST,
            input_digest: DIGEST,
            catalog_sha256: DIGEST,
            registry_sha256: DIGEST,
            arrow_sha256: &listed,
            driver_digest: DIGEST,
            arrow_schema_sha256: &wrong_schema,
            declared_receipt_sha256: Some(&hash),
        };
        assert_eq!(
            check_admission_receipt(&body, &schema_only),
            Err("digest_mismatch")
        );
        assert_eq!(
            check_admission_receipt(&body, &expect("oracle", &listed, None, Some("run-2175138"))),
            Err("digest_mismatch")
        );
        let mut secret = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        secret["note"] = serde_json::json!("secret://token");
        let secret_bytes = serde_json::to_vec(&secret).unwrap();
        let secret_hash = hex_bytes(&Sha256::digest(&secret_bytes));
        assert_eq!(
            check_admission_receipt(
                &secret_bytes,
                &expect("oracle", &listed, Some(&secret_hash), None)
            ),
            Err("nonadmitted")
        );
    }
}
