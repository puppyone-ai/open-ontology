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

#[cfg(test)]
#[path = "../tests/support/oracle_capacity_admission.rs"]
mod oracle_capacity_admission;

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
    let (Some(bin), Some(path), Some(digest)) = (bin, path, digest) else {
        return Err("nonadmitted");
    };
    if !bin.starts_with('/') || !path.starts_with('/') || !hex64(&digest) {
        return Err("nonadmitted");
    }
    let bytes = read_capped(Path::new(&path))?;
    if hex_bytes(&Sha256::digest(&bytes)) != digest {
        return Err("digest_mismatch");
    }
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
/// Linux Rust 1.94 `oo-source-admission` that parses SQL Server C2 CT facts.
/// Measured from `/tmp/oo-sqlserver-linux-target/release/oo-source-admission`.
pub const ADMISSION_BIN_SHA256_SQLSERVER_CT: &str =
    "9a739ca803f0d6e5b1ff5e466903d0d459f6ed176215f471da11dfe43c103b24";
/// Linux Rust 1.94 `oo-source-admission` with native MySQL/MariaDB GTID scope binding.
pub const ADMISSION_BIN_SHA256_GTID_SCOPE: &str =
    "84f6a9aa6a2ba072410460c7cf615ef316c8bf915b30f6bd30c4b535526333ae";

/// Trusted runtime allowlist. A manifest field cannot add a program.
/// Linux Rust 1.94 C1 baseline per-table schema and quiet barrier validator.
pub const ADMISSION_BIN_SHA256_C1_BASELINE_TYPED: &str =
    "c5c797dbd3348a3896bd78827dd7e770f97eedd58fa219d69a3826326d84d586";

pub const ADMISSION_BIN_SHA256_MYSQL_BASELINE_GTID: &str =
    "85cf8ad37a10ec91113dde35b1a51702fda90b1a65041df709dce89aab0ae480";
/// Frozen macOS validator for independently admitted C5 range parts.
pub const ADMISSION_BIN_SHA256_C5_RANGE_PART: &str =
    "946a4ff75f0936738310f677e806c66e02a0dfce82c5c127d28fc14629ee9d24";
/// Linux arm64 Rust 1.94 build of the same range-part admission boundary.
pub const ADMISSION_BIN_SHA256_C5_RANGE_PART_LINUX: &str =
    "33995cdc22a0814e468cc19a52cff58088bf6ec6692dbc6337385f1c9a43e8eb";
/// Linux Rust 1.94 `oo-source-admission` of T13c (chunked-typed/v2 binding of open-time digests,
/// per-table registry admission). Built from tools/source-admission before this loader (BUILD-AND-BASELINE.md 3.5).
pub const ADMISSION_BIN_SHA256_T13C: &str =
    "40f52d923aab6a75d67ad0d796320614d98efd390dbf453d75d557ba40cd6982";
/// Linux arm64 Rust 1.94 dev validator from 9ddf5a476, including source-audited MySQL full capture.
pub const ADMISSION_BIN_SHA256_MYSQL_FULL: &str =
    "3f8b2b3427733bfb0241712ff1a51a54b5bb5e3998b449decd4d9125b2f144ef";

pub const ADMISSION_BIN_SHA256_GRANT_ENFORCED: &str =
    "8de7d419ddd113738d83fa0b5077e8d757d6e2ab58578040e5262ffcdaef7527";

pub const ADMISSION_BIN_SHA256_C5_MULTIPART_ANCHOR: &str =
    "70f14fe31f151e5c505a16e6b1983a3537c81f165c84e0e20ca21475aeb7b757";

pub const ADMISSION_BIN_SHA256_C5_RANGE: &str =
    "f61711e3118cf125af588673596d256de2f5cd6374f69baeda40e5e91be9751c";

pub const ADMISSION_BIN_SHA256_C5_MERGE: &str =
    "4afd8fdbe68ddf8b0230716ff09b63fe648d0df1ce635365577bb9c7146230cb";

pub const ADMISSION_BIN_SHA256_RELEASE_1: &str =
    "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2";

/// Linux arm64 release validator with bounded, SHA-bound C5 membership files.
pub const ADMISSION_BIN_SHA256_C5_MEMBERS_FILE: &str =
    "2c431d8d9b71a2f1c8b2242453ce18a7e61300a179347987905f43220e9454b1";

/// Exact macOS arm64 validator built with Rust 1.98.1 for the owned C1
/// integration. No manifest field can extend this executable allowlist.
pub const ADMISSION_BIN_SHA256_MACOS_C1_20261027: &str =
    "79ef045b882a8afd77272ce2e00be1e5505906b29341e4613fe22031992c5866";
/// Tested macOS Oracle immutable-buffer admission; preserves prior validators.
pub const ADMISSION_BIN_SHA256_ORACLE_BUFFER_20261027: &str =
    "ba0d594d09aaeb9b82fba5164351711bb88cde4f618853e1c553e3321160250a";
/// Owned continuous-buffer validator, including a nonzero initial cursor.
pub const ADMISSION_BIN_SHA256_ORACLE_CONTINUOUS_20261027: &str =
    "e920f1ac8fa1da1757429643c688d69322a87c72eb511fb1af5c1e54cc013355";
/// Fixed Oracle SCN/range validator; no source-engine relabeling.
pub const ADMISSION_BIN_SHA256_ORACLE_C5_RANGE_20261027: &str =
    "85a7ae15105d41153e7c15f0b9dd95959ece36e8e21296bf4fcb6aa0aef52ecd";
/// Reviewed macOS immutable-buffer validator without the arbitrary million-ordinal cap.
/// Retained real Oracle capture decisions match the prior validator byte for byte.
pub const ADMISSION_BIN_SHA256_ORACLE_CAPACITY_20261030: &str =
    "063c97f05f939548c6fd2991a796fd6f53b08f07d59dc72d76855cc5a3e41565";

pub fn approved_admission_bin(digest: &str) -> bool {
    if digest == ADMISSION_BIN_SHA256_ORACLE_CAPACITY_20261030 {
        return true;
    }
    if digest == ADMISSION_BIN_SHA256_ORACLE_C5_RANGE_20261027 {
        return true;
    }
    if digest == ADMISSION_BIN_SHA256_ORACLE_CONTINUOUS_20261027 {
        return true;
    }
    if digest == ADMISSION_BIN_SHA256_ORACLE_BUFFER_20261027 {
        return true;
    }
    if digest == ADMISSION_BIN_SHA256_MACOS_C1_20261027 {
        return true;
    }
    if digest == ADMISSION_BIN_SHA256_C5_MEMBERS_FILE {
        return true;
    }
    if digest == "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2" {
        return true;
    }
    if digest == "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2" {
        return true;
    }
    if digest == "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2" {
        return true;
    }
    if digest == "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2" {
        return true;
    }
    if digest == "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2" {
        return true;
    }
    if digest == "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2" {
        return true;
    }
    if digest == "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2" {
        return true;
    }
    if digest == "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2" {
        return true;
    }
    if digest == "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2" {
        return true;
    }
    digest == ADMISSION_BIN_SHA256_RELEASE_1
        || digest == ADMISSION_BIN_SHA256_C5_MERGE
        || digest == ADMISSION_BIN_SHA256_C5_RANGE
        || digest == ADMISSION_BIN_SHA256_C5_MULTIPART_ANCHOR
        || digest == "aba75b8a879712b4afcf722e7f00e5641d0a875c2165300663ae0a86c1eceeac"
        || digest == ADMISSION_BIN_SHA256_GRANT_ENFORCED
        || digest == ADMISSION_BIN_SHA256
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
        || digest == ADMISSION_BIN_SHA256_SQLSERVER_CT
        || digest == ADMISSION_BIN_SHA256_C1_BASELINE_TYPED
        || digest == ADMISSION_BIN_SHA256_MYSQL_BASELINE_GTID
        || digest == ADMISSION_BIN_SHA256_C5_RANGE_PART
        || digest == ADMISSION_BIN_SHA256_C5_RANGE_PART_LINUX
        || digest == ADMISSION_BIN_SHA256_T13C
        || digest == ADMISSION_BIN_SHA256_MYSQL_FULL
        || digest == ADMISSION_BIN_SHA256_GTID_SCOPE
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
    pub multipart_scope: Option<serde_json::Value>,
    pub range_scope: Option<serde_json::Value>,
    /// Source-observed C4 scope, equal to the hash-bound capturer payload.
    pub partition_scope: Option<serde_json::Value>,
    pub transaction_scope: Option<serde_json::Value>,
    /// Registered Maria or MySQL GTID instance. Separate from a PostgreSQL xid.
    pub gtid_scope: Option<serde_json::Value>,
    /// Target-registered Mongo source instance; election term is never the epoch.
    pub source_instance: Option<serde_json::Value>,
    /// Epoch-free C1 bootstrap lineage and closed snapshot/CDC envelope.
    pub baseline_scope: Option<serde_json::Value>,
}

/// Read quality from the same hash-bound capturer that sealed the baseline.
pub(crate) fn baseline_quality(
    captured: &ResolvedCapture,
) -> Result<Vec<serde_json::Value>, &'static str> {
    if captured.baseline_scope.is_none() {
        return Ok(Vec::new());
    }
    if !matches!(captured.source_engine.as_str(), "mysql" | "oracle") || captured.objects.len() != 1
    {
        return Err("nonadmitted");
    }
    let (_, path, _) = operator_values();
    let bytes = read_capped(Path::new(&path.ok_or("nonadmitted")?))?;
    if hex_bytes(&Sha256::digest(&bytes)) != captured.registry_sha256 {
        return Err("digest_mismatch");
    }
    let registry: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "nonadmitted")?;
    let binding = registry.pointer("/bindings/0").ok_or("nonadmitted")?;
    let bytes = read_capture_receipt(
        &required_abs(binding, "capture_receipt_path")?,
        &required_hex(binding, "capture_receipt_sha256")?,
    )?;
    let receipt: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "nonadmitted")?;
    if receipt["source_id"] != captured.source_id
        || receipt.get("baseline_scope") != captured.baseline_scope.as_ref()
    {
        return Err("nonadmitted");
    }
    let object = &captured.objects[0];
    let table = receipt["tables"]
        .as_array()
        .ok_or("nonadmitted")?
        .iter()
        .find(|t| t["schema"] == object.logical_schema && t["name"] == object.logical_table)
        .ok_or("nonadmitted")?;
    table["quality_rejects"]
        .as_array()
        .cloned()
        .ok_or("nonadmitted")
}

/// Closed native products admitted by the real source capture boundary.
///
/// This list matches `native_arrow::recognized_engine`. A request cannot add one.
pub(crate) fn sealed_product(engine: &str) -> Result<(), &'static str> {
    if matches!(
        engine,
        "mysql"
            | "postgresql"
            | "postgres"
            | "sqlserver"
            | "oracle"
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
        multipart_scope: parsed.multipart_scope.clone(),
        range_scope: parsed.range_scope.clone(),
        partition_scope: parsed.partition_scope.clone(),
        transaction_scope: parsed.transaction_scope.clone(),
        gtid_scope: parsed.gtid_scope.clone(),
        source_instance: parsed.source_instance.clone(),
        baseline_scope: parsed.baseline_scope.clone(),
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
    multipart_scope: Option<serde_json::Value>,
    range_scope: Option<serde_json::Value>,
    partition_scope: Option<serde_json::Value>,
    transaction_scope: Option<serde_json::Value>,
    gtid_scope: Option<serde_json::Value>,
    source_instance: Option<serde_json::Value>,
    baseline_scope: Option<serde_json::Value>,
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
        && binding.get("input_digest").and_then(|value| value.as_str()) == Some(input_digest)
        && (!binding.get("gtid_scope").is_some_and(|scope| {
            scope.get("codec").and_then(|v| v.as_str()) == Some("mongo-cluster-time/v1")
        }) || (binding
            .get("source_instance")
            .is_some_and(|instance| validate_source_instance(instance, binding).is_ok())
            && binding["source_instance"]["source_epoch"]
                == binding["gtid_scope"]["source_epoch"]
            && binding["source_instance"]["measured_id"] == binding["gtid_scope"]["lineage"]));
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
        && binding.get("input_digest").and_then(|value| value.as_str()) == Some(input_digest)
        && (!binding.get("gtid_scope").is_some_and(|scope| {
            scope.get("codec").and_then(|v| v.as_str()) == Some("mongo-cluster-time/v1")
        }) || (binding
            .get("source_instance")
            .is_some_and(|instance| validate_source_instance(instance, binding).is_ok())
            && binding["source_instance"]["source_epoch"]
                == binding["gtid_scope"]["source_epoch"]
            && binding["source_instance"]["measured_id"] == binding["gtid_scope"]["lineage"]));
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
    let multipart_scope = binding.get("multipart_scope").cloned();
    if let Some(value) = &multipart_scope {
        let scope = crate::native_c5_multipart::Manifest::parse(value)?;
        if binding["source_engine"] != "mysql"
            || scope.source_id != binding["source_id"]
            || scope.source_generation != binding["source_generation"]
            || scope.schema_sha256 != binding["schema_digest"]
            || scope.occurrence != binding["run_id"]
            || parsed_objects.len() != 1
            || scope.schema != parsed_objects[0].logical_schema
            || scope.table != parsed_objects[0].logical_table
            || scope.parts.iter().map(|p| p.rows).sum::<u64>() != parsed_objects[0].row_count
        {
            return Err("multipart_scope");
        }
    }
    let range_scope = binding.get("range_scope").cloned();
    if let Some(value) = &range_scope {
        if value["protocol"] == crate::native_c5_parts::PART_SCOPE_PROTOCOL {
            let scope = crate::native_c5_parts::PartScope::parse(value)?;
            if scope.range.source_engine != binding["source_engine"]
                || scope.range.source_id != binding["source_id"]
                || scope.range.source_generation != binding["source_generation"]
                || serde_json::to_value(&scope.snapshot).map_err(|_| "c5_part_scope")?
                    != binding["snapshot"]
                || parsed_objects.len() != 1
                || scope.range.schema != parsed_objects[0].logical_schema
                || scope.range.table != parsed_objects[0].logical_table
                || scope.part.arrow.sha256 != parsed_objects[0].arrow_sha256
                || scope.part.row_count != parsed_objects[0].row_count
                || [
                    "multipart_scope",
                    "partition_scope",
                    "transaction_scope",
                    "gtid_scope",
                    "baseline_scope",
                    "source_instance",
                ]
                .iter()
                .any(|key| binding.get(*key).is_some())
            {
                return Err("c5_part_scope");
            }
        } else {
            let scope = crate::native_c5_range::RangeScope::parse(value)?;
            if scope.source_engine != binding["source_engine"]
                || scope.source_id != binding["source_id"]
                || scope.source_generation != binding["source_generation"]
                || parsed_objects.len() != 1
                || scope.schema != parsed_objects[0].logical_schema
                || scope.table != parsed_objects[0].logical_table
                || scope.arrow_sha256 != parsed_objects[0].arrow_sha256
                || scope.row_count != parsed_objects[0].row_count
                || [
                    "partition_scope",
                    "transaction_scope",
                    "gtid_scope",
                    "baseline_scope",
                    "source_instance",
                ]
                .iter()
                .any(|key| binding.get(*key).is_some())
            {
                return Err("c5_range_scope");
            }
        }
    }
    let partition_scope = binding.get("partition_scope").cloned();
    if let Some(scope) = partition_scope.as_ref() {
        if !matches!(
            binding
                .get("source_engine")
                .and_then(serde_json::Value::as_str),
            Some("clickhouse" | "doris" | "starrocks")
        ) {
            return Err("schema");
        }
        validate_partition_scope(scope, binding, &parsed_objects)?;
    }
    let transaction_scope = binding.get("transaction_scope").cloned();
    if transaction_scope.is_some()
        && (!matches!(
            binding
                .get("source_engine")
                .and_then(serde_json::Value::as_str),
            Some("postgres" | "postgresql")
        ) || partition_scope.is_some())
    {
        return Err("schema");
    }
    let gtid_scope = binding.get("gtid_scope").cloned();
    if let Some(scope) = gtid_scope.as_ref() {
        if transaction_scope.is_some() || partition_scope.is_some() {
            return Err("schema");
        }
        validate_gtid_scope(scope, binding)?;
    }
    let source_instance = binding.get("source_instance").cloned();
    if let Some(instance) = source_instance.as_ref() {
        if transaction_scope.is_some()
            || partition_scope.is_some()
            || !matches!(
                binding
                    .get("source_engine")
                    .and_then(serde_json::Value::as_str),
                Some("mongodb" | "mysql" | "mariadb")
            )
        {
            return Err("schema");
        }
        validate_source_instance(instance, binding)?;
    }
    let baseline_scope = binding.get("baseline_scope").cloned();
    if let Some(scope) = baseline_scope.as_ref() {
        if gtid_scope.is_some()
            || source_instance.is_some()
            || transaction_scope.is_some()
            || partition_scope.is_some()
        {
            return Err("schema");
        }
        validate_c1_baseline_scope(scope, binding, &parsed_objects)?;
    }
    let mut parsed_fields = Vec::with_capacity(fields.len());
    for field in fields {
        let source_oid = match field.get("source_oid") {
            None => None,
            Some(value) => {
                Some(u32::try_from(value.as_u64().ok_or("schema")?).map_err(|_| "schema")?)
            }
        };
        let source_typmod = opt_i32(field, "source_typmod")?;
        let source_format = match field.get("source_format") {
            None => None,
            Some(value) => Some(
                value
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .ok_or("schema")?
                    .to_owned(),
            ),
        };
        if (source_oid.is_some() || source_typmod.is_some() || source_format.is_some())
            && (!matches!(
                binding
                    .get("source_engine")
                    .and_then(serde_json::Value::as_str),
                Some("postgres" | "postgresql")
            ) || source_oid.is_none()
                || source_typmod.is_none()
                || source_format.is_none())
        {
            return Err("schema");
        }
        let bson_type = match field.get("bson_type") {
            None => None,
            Some(value) => {
                if binding
                    .get("source_engine")
                    .and_then(serde_json::Value::as_str)
                    != Some("mongodb")
                {
                    return Err("schema");
                }
                let variant = value
                    .get("variant")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("schema")?;
                let subtype = value.get("subtype").and_then(serde_json::Value::as_str);
                if variant.len() != 2
                    || !variant.bytes().all(|byte| byte.is_ascii_hexdigit())
                    || subtype.is_some_and(|item| {
                        item.len() != 2 || !item.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                    || (variant == "05") != subtype.is_some()
                {
                    return Err("schema");
                }
                Some(BsonType {
                    variant: variant.to_owned(),
                    subtype: subtype.map(str::to_owned),
                })
            }
        };
        if binding
            .get("source_engine")
            .and_then(serde_json::Value::as_str)
            == Some("mongodb")
            && matches!(
                required_token(field, "arrow_type")?.as_str(),
                "fixedsizebinary(12)" | "fixedsizebinary(16)"
            )
            && bson_type.is_none()
        {
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
        multipart_scope,
        range_scope,
        partition_scope,
        transaction_scope,
        gtid_scope,
        source_instance,
        baseline_scope,
    })
}

fn validate_gtid_scope(
    scope: &serde_json::Value,
    binding: &serde_json::Value,
) -> Result<(), &'static str> {
    const KEYS: &[&str] = &[
        "codec",
        "lineage",
        "source_epoch",
        "applied_pos",
        "txn_id",
        "measured_id",
        "config_digest",
        "schema_digest",
    ];
    let object = scope.as_object().ok_or("schema")?;
    if object.len() != KEYS.len() || KEYS.iter().any(|key| !object.contains_key(*key)) {
        return Err("schema");
    }
    let engine = binding
        .get("source_engine")
        .and_then(serde_json::Value::as_str)
        .ok_or("schema")?;
    let codec = scope["codec"].as_str().ok_or("schema")?;
    let lineage = scope["lineage"].as_str().ok_or("schema")?;
    let measured = scope["measured_id"].as_str().ok_or("schema")?;
    let pos = scope["applied_pos"].as_str().ok_or("schema")?;
    let txn = scope["txn_id"].as_str().ok_or("schema")?;
    let epoch = scope["source_epoch"]
        .as_i64()
        .filter(|value| (1..=i64::from(i32::MAX)).contains(value))
        .ok_or("schema")?;
    let config = scope["config_digest"].as_str().ok_or("schema")?;
    let schema = scope["schema_digest"].as_str().ok_or("schema")?;
    if !hex64(config)
        || !hex64(schema)
        || binding
            .get("config_digest")
            .and_then(serde_json::Value::as_str)
            != Some(config)
        || binding
            .get("schema_digest")
            .and_then(serde_json::Value::as_str)
            != Some(schema)
        || measured != lineage
    {
        return Err("schema");
    }
    if lineage.contains(',')
        || pos.contains(',')
        || txn.contains(',')
        || txn.contains('/')
        || pos.contains('/')
        || pos.contains(':')
    {
        return Err("schema");
    }
    let _ = epoch;
    match (engine, codec) {
        ("mariadb", "mariadb-gtid/v1") => {
            let (domain, server) = maria_binding_lineage(lineage).ok_or("schema")?;
            if !gtid_digits(pos, true) || txn != format!("{domain}-{server}-{pos}") {
                return Err("schema");
            }
        }
        ("mysql", "mysql-gtid/v1") => {
            let uuid = lineage
                .strip_prefix("mysql.")
                .filter(|text| mysql_binding_uuid(text))
                .ok_or("schema")?;
            if !gtid_digits(pos, false) || txn != format!("{uuid}:{pos}") {
                return Err("schema");
            }
        }
        _ => return Err("schema"),
    }
    Ok(())
}

fn validate_c1_baseline_scope(
    scope: &serde_json::Value,
    binding: &serde_json::Value,
    objects: &[BoundObject],
) -> Result<(), &'static str> {
    if scope["protocol"].as_str() == Some(C1_BASELINE_SCOPE_V2) {
        return validate_c1_baseline_scope_v2(scope, binding, objects);
    }
    const KEYS: &[&str] = &[
        "protocol",
        "engine",
        "lineage",
        "l0",
        "l_end",
        "complete",
        "read_fence_digest",
        "config_digest",
        "schema_digest",
        "capture_digest",
        "consistency_grade",
        "cross_table_consistent",
        "source_identity",
        "tables",
    ];
    let object = scope.as_object().ok_or("schema")?;
    let oracle = scope["engine"] == "oracle";
    if oracle {
        crate::c1_buffer_position::validate_scope(scope)?;
    }
    if object.len() != KEYS.len() + usize::from(oracle)
        || KEYS.iter().any(|key| !object.contains_key(*key))
        || scope["protocol"]
            != if oracle {
                crate::c1_buffer_position::SCOPE
            } else {
                "oo.source-admission.c1-baseline-scope/v1"
            }
        || scope["engine"] != binding["source_engine"]
        || !matches!(
            scope["engine"].as_str(),
            Some("mysql" | "mariadb" | "mongodb" | "oracle")
        )
        || scope["complete"] != true
        || scope["config_digest"] != binding["config_digest"]
        || !scope["schema_digest"].as_str().is_some_and(hex64)
        || !matches!(
            scope["consistency_grade"].as_str(),
            Some("T" | "S1" | "S0" | "B")
        )
        || scope["cross_table_consistent"] != (scope["consistency_grade"] == "T")
        || !scope["read_fence_digest"].as_str().is_some_and(hex64)
        || !scope["capture_digest"].as_str().is_some_and(hex64)
    {
        return Err("schema");
    }
    let lineage = scope["lineage"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .ok_or("schema")?;
    let l0 = scope["l0"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 256)
        .ok_or("schema")?;
    let l_end = scope["l_end"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 256)
        .ok_or("schema")?;
    if l0.contains(',') || l_end.contains(',') {
        return Err("schema");
    }
    let identity = scope["source_identity"].as_object().ok_or("schema")?;
    if identity.len() != 4
        || !["engine", "measured_id", "shard_id", "owner_token"]
            .iter()
            .all(|k| identity.contains_key(*k))
        || identity["engine"] != scope["engine"]
        || identity["measured_id"].as_str() != Some(lineage)
        || !identity["shard_id"]
            .as_str()
            .is_some_and(|v| !v.is_empty() && v.len() <= 128)
        || !identity["owner_token"].as_str().is_some_and(binding_uuid)
    {
        return Err("schema");
    }
    let tables = scope["tables"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 64)
        .ok_or("schema")?;
    let mut previous: Option<(&str, &str)> = None;
    let mut matched = 0usize;
    for table in tables {
        let schema = table["schema"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or("schema")?;
        let name = table["table"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or("schema")?;
        if previous.is_some_and(|old| old >= (schema, name))
            || table["row_count"].as_u64().is_none()
            || !table["arrow_sha256"].as_str().is_some_and(hex64)
            || !table["schema_sha256"].as_str().is_some_and(hex64)
        {
            return Err("schema");
        }
        if let Some(object) = objects
            .iter()
            .find(|item| item.logical_schema == schema && item.logical_table == name)
        {
            matched += 1;
            if table["schema_sha256"] != binding["schema_digest"]
                || object.row_count != table["row_count"].as_u64().unwrap_or(u64::MAX)
                || object.arrow_sha256 != table["arrow_sha256"].as_str().unwrap_or_default()
            {
                return Err("digest_mismatch");
            }
        }
        previous = Some((schema, name));
    }
    if objects.len() != 1 || matched != 1 {
        return Err("schema");
    }
    Ok(())
}

const C1_BASELINE_SCOPE_V2: &str = "oo.source-admission.c1-baseline-scope/v2";

fn validate_c1_baseline_scope_v2(
    scope: &serde_json::Value,
    binding: &serde_json::Value,
    objects: &[BoundObject],
) -> Result<(), &'static str> {
    const KEYS: &[&str] = &[
        "protocol",
        "engine",
        "lineage",
        "l0",
        "l0_pos",
        "l_end",
        "l_end_pos",
        "complete",
        "config_digest",
        "schema_digest",
        "consistency_grade",
        "cross_table_consistent",
        "composition_rule",
        "bucket_bits",
        "cdc_batches",
        "source_identity",
        "tables",
        "expected_rows",
        "expected_composed_digest",
    ];
    let object = scope.as_object().ok_or("schema")?;
    if object.len() != KEYS.len()
        || KEYS.iter().any(|key| !object.contains_key(*key))
        || scope["engine"].as_str() != Some("mysql")
        || binding["source_engine"].as_str() != Some("mysql")
        || scope["complete"] != true
        || scope["config_digest"] != binding["config_digest"]
        || !scope["schema_digest"].as_str().is_some_and(hex64)
        || scope["consistency_grade"].as_str() != Some("T")
        || scope["cross_table_consistent"] != true
        || scope["composition_rule"].as_str() != Some("c1-compose/v2")
        || scope["bucket_bits"].as_u64() != Some(12)
        || !scope["expected_composed_digest"]
            .as_str()
            .is_some_and(hex64)
    {
        return Err("schema");
    }
    let lineage = scope["lineage"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .ok_or("schema")?;
    let measured = lineage
        .strip_prefix("mysql.")
        .filter(|u| binding_uuid(u))
        .ok_or("schema")?;
    let l0_pos = scope["l0_pos"].as_u64().ok_or("schema")?;
    let l_end_pos = scope["l_end_pos"].as_u64().ok_or("schema")?;
    if l_end_pos < l0_pos
        || scope["l0"].as_str() != Some(format!("{measured}:1-{l0_pos}").as_str())
        || scope["l_end"].as_str() != Some(format!("{measured}:1-{l_end_pos}").as_str())
    {
        return Err("schema");
    }
    let identity = scope["source_identity"].as_object().ok_or("schema")?;
    if identity.len() != 4
        || identity.get("engine").and_then(|v| v.as_str()) != Some("mysql")
        || identity.get("measured_id").and_then(|v| v.as_str()) != Some(lineage)
        || !identity
            .get("shard_id")
            .and_then(|v| v.as_str())
            .is_some_and(|v| !v.is_empty() && v.len() <= 128)
        || !identity
            .get("owner_token")
            .and_then(|v| v.as_str())
            .is_some_and(binding_uuid)
    {
        return Err("schema");
    }
    let batches = &scope["cdc_batches"];
    if !batches.is_object()
        || !batches["sha256"].as_str().is_some_and(hex64)
        || !batches["chain_sha256"].as_str().is_some_and(hex64)
        || batches["count"].as_u64().is_none()
    {
        return Err("schema");
    }
    let tables = scope["tables"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 64)
        .ok_or("schema")?;
    let mut previous: Option<(&str, &str)> = None;
    let mut total: u64 = 0;
    let mut matched = 0usize;
    for table in tables {
        let schema = table["schema"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or("schema")?;
        let name = table["table"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or("schema")?;
        let rows = table["expected_rows"].as_u64().ok_or("schema")?;
        if previous.is_some_and(|old| old >= (schema, name))
            || table["fill_rows"].as_u64().is_none()
            || table["cdc_rows"].as_u64().is_none()
            || !table["parts_digest"].as_str().is_some_and(hex64)
            || !table["schema_sha256"].as_str().is_some_and(hex64)
            || !table["expected_composed_digest"]
                .as_str()
                .is_some_and(hex64)
        {
            return Err("schema");
        }
        if tables.len() == 1
            && table["expected_composed_digest"] != scope["expected_composed_digest"]
        {
            return Err("schema");
        }
        total = total.checked_add(rows).ok_or("schema")?;
        if let Some(object) = objects
            .iter()
            .find(|item| item.logical_schema == schema && item.logical_table == name)
        {
            matched += 1;
            if table["schema_sha256"] != binding["schema_digest"] || object.row_count != rows {
                return Err("digest_mismatch");
            }
        }
        previous = Some((schema, name));
    }
    if scope["expected_rows"].as_u64() != Some(total) || objects.len() != 1 || matched != 1 {
        return Err("schema");
    }
    Ok(())
}

fn validate_source_instance(
    instance: &serde_json::Value,
    binding: &serde_json::Value,
) -> Result<(), &'static str> {
    const KEYS: &[&str] = &[
        "protocol",
        "source",
        "engine",
        "measured_id",
        "source_epoch",
        "config_digest",
        "schema_digest",
        "shard_id",
        "owner_token",
        "lease_id",
        "replay",
    ];
    let object = instance.as_object().ok_or("schema")?;
    if object.len() != KEYS.len()
        || KEYS.iter().any(|key| !object.contains_key(*key))
        || instance["protocol"] != "oo.pipeline.native-source-instance/v1"
        || instance["source"] != binding["source_id"]
        || instance["engine"] != binding["source_engine"]
        || !matches!(
            instance["engine"].as_str(),
            Some("mongodb" | "mysql" | "mariadb")
        )
        || instance["source_epoch"]
            .as_i64()
            .is_none_or(|epoch| epoch < 1 || epoch > i64::from(i32::MAX))
        || !instance["measured_id"]
            .as_str()
            .is_some_and(|v| !v.is_empty() && v.len() <= 128)
        || (binding.get("gtid_scope").is_some()
            && (instance["measured_id"] != binding["gtid_scope"]["lineage"]
                || instance["source_epoch"] != binding["gtid_scope"]["source_epoch"]))
        || (binding.get("baseline_scope").is_some()
            && (instance["measured_id"] != binding["baseline_scope"]["lineage"]
                || instance["engine"] != binding["baseline_scope"]["engine"]))
        || instance["config_digest"] != binding["config_digest"]
        || instance["schema_digest"] != binding["schema_digest"]
        || !instance["shard_id"]
            .as_str()
            .is_some_and(|v| !v.is_empty() && v.len() <= 128)
        || !instance["owner_token"].as_str().is_some_and(binding_uuid)
        || !instance["lease_id"].as_str().is_some_and(binding_uuid)
        || instance["replay"].as_bool().is_none()
    {
        return Err("schema");
    }
    Ok(())
}

fn binding_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit() && !b.is_ascii_uppercase()
            }
        })
}

fn maria_binding_lineage(lineage: &str) -> Option<(u32, u32)> {
    let rest = lineage.strip_prefix('d')?;
    let (domain, server) = rest.split_once(".s")?;
    let domain = canonical_binding_u32(domain)?;
    let server = canonical_binding_u32(server).filter(|value| *value != 0)?;
    Some((domain, server))
}

fn canonical_binding_u32(text: &str) -> Option<u32> {
    let bytes = text.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 10
        || !bytes.iter().all(|byte| byte.is_ascii_digit())
        || (bytes.len() > 1 && bytes[0] == b'0')
    {
        return None;
    }
    text.parse().ok()
}

fn gtid_digits(pos: &str, zero_ok: bool) -> bool {
    let bytes = pos.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 20
        && bytes.iter().all(|byte| byte.is_ascii_digit())
        && (bytes.len() == 1 || bytes[0] != b'0')
        && pos.parse::<u64>().is_ok()
        && (zero_ok || pos != "0")
}

fn mysql_binding_uuid(text: &str) -> bool {
    let mut parts = text.split('-');
    for width in [8, 4, 4, 4, 12] {
        let Some(part) = parts.next() else {
            return false;
        };
        if part.len() != width
            || !part
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return false;
        }
    }
    parts.next().is_none()
}

fn validate_partition_scope(
    scope: &serde_json::Value,
    binding: &serde_json::Value,
    objects: &[BoundObject],
) -> Result<(), &'static str> {
    const KEYS: &[&str] = &[
        "source_id",
        "source_generation",
        "logical_schema",
        "logical_table",
        "partition_id",
        "observed_partition_ids",
        "complete",
        "table_epoch",
        "schema_fingerprint",
        "schema_digest",
        "type_digest",
        "arrow_sha256",
        "row_count",
        "current_signature",
        "mutations_pending",
    ];
    const OLAP_KEYS: &[&str] = &[
        "source_engine",
        "key_model",
        "routing_column",
        "partition_range",
        "native_partition_id",
        "native_partition_version",
    ];
    let olap = matches!(
        binding["source_engine"].as_str(),
        Some("doris" | "starrocks")
    );
    if !olap && binding["source_engine"] != "clickhouse" {
        return Err("schema");
    }
    let object = scope.as_object().ok_or("schema")?;
    if object.len() != KEYS.len() + if olap { OLAP_KEYS.len() } else { 0 }
        || KEYS.iter().any(|key| !object.contains_key(*key))
        || (olap && OLAP_KEYS.iter().any(|key| !object.contains_key(*key)))
        || objects.len() != 1
        || scope["source_id"] != binding["source_id"]
        || scope["source_generation"] != binding["source_generation"]
        || scope["schema_digest"] != binding["schema_digest"]
        || scope["type_digest"] != binding["type_digest"]
        || scope["logical_schema"] != objects[0].logical_schema
        || scope["logical_table"] != objects[0].logical_table
        || scope["arrow_sha256"] != objects[0].arrow_sha256
        || scope["row_count"].as_u64() != Some(objects[0].row_count)
        || scope["complete"] != true
        || scope["mutations_pending"] != false
    {
        return Err("schema");
    }
    for key in ["partition_id", "table_epoch", "schema_fingerprint"] {
        if scope[key].as_str().is_none_or(str::is_empty) {
            return Err("schema");
        }
    }
    if olap {
        let native_id = scope["native_partition_id"].as_str().ok_or("schema")?;
        let signature = scope["current_signature"].as_str().ok_or("schema")?;
        if scope["source_engine"] != binding["source_engine"]
            || !matches!(
                scope["key_model"].as_str(),
                Some("duplicate" | "unique" | "primary")
            )
            || scope["routing_column"].as_str().is_none_or(|value| {
                value.is_empty()
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            })
            || !partition_decimal(native_id)
            || !scope["native_partition_version"]
                .as_str()
                .is_some_and(partition_decimal)
            || !scope["table_epoch"].as_str().is_some_and(partition_digest)
            || !scope["schema_fingerprint"]
                .as_str()
                .is_some_and(partition_digest)
            || !scope["partition_range"]
                .as_object()
                .is_some_and(partition_range)
            || !signature
                .strip_prefix(&format!("{native_id}|"))
                .is_some_and(partition_digest)
        {
            return Err("schema");
        }
    }
    if !scope["current_signature"].is_null()
        && scope["current_signature"]
            .as_str()
            .is_none_or(str::is_empty)
    {
        return Err("schema");
    }
    if scope["current_signature"].is_null() && objects[0].row_count != 0 {
        return Err("schema");
    }
    let ids = scope["observed_partition_ids"].as_array().ok_or("schema")?;
    if ids.is_empty() || ids.len() > 1024 {
        return Err("schema");
    }
    let mut previous = "";
    for id in ids {
        let value = id
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or("schema")?;
        if value <= previous {
            return Err("schema");
        }
        previous = value;
    }
    if !ids.iter().any(|id| id == &scope["partition_id"]) {
        return Err("schema");
    }
    Ok(())
}

fn partition_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn partition_decimal(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|number| number > 0)
}

fn partition_range(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    if object.len() != 2 {
        return false;
    }
    let (Some(lower), Some(upper)) = (
        object.get("lower").and_then(serde_json::Value::as_str),
        object.get("upper").and_then(serde_json::Value::as_str),
    ) else {
        return false;
    };
    let iso = |date: &str| {
        date.len() == 10
            && date.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 4 | 7) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit()
                }
            })
    };
    iso(lower) && iso(upper) && lower < upper
}

/// Grade and audit taken from a frozen server registration, not from the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerRegistration {
    pub fields: Vec<BoundField>,
    pub multipart_scope: Option<serde_json::Value>,
    pub range_scope: Option<serde_json::Value>,
    pub partition_scope: Option<serde_json::Value>,
    pub transaction_scope: Option<serde_json::Value>,
    pub gtid_scope: Option<serde_json::Value>,
    pub source_instance: Option<serde_json::Value>,
    /// Epoch-free C1 bootstrap lineage admitted before target registration.
    pub baseline_scope: Option<serde_json::Value>,
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
    if binding.get("baseline_scope").is_some() {
        let _ = parse_binding(binding)?;
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
        multipart_scope: binding.get("multipart_scope").cloned(),
        range_scope: binding.get("range_scope").cloned(),
        partition_scope: None,
        transaction_scope: binding.get("transaction_scope").cloned(),
        gtid_scope: binding.get("gtid_scope").cloned(),
        source_instance: binding.get("source_instance").cloned(),
        baseline_scope: binding.get("baseline_scope").cloned(),
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

#[cfg(test)]
const MAX_DRIVER: u64 = 32 * 1024 * 1024;

/// Approved driver resources. Each pair is a digest and its exact byte length.
///
/// Historical exact-size prechecks remain for pinned releases. Other artifacts
/// are hashed with bounded memory, not rejected because a debug build is larger.
const APPROVED_DRIVER_RESOURCES: &[(&str, u64)] = &[
    (
        "f9933cd47cda17ba623a087bb04149cd935e7d85d74fdda93d8883dbb811f83a",
        49_929_568,
    ),
    (ADMISSION_BIN_SHA256_C5_RANGE_PART_LINUX, 12_875_168),
    // Frozen production CLI corresponding to the owned growing-range source qualification.
    (
        "33c7698775a142a0bc846992c6453f5ddbbe8874e4f0da6dbbfa5ea70a11c281",
        60_695_424,
    ),
    // Fixed MySQL typed-delta/recovery producer; exact bytes, not a larger global cap.
    (
        "6c60fc489c40f82ea7662478fd8b4f77978bf07eafd8ef31870f49e6ebc5dfd8",
        68_937_024,
    ),
    (
        "3eef5786e2f03593ce48de9b818a125db6e8b48820f297b183019473276f277a",
        68_939_152,
    ),
    (
        "79efdf5187dab0ad92f13249d7001379030b734722760821c9555e74a8bbf1a7",
        59_708_480,
    ),
    (
        "bec48412b85bf8627f4707fe1c61ad8658b298ef6fda35b568e231d2b662a176",
        60_108_320,
    ),
    (
        "20e72de2aa1889c1e6db467e905b79a1f71c390018e4d22a9d23021ac76241f5",
        60_653_216,
    ),
    (
        "765646194e9a5523b620f01b12b3109f90b5dd35a9770f6b9d1a8fd574f44a5c",
        60_858_288,
    ),
    // Frozen stripped macOS C1 v3 producer; mysql-integration/frozen-runtime.
    // The default 32 MiB cap stays in force for every other binary.
    (
        "80c9d4b21d11fa39387e909e9d864a9fc04463305e2777713b86de27e4a79a80",
        58_994_496,
    ),
    (
        "772ecb51a6904ba2db01f3ecfff1cfe4046945c1e0585f008559e607d321c2b2",
        58_994_496,
    ),
    (
        "9ad546ac9c12c44c7e40b679df4556f89d53376e3644a98e4bafc1795a9c5806",
        24_095_272,
    ),
    // release builds from 7fd3ef2f2042c17c1771532e7bec789f85ae9713; Linux arm64, Rust 1.94.
    (
        "7461fd0fb3669e66cb15ec467510136b06f943013c9650784795a6ec01ae1bcc",
        24094928,
    ),
    (
        "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2",
        5654928,
    ),
    // release builds from 159bfe60edd2c4528d6f3128817ed07235862ea6; Linux arm64, Rust 1.94.
    (
        "affd22035f5acafdd2423d7ea6d9570baec7f2e8d178a52f1dc82409543323b6",
        24094952,
    ),
    (
        "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2",
        5654928,
    ),
    // release builds from 942fbfcd1f7b8f619600397b98219daebfa01575; Linux arm64, Rust 1.94.
    (
        "df7741338bbc4292571b109e35ca7385cc84561b224f855eea79e39360360fa0",
        24094952,
    ),
    (
        "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2",
        5654928,
    ),
    // release builds from 3f9a762e9f28245f9947e3d530edc3abe404e586; Linux arm64, Rust 1.94.
    (
        "261cd311d2060aed44024107c5960fb94473a9347b7094f246e8d8ddc6b8df3d",
        24095008,
    ),
    (
        "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2",
        5654928,
    ),
    // release builds from 80ab9d32d79f9d789efb01dff9c96b34b9f8d316; Linux arm64, Rust 1.94.
    (
        "c3fd18889526249824830768a4c68168f3570e760d19bd1a982994ee3b676199",
        24096736,
    ),
    (
        "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2",
        5654928,
    ),
    // release builds from 163dccd63b6a1e1faaf099ca2e11f7f58664e1a5; Linux arm64, Rust 1.94.
    (
        "73b72dea0301ca6a959d7e04e2fe6f9f0b71fff902493f3bdcd7b39c7db08f75",
        24162360,
    ),
    (
        "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2",
        5654928,
    ),
    // release builds from e561cfff83c3b374d75bd1d9bb733b3c5a6b8757; Linux arm64, Rust 1.94.
    (
        "a002a76aa7b05d0b65bad9461af3ed35c5c552df1f3c1a57b3fd4eb01c9bb302",
        24162376,
    ),
    (
        "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2",
        5654928,
    ),
    // release builds from 3018f02954e73fc79df6e024a96e24e21da9f04c; Linux arm64, Rust 1.94.
    (
        "cd3e4c0d4148a0dd3c98bdc6b9f1d6b307a7f4b08064fbf2a0b0ae993d382175",
        24162352,
    ),
    (
        "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2",
        5654928,
    ),
    // release builds from 750cf05a5d7d8c58c2f5ee928fca8f6af84b5e88; Linux arm64, Rust 1.94.
    (
        "3c981b095cab2c90d7f9ef3066e15731ad5a1582ebfd013890420868c82d14b5",
        24162360,
    ),
    (
        "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2",
        5654928,
    ),
    // release builds from 2edc62bd2 (type_policy, wide schemas, audit cells); Linux arm64, Rust 1.94.
    (
        "a6d307e42130a95f879980c6c41fcfcef92a0b7bdc25d121d67edea3c3a5bbc2",
        24096584,
    ),
    (ADMISSION_BIN_SHA256_RELEASE_1, 5654928),
    // mysql-preview with session sql_mode/time_zone init, built from 5cf200e97 (Linux arm64, Rust 1.94).
    (
        "c9a41a434b70c6626dd98efc5743ab26d56b48bab4bcaf0bd32290f7bf6a5aab",
        365_673_136,
    ),
    (
        "5251dc061491f34dd65e442695865d7a055e5457d2b4caf53aad054d57412256",
        325_704_944,
    ),
    (ADMISSION_BIN_SHA256_C5_MERGE, 68_556_064),
    (
        "9ff6e76e0b434f340daf889b7b319750e0f3369421106d7bf40c4df0751e9602",
        67082960,
    ),
    (ADMISSION_BIN_SHA256_C5_RANGE, 68_115_528),
    (
        "58c2027585336b1ca10cc0c7e042cfffa5f75b0df711cc1fd6ce77ba70c93093",
        49_264_152,
    ),
    (
        "96810e8bbf548ae892809c3f9961c7305fe46ee5b1c670c4595e90f323717263",
        46_639_448,
    ),
    // Same full capturer plus explicit source-SQL transport at 3722db9d0; Rust 1.94 arm64, DWARF stripped.
    (
        "940155d19b2b20276d6f217432fbbab181b36fb6ee4070dd033d755cc69e7f91",
        40_957_904,
    ),
    // Rust 1.94 arm64 dev capture-full at db73a226c, with DWARF stripped.
    (
        "19bfb02733d52634c26f2fadca3adbcd47ce6bf04c7bd9408b0862f3e012164e",
        40_940_392,
    ),
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
        None => true,
    }
}

#[test]
fn typed_delta_driver_resource_requires_exact_frozen_length() {
    for (digest, size) in [
        (
            "6c60fc489c40f82ea7662478fd8b4f77978bf07eafd8ef31870f49e6ebc5dfd8",
            68_937_024,
        ),
        (
            "3eef5786e2f03593ce48de9b818a125db6e8b48820f297b183019473276f277a",
            68_939_152,
        ),
        (
            "79efdf5187dab0ad92f13249d7001379030b734722760821c9555e74a8bbf1a7",
            59_708_480,
        ),
        (
            "bec48412b85bf8627f4707fe1c61ad8658b298ef6fda35b568e231d2b662a176",
            60_108_320,
        ),
        (
            "20e72de2aa1889c1e6db467e905b79a1f71c390018e4d22a9d23021ac76241f5",
            60_653_216,
        ),
        (
            "765646194e9a5523b620f01b12b3109f90b5dd35a9770f6b9d1a8fd574f44a5c",
            60_858_288,
        ),
    ] {
        assert!(driver_length_allowed(digest, size));
        assert!(!driver_length_allowed(digest, size - 1));
        assert!(!driver_length_allowed(digest, size + 1));
        assert!(driver_length_allowed(&"f".repeat(64), size));
    }
    assert!(driver_length_allowed(&"f".repeat(64), MAX_DRIVER));
    assert!(driver_length_allowed(&"f".repeat(64), MAX_DRIVER + 1));
}

fn read_driver(path: &str, expect: &str) -> Result<(), &'static str> {
    use std::io::Read;
    let meta = fs::metadata(path).map_err(|_| "digest_missing")?;
    if !meta.is_file() || !hex64(expect) || !driver_length_allowed(expect, meta.len()) {
        return Err("digest_mismatch");
    }
    let file = fs::File::open(path).map_err(|_| "digest_missing")?;
    let opened = file.metadata().map_err(|_| "digest_missing")?;
    if !opened.is_file()
        || opened.len() != meta.len()
        || opened.modified().ok() != meta.modified().ok()
    {
        return Err("digest_mismatch");
    }
    let mut input = (&file).take(meta.len().checked_add(1).ok_or("digest_mismatch")?);
    let mut buffer = [0u8; 64 * 1024];
    let mut digest = Sha256::new();
    let mut total = 0u64;
    loop {
        let n = input.read(&mut buffer).map_err(|_| "digest_missing")?;
        if n == 0 {
            break;
        }
        total = total.checked_add(n as u64).ok_or("digest_mismatch")?;
        if total > meta.len() {
            return Err("digest_mismatch");
        }
        digest.update(&buffer[..n]);
    }
    let after = file.metadata().map_err(|_| "digest_missing")?;
    let path_after = fs::metadata(path).map_err(|_| "digest_missing")?;
    if total == meta.len()
        && after.len() == meta.len()
        && path_after.len() == meta.len()
        && after.modified().ok() == meta.modified().ok()
        && path_after.modified().ok() == meta.modified().ok()
        && hex_bytes(&digest.finalize()) == expect
    {
        Ok(())
    } else {
        Err("digest_mismatch")
    }
}

#[test]
fn driver_hash_streams_beyond_legacy_memory_cap_and_rejects_wrong_bytes() {
    use std::io::Write;
    let path = std::env::temp_dir().join(format!(
        "streamed-driver-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut file = fs::File::create(&path).unwrap();
    let chunk = vec![0x5au8; 64 * 1024];
    let mut expected = Sha256::new();
    for _ in 0..513 {
        file.write_all(&chunk).unwrap();
        expected.update(&chunk);
    }
    file.sync_all().unwrap();
    let digest = hex_bytes(&expected.finalize());
    assert!(file.metadata().unwrap().len() > MAX_DRIVER);
    assert_eq!(read_driver(path.to_str().unwrap(), &digest), Ok(()));
    assert_eq!(
        read_driver(path.to_str().unwrap(), &"f".repeat(64)),
        Err("digest_mismatch")
    );
    file.write_all(b"changed").unwrap();
    file.sync_all().unwrap();
    assert_eq!(
        read_driver(path.to_str().unwrap(), &digest),
        Err("digest_mismatch")
    );
    drop(file);
    fs::remove_file(path).unwrap();
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
    if let Some(scope) = binding.multipart_scope.as_ref() {
        let capturer: serde_json::Value =
            serde_json::from_slice(&_receipt).map_err(|_| "nonadmitted")?;
        if capturer.get("multipart_scope") != Some(scope) {
            return Err("nonadmitted");
        }
    }
    if let Some(scope) = binding.range_scope.as_ref() {
        let capturer: serde_json::Value =
            serde_json::from_slice(&_receipt).map_err(|_| "nonadmitted")?;
        if capturer.get("range_scope") != Some(scope) {
            return Err("nonadmitted");
        }
    }
    if let Some(scope) = binding.partition_scope.as_ref() {
        let capturer: serde_json::Value =
            serde_json::from_slice(&_receipt).map_err(|_| "nonadmitted")?;
        if capturer.get("partition_scope") != Some(scope) {
            return Err("nonadmitted");
        }
    }
    if let Some(scope) = binding.transaction_scope.as_ref() {
        let capturer: serde_json::Value =
            serde_json::from_slice(&_receipt).map_err(|_| "nonadmitted")?;
        if capturer.get("transaction_scope") != Some(scope) {
            return Err("nonadmitted");
        }
    }
    if let Some(scope) = binding.gtid_scope.as_ref() {
        let capturer: serde_json::Value =
            serde_json::from_slice(&_receipt).map_err(|_| "nonadmitted")?;
        if capturer.get("gtid_scope") != Some(scope) {
            return Err("nonadmitted");
        }
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
        .is_none_or(|class| {
            class != "R0"
                && !(class == "R1" && matches!(engine, "postgresql" | "postgres" | "sqlserver"))
        })
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

/// Policy admission only. The caller must independently compare all exported
/// values/positions with the sealed, live-leased SQL stage before final commit.
pub(crate) fn admit_typed_export_policy(
    policy: &serde_json::Value,
    actual: &serde_json::Value,
) -> Result<(), &'static str> {
    const KEYS: &[&str] = &[
        "source_id",
        "report_path",
        "report_sha256",
        "config_path",
        "config_sha256",
        "driver_path",
        "driver_sha256",
        "admission_binary",
        "admission_sha256",
        "source_preflight",
        "reader_write_attempts",
    ];
    let object = policy.as_object().ok_or("typed_export_policy")?;
    if object.len() != KEYS.len() || object.keys().any(|k| !KEYS.contains(&k.as_str())) {
        return Err("typed_export_policy");
    }
    let report_path = required_abs(policy, "report_path")?;
    let config_path = required_abs(policy, "config_path")?;
    let report_bytes = read_hashed(&report_path, &required_hex(policy, "report_sha256")?)?;
    let config_bytes = read_hashed(&config_path, &required_hex(policy, "config_sha256")?)?;
    let report: serde_json::Value =
        serde_json::from_slice(&report_bytes).map_err(|_| "typed_export_policy")?;
    let config: serde_json::Value =
        serde_json::from_slice(&config_bytes).map_err(|_| "typed_export_policy")?;
    let bin = required_abs(policy, "admission_binary")?;
    if std::env::var("OO_NATIVE_ADMISSION_BIN").ok().as_deref() != Some(bin.as_str()) {
        return Err("typed_export_policy");
    }
    let bin_sha = required_hex(policy, "admission_sha256")?;
    if !approved_admission_bin(&bin_sha) {
        return Err("typed_export_policy");
    }
    read_driver(&bin, &bin_sha)?;
    read_driver(
        &required_abs(policy, "driver_path")?,
        &required_hex(policy, "driver_sha256")?,
    )?;
    let decision_bytes = run_validator(&bin, &report_path, &config_path)?;
    let decision: serde_json::Value =
        serde_json::from_slice(&decision_bytes).map_err(|_| "typed_export_policy")?;
    typed_export_policy_documents(policy, actual, &report, &config, &decision)?;
    // Refuse an operator file changed while the validator was reading it.
    if read_hashed(&report_path, &required_hex(policy, "report_sha256")?)? != report_bytes
        || read_hashed(&config_path, &required_hex(policy, "config_sha256")?)? != config_bytes
    {
        return Err("typed_export_policy");
    }
    Ok(())
}

fn typed_export_policy_documents(
    policy: &serde_json::Value,
    actual: &serde_json::Value,
    report: &serde_json::Value,
    config: &serde_json::Value,
    decision: &serde_json::Value,
) -> Result<(), &'static str> {
    let source = actual["source_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("typed_export_policy")?;
    let instance = &actual["source_instance"];
    let uuid = actual["measured_id"]
        .as_str()
        .and_then(|s| s.strip_prefix("mysql."))
        .filter(|s| mysql_binding_uuid(s))
        .ok_or("typed_export_policy")?;
    let end = actual["l_end"]
        .as_str()
        .and_then(|s| {
            s.parse::<u64>()
                .ok()
                .filter(|n| *n > 0 && n.to_string() == s)
        })
        .ok_or("typed_export_policy")?;
    let prefix = format!("{uuid}:1-");
    let start = actual["l0"]
        .as_str()
        .and_then(|s| s.strip_prefix(&prefix))
        .and_then(|s| {
            s.parse::<u64>()
                .ok()
                .filter(|n| *n > 0 && n.to_string() == s)
        })
        .ok_or("typed_export_policy")?;
    if actual["protocol"] != "oo.mysql.typed-export-binding/v1"
        || actual["engine"] != "mysql"
        || instance["engine"] != "mysql"
        || policy["source_id"] != source
        || instance["source"] != source
        || instance["lease_id"] != actual["task_id"]
        || instance["source_epoch"] != actual["source_epoch"]
        || actual["source_epoch"].as_i64().is_none_or(|n| n < 1)
        || start > end
        || ["measured_id", "config_digest", "schema_digest"]
            .iter()
            .any(|k| instance[*k] != actual[*k])
        || ["config_digest", "schema_digest"]
            .iter()
            .any(|k| actual[*k].as_str().is_none_or(|s| !hex64(s)))
        || policy["config_sha256"] != actual["config_digest"]
        || config["protocol"] != "oo.source-admission.config/v1"
        || config["source_id"] != source
        || config["target_id"] != source
        || config["compat_mode"] != "mysql-native"
        || report["protocol"] != REPORT_PROTOCOL
        || report["source_id"] != source
        || report["target_id"] != source
        || report["compat_mode"] != "mysql-native"
        || report["secret_ref"] != config["secret_ref"]
        || report["permissions"] != config["permissions"]
        || !report["r1w"].is_null()
        || report.get("capture_witness").is_some_and(|w| !w.is_null())
        || report["driver"]["kind"] != "native"
        || report["driver"]["name"] != "mysql_async"
        || report["driver"]["digest"] != policy["driver_sha256"]
        || policy["reader_write_attempts"] != 0
        || !config["debezium"]["heartbeat_action_query"].is_null()
        || !config["debezium"]["signal_data_collection"].is_null()
    {
        return Err("typed_export_policy");
    }
    verify_config(config, "mysql")?;
    let preflight = &policy["source_preflight"];
    if ["gtid_mode", "enforce_gtid_consistency"]
        .iter()
        .any(|k| preflight[*k] != "ON")
        || preflight["log_bin"] != "1"
        || preflight["binlog_format"] != "ROW"
        || preflight["binlog_row_image"] != "FULL"
        || preflight["binlog_expire_logs_seconds"].as_u64().is_none()
    {
        return Err("typed_export_policy");
    }
    let tables = actual["tables"]
        .as_array()
        .filter(|t| !t.is_empty() && t.len() <= 32)
        .ok_or("typed_export_policy")?;
    let mut wanted = std::collections::BTreeSet::new();
    let mut total = 0u64;
    for table in tables {
        let name = table["name"].as_str().ok_or("typed_export_policy")?;
        if !wanted.insert(name.to_owned()) {
            return Err("typed_export_policy");
        }
        total = total
            .checked_add(table["rows"].as_u64().ok_or("typed_export_policy")?)
            .ok_or("typed_export_policy")?;
    }
    let mut permitted = std::collections::BTreeSet::new();
    for permission in config["permissions"]
        .as_array()
        .ok_or("typed_export_policy")?
    {
        if permission["name"] != "SELECT"
            || permission["granted"] != true
            || permission["status"] != "known"
        {
            return Err("typed_export_policy");
        }
        let db = permission["scope"]["database"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("typed_export_policy")?;
        let table = permission["scope"]["table"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("typed_export_policy")?;
        if !permitted.insert(format!("{db}.{table}")) {
            return Err("typed_export_policy");
        }
    }
    if wanted != permitted
        || config["budget"]["hard_limit"]
            .as_u64()
            .is_none_or(|n| n == 0 || n > 50_000_000 || total > n)
    {
        return Err("typed_export_policy");
    }
    let methods = report["capture_methods"]
        .as_array()
        .filter(|m| m.len() == 1)
        .ok_or("typed_export_policy")?;
    let method = &methods[0];
    if method["method"] != "C1"
        || method["cdc"] != "Binlog"
        || method["write_class"] != "R0"
        || method["consistency"] != "T"
        || method["token_order"] != "Total"
        || method["delete_detection"] != "native"
        || decision["protocol"] != DECISION_PROTOCOL
        || decision["admitted"] != true
        || decision["matrix_mutated"] != false
        || decision["identity"]["source_id"] != source
        || decision["blocks"].as_array().is_none_or(|b| !b.is_empty())
        || !write_class_allowed(decision, "mysql")
    {
        return Err("typed_export_policy");
    }
    let admitted = decision["model"]["capture_methods"]
        .as_array()
        .filter(|m| m.len() == 1)
        .ok_or("typed_export_policy")?;
    if [
        "method",
        "cdc",
        "write_class",
        "consistency",
        "token_order",
        "delete_detection",
        "retention_window",
    ]
    .iter()
    .any(|k| admitted[0][*k] != method[*k])
    {
        return Err("typed_export_policy");
    }
    Ok(())
}

#[test]
fn typed_export_policy_stays_bound_to_the_actual_stage_and_read_only_scope() {
    use serde_json::json;
    let source = "owned_export";
    let uuid = "11111111-1111-4111-8111-111111111111";
    let mut config = json!({"protocol":"oo.source-admission.config/v1","source_id":source,"target_id":source,
        "compat_mode":"mysql-native","secret_ref":"secret://unit","requested_write_class":"R0","r1w":null,
        "permissions":[{"name":"SELECT","scope":{"database":"db","table":"items"},"granted":true,"status":"known"}],
        "write_options":{"signal_table":false,"heartbeat_write":false,"create_stream":false,"create_trigger":false},
        "debezium":{"snapshot_mode":"never","create_stream":false,"create_trigger":false},
        "budget":{"unit":"Rows","hard_limit":2000000}});
    let method = json!({"method":"C1","cdc":"Binlog","write_class":"R0","consistency":"T",
        "token_order":"Total","delete_detection":"native","retention_window":"604800s"});
    let report = json!({"protocol":REPORT_PROTOCOL,"source_id":source,"target_id":source,
        "compat_mode":"mysql-native","secret_ref":config["secret_ref"],"permissions":config["permissions"],"r1w":null,
        "driver":{"kind":"native","name":"mysql_async","digest":"d".repeat(64)},
        "capture_methods":[method]});
    let decision = json!({"protocol":DECISION_PROTOCOL,"admitted":true,"matrix_mutated":false,
        "identity":{"source_id":source},"blocks":[],"model":{"capture_methods":[method],"capabilities":[{"write_class":"R0"}]}});
    let policy = json!({"source_id":source,"config_sha256":"a".repeat(64),"driver_sha256":"d".repeat(64),
        "reader_write_attempts":0,"source_preflight":{"log_bin":"1","binlog_format":"ROW","binlog_row_image":"FULL",
            "gtid_mode":"ON","enforce_gtid_consistency":"ON","binlog_expire_logs_seconds":604800}});
    let actual = json!({"protocol":"oo.mysql.typed-export-binding/v1","source_id":source,"engine":"mysql",
        "task_id":uuid,"source_epoch":1,"measured_id":format!("mysql.{uuid}"),"config_digest":"a".repeat(64),
        "schema_digest":"b".repeat(64),"l0":format!("{uuid}:1-10"),"l_end":"13",
        "tables":[{"name":"db.items","rows":1010000}],
        "source_instance":{"source":source,"engine":"mysql","lease_id":uuid,"source_epoch":1,
            "measured_id":format!("mysql.{uuid}"),"config_digest":"a".repeat(64),"schema_digest":"b".repeat(64)}});
    typed_export_policy_documents(&policy, &actual, &report, &config, &decision).unwrap();
    for key in [
        "source_id",
        "engine",
        "source_epoch",
        "schema_digest",
        "config_digest",
        "l0",
        "l_end",
    ] {
        let mut bad = actual.clone();
        bad[key] = json!("foreign");
        assert!(
            typed_export_policy_documents(&policy, &bad, &report, &config, &decision).is_err(),
            "{key}"
        );
    }
    for key in [
        "source_id",
        "config_sha256",
        "driver_sha256",
        "reader_write_attempts",
        "source_preflight",
    ] {
        let mut bad = policy.clone();
        bad[key] = json!("foreign");
        assert!(
            typed_export_policy_documents(&bad, &actual, &report, &config, &decision).is_err(),
            "{key}"
        );
    }
    let mut bad = decision.clone();
    bad["model"]["capture_methods"][0]["method"] = json!("C5");
    assert!(typed_export_policy_documents(&policy, &actual, &report, &config, &bad).is_err());
    let mut other = actual.clone();
    other["tables"][0]["name"] = json!("db.foreign");
    assert!(typed_export_policy_documents(&policy, &other, &report, &config, &decision).is_err());
    config["budget"]["hard_limit"] = json!(100000);
    assert!(typed_export_policy_documents(&policy, &actual, &report, &config, &decision).is_err());
    config["budget"]["hard_limit"] = json!(2000000);
    config["write_options"]["heartbeat_write"] = json!(true);
    assert!(typed_export_policy_documents(&policy, &actual, &report, &config, &decision).is_err());
    config["write_options"]["heartbeat_write"] = json!(false);
    config["debezium"]["heartbeat_action_query"] = json!("UPDATE source_table SET n=1");
    assert!(typed_export_policy_documents(&policy, &actual, &report, &config, &decision).is_err());
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
    if !output.status.success() {
        return Err("nonadmitted");
    }
    Ok(output.stdout)
}

#[test]
fn validator_stdout_never_masks_a_nonzero_exit() {
    use std::os::unix::fs::PermissionsExt;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("oo-validator-exit-{}-{nonce}", std::process::id()));
    for code in [0, 1, 2, 7] {
        fs::write(
            &path,
            format!("#!/bin/sh\nprintf '%s' '{{\"admitted\":true}}'\nexit {code}\n"),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            run_validator(path.to_str().unwrap(), "/unused-report", "/unused-config").is_ok(),
            code == 0
        );
    }
    fs::remove_file(path).unwrap();
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
        matches!(
            item.get("write_class").and_then(|value| value.as_str()),
            Some("R0")
        ) || (item.get("write_class").and_then(|value| value.as_str()) == Some("R1")
            && ((matches!(engine, "postgresql" | "postgres")
                && item.get("cdc").and_then(|v| v.as_str()) == Some("LogicalDecoding"))
                || (engine == "sqlserver"
                    && item.get("feed").and_then(|v| v.as_str()) == Some("ChangeTracking"))))
    }) && methods.iter().all(|item| {
        item.get("write_class").and_then(|value| value.as_str()) == Some("R0")
            || (item.get("write_class").and_then(|value| value.as_str()) == Some("R1")
                && ((matches!(engine, "postgresql" | "postgres")
                    && item.get("method").and_then(|v| v.as_str()) == Some("C1")
                    && item.get("cdc").and_then(|v| v.as_str()) == Some("LogicalDecoding"))
                    || (engine == "sqlserver"
                        && item.get("method").and_then(|v| v.as_str()) == Some("C2")
                        && item.get("feed").and_then(|v| v.as_str()) == Some("ChangeTracking"))))
    })
}

fn verify_r1_capture(
    config: &serde_json::Value,
    engine: &str,
    admission: &[u8],
    capturer: &[u8],
) -> Result<(), &'static str> {
    if config.get("requested_write_class").and_then(|v| v.as_str()) != Some("R1") {
        return Ok(());
    }
    let receipt: serde_json::Value =
        serde_json::from_slice(admission).map_err(|_| "nonadmitted")?;
    let captured: serde_json::Value =
        serde_json::from_slice(capturer).map_err(|_| "nonadmitted")?;
    let kinds: &[&str] = match engine {
        "postgresql" | "postgres"
            if receipt.get("capture_method").and_then(|v| v.as_str()) == Some("C1") =>
        {
            &["publication", "replication_slot"]
        }
        "sqlserver" if receipt.get("capture_method").and_then(|v| v.as_str()) == Some("C2") => {
            &["ct_database", "ct_table"]
        }
        _ => return Err("nonadmitted"),
    };
    if receipt.get("write_class").and_then(|v| v.as_str()) != Some("R1")
        || receipt.get("write_attempts").and_then(|v| v.as_u64()) != Some(0)
        || receipt.get("dba_objects") != captured.get("dba_objects")
    {
        return Err("nonadmitted");
    }
    let objects = receipt
        .get("dba_objects")
        .and_then(|v| v.as_array())
        .ok_or("nonadmitted")?;
    let mut seen_sha: Option<String> = None;
    for kind in kinds {
        let matching: Vec<_> = objects
            .iter()
            .filter(|v| v.get("kind").and_then(|v| v.as_str()) == Some(*kind))
            .collect();
        if matching.len() != 1 {
            return Err("nonadmitted");
        }
        let item = matching[0];
        if item.get("created_by").and_then(|v| v.as_str()) != Some("dba")
            || item.get("observed").and_then(|v| v.as_bool()) != Some(true)
            || item.get("platform_created").and_then(|v| v.as_bool()) != Some(false)
            || item.get("platform_write_attempts").and_then(|v| v.as_u64()) != Some(0)
            || ["name", "risk", "rollback"].iter().any(|k| {
                item.get(*k)
                    .and_then(|v| v.as_str())
                    .is_none_or(str::is_empty)
            })
        {
            return Err("nonadmitted");
        }
        let sha = required_hex(item, "observation_sha256")?;
        let bytes = read_hashed(&required_abs(item, "observation_path")?, &sha)?;
        if bytes.is_empty() {
            return Err("nonadmitted");
        }
        if engine == "sqlserver" {
            if seen_sha.as_ref().is_some_and(|prior| prior != &sha) {
                return Err("nonadmitted");
            }
            seen_sha = Some(sha);
            let facts: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| "nonadmitted")?;
            ct_anchor(kind, item, &facts, &captured)?;
        }
    }
    Ok(())
}

fn ct_anchor(
    kind: &str,
    item: &serde_json::Value,
    facts: &serde_json::Value,
    captured: &serde_json::Value,
) -> Result<(), &'static str> {
    if facts.get("tds_query_executed").and_then(|v| v.as_bool()) != Some(true) {
        return Err("nonadmitted");
    }
    let database = facts
        .get("database")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("nonadmitted")?;
    let schema = facts
        .get("schema")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("nonadmitted")?;
    let table = facts
        .get("table")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("nonadmitted")?;
    let guid = facts
        .get("database_guid")
        .and_then(|v| v.as_str())
        .ok_or("nonadmitted")?;
    let fork = facts
        .get("recovery_fork_guid")
        .and_then(|v| v.as_str())
        .ok_or("nonadmitted")?;
    if !uuid_text(guid) || !uuid_text(fork) {
        return Err("nonadmitted");
    }
    let object_id = facts
        .get("object_id")
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0)
        .ok_or("nonadmitted")?;
    let current = facts
        .get("current_version")
        .and_then(|v| v.as_i64())
        .filter(|n| *n >= 0)
        .ok_or("nonadmitted")?;
    let minimum = facts
        .get("min_valid_version")
        .and_then(|v| v.as_i64())
        .filter(|n| *n >= 0)
        .ok_or("nonadmitted")?;
    if minimum > current {
        return Err("nonadmitted");
    }
    let name = item
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("nonadmitted")?;
    let expect = if kind == "ct_database" {
        database.to_string()
    } else {
        format!("{schema}.{table}")
    };
    if name != expect {
        return Err("nonadmitted");
    }
    let candidate = captured.get("candidate_position").ok_or("nonadmitted")?;
    if candidate.get("protocol").and_then(|v| v.as_str()) != Some("oo.mssql-preview.ct-position/v1")
        || candidate.get("database").and_then(|v| v.as_str()) != Some(database)
        || candidate.get("database_guid").and_then(|v| v.as_str()) != Some(guid)
        || candidate.get("recovery_fork_guid").and_then(|v| v.as_str()) != Some(fork)
    {
        return Err("nonadmitted");
    }
    let tables = candidate
        .get("tables")
        .and_then(|v| v.as_array())
        .ok_or("nonadmitted")?;
    let matching: Vec<_> = tables
        .iter()
        .filter(|row| {
            row.get("schema").and_then(|v| v.as_str()) == Some(schema)
                && row.get("name").and_then(|v| v.as_str()) == Some(table)
        })
        .collect();
    if matching.len() != 1 {
        return Err("nonadmitted");
    }
    if matching[0].get("object_id").and_then(|v| v.as_i64()) != Some(object_id)
        || matching[0].get("version").and_then(|v| v.as_i64()) != Some(current)
    {
        return Err("nonadmitted");
    }
    Ok(())
}

fn uuid_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
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
    use super::*;

    #[test]
    fn c1_v2_baseline_scope_is_validated_as_the_capturer_seals_it() {
        let uuid = "4fa4d36d-ca24-11f1-b35d-f2a970aacec2";
        let owner = "2b4008f4-da90-44c4-99fa-cc0de9aad596";
        let config = "75".repeat(32);
        let schema = "d7".repeat(32);
        let digest = "ab".repeat(32);
        let binding = serde_json::json!({"source_engine": "mysql", "config_digest": config, "schema_digest": schema});
        let table = serde_json::json!({"schema": "ob_fixture", "table": "customers", "expected_rows": 2000,
            "fill_rows": 2000, "cdc_rows": 0, "parts_digest": digest, "schema_sha256": schema,
            "expected_composed_digest": digest});
        let scope = serde_json::json!({"protocol": "oo.source-admission.c1-baseline-scope/v2", "engine": "mysql",
            "lineage": format!("mysql.{uuid}"), "l0": format!("{uuid}:1-65"), "l0_pos": 65,
            "l_end": format!("{uuid}:1-65"), "l_end_pos": 65, "complete": true, "config_digest": config,
            "schema_digest": "cd".repeat(32), "consistency_grade": "T", "cross_table_consistent": true,
            "composition_rule": "c1-compose/v2", "bucket_bits": 12,
            "cdc_batches": {"sha256": "ef".repeat(32), "count": 0, "chain_sha256": "01".repeat(32),
                "first_pos": null, "last_pos": null},
            "source_identity": {"engine": "mysql", "measured_id": format!("mysql.{uuid}"), "shard_id": "mysql_1",
                "owner_token": owner},
            "tables": [table], "expected_rows": 2000, "expected_composed_digest": digest});
        let objects = |rows: u64| {
            vec![BoundObject {
                logical_schema: "ob_fixture".into(),
                logical_table: "customers".into(),
                row_count: rows,
                arrow_sha256: "aa".repeat(32),
            }]
        };
        assert_eq!(
            validate_c1_baseline_scope(&scope, &binding, &objects(2000)),
            Ok(())
        );
        let mut short = scope.clone();
        short["expected_rows"] = serde_json::json!(1999);
        assert_eq!(
            validate_c1_baseline_scope(&short, &binding, &objects(2000)),
            Err("schema")
        );
        assert_eq!(
            validate_c1_baseline_scope(&scope, &binding, &objects(1999)),
            Err("digest_mismatch")
        );
        let mut moved = scope.clone();
        moved["l_end"] = serde_json::json!(format!("{uuid}:1-66"));
        assert_eq!(
            validate_c1_baseline_scope(&moved, &binding, &objects(2000)),
            Err("schema")
        );
    }

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
        let config = serde_json::json!({"requested_write_class":"R1"});
        let receipt = serde_json::json!({"capture_method":"C2","write_class":"R1","write_attempts":0,"dba_objects":objects,"candidate_position":candidate});
        let bytes = serde_json::to_vec(&receipt).unwrap();
        assert_eq!(
            verify_r1_capture(&config, "sqlserver", &bytes, &bytes),
            Ok(())
        );
        let mut missing = receipt.clone();
        missing["dba_objects"][0]["kind"] = serde_json::json!("anything");
        assert_eq!(
            verify_r1_capture(
                &config,
                "sqlserver",
                &serde_json::to_vec(&missing).unwrap(),
                &serde_json::to_vec(&missing).unwrap()
            ),
            Err("nonadmitted")
        );
        missing = receipt.clone();
        missing["dba_objects"][0]["observation_sha256"] = serde_json::json!("00".repeat(32));
        assert!(
            verify_r1_capture(
                &config,
                "sqlserver",
                &serde_json::to_vec(&missing).unwrap(),
                &serde_json::to_vec(&missing).unwrap()
            )
            .is_err()
        );
        let junk = path.with_extension("junk");
        std::fs::write(&junk, b"actual-test-observation").unwrap();
        let junk_hash = hex_bytes(&Sha256::digest(b"actual-test-observation"));
        missing = receipt.clone();
        for index in 0..2 {
            missing["dba_objects"][index]["observation_path"] = serde_json::json!(&junk);
            missing["dba_objects"][index]["observation_sha256"] = serde_json::json!(&junk_hash);
        }
        assert_eq!(
            verify_r1_capture(
                &config,
                "sqlserver",
                &serde_json::to_vec(&missing).unwrap(),
                &serde_json::to_vec(&missing).unwrap()
            ),
            Err("nonadmitted")
        );
        let mut drifted = facts.clone();
        drifted["current_version"] = serde_json::json!(9);
        let drifted_raw = serde_json::to_vec(&drifted).unwrap();
        std::fs::write(&path, &drifted_raw).unwrap();
        let drifted_hash = hex_bytes(&Sha256::digest(&drifted_raw));
        missing = receipt.clone();
        for index in 0..2 {
            missing["dba_objects"][index]["observation_sha256"] = serde_json::json!(&drifted_hash);
        }
        assert_eq!(
            verify_r1_capture(
                &config,
                "sqlserver",
                &serde_json::to_vec(&missing).unwrap(),
                &serde_json::to_vec(&missing).unwrap()
            ),
            Err("nonadmitted")
        );
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(junk).unwrap();
    }

    const DIGEST: &str = "abababababababababababababababababababababababababababababababab";

    #[test]
    fn large_capture_receipt_keeps_other_control_files_capped() {
        let path = std::env::temp_dir().join(format!("oo-capture-receipt-{}", std::process::id()));
        let bytes = vec![b'x'; 2_329_469];
        std::fs::write(&path, &bytes).unwrap();
        let digest = hex_bytes(&Sha256::digest(&bytes));
        assert_eq!(
            read_hashed(path.to_str().unwrap(), &digest),
            Err("nonadmitted")
        );
        assert_eq!(
            read_capture_receipt(path.to_str().unwrap(), &digest),
            Ok(bytes)
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn maria_tracking_capture_receipt_uses_the_scoped_limit() {
        let Ok(path) = std::env::var("OO_TEST_MARIA_CAPTURE_RECEIPT") else {
            return;
        };
        let digest = "34434c139e5c106cc99b41614cb775ee25b9d3db35bc42878761febdbfbbfa32";
        assert_eq!(read_hashed(&path, digest), Err("nonadmitted"));
        assert_eq!(
            read_capture_receipt(&path, digest).unwrap().len(),
            2_329_469
        );
        assert_eq!(read_capture_receipt(&path, DIGEST), Err("digest_mismatch"));
    }

    #[test]
    fn gtid_scope_is_a_registered_instance_and_not_a_postgres_xid() {
        let digest = "ab".repeat(32);
        let mut binding = serde_json::json!({"source_engine":"mariadb","config_digest":digest,"schema_digest":digest});
        let scope = serde_json::json!({"codec":"mariadb-gtid/v1","lineage":"d0.s1","source_epoch":1,"applied_pos":"7","txn_id":"0-1-7","measured_id":"d0.s1","config_digest":digest,"schema_digest":digest});
        assert_eq!(validate_gtid_scope(&scope, &binding), Ok(()));
        let mut set = scope.clone();
        set["txn_id"] = serde_json::json!("0-1-7,0-1-8");
        assert_eq!(validate_gtid_scope(&set, &binding), Err("schema"));
        let mut caller = scope.clone();
        caller["epoch_source"] = serde_json::json!("caller");
        assert_eq!(validate_gtid_scope(&caller, &binding), Err("schema"));
        binding["source_engine"] = serde_json::json!("postgres");
        assert_eq!(validate_gtid_scope(&scope, &binding), Err("schema"));
        binding["source_engine"] = serde_json::json!("mysql");
        let mysql = serde_json::json!({"codec":"mysql-gtid/v1","lineage":"mysql.1bc0749a-c3f5-11f1-9a89-6e5583e808c4","source_epoch":4,"applied_pos":"17","txn_id":"1bc0749a-c3f5-11f1-9a89-6e5583e808c4:17","measured_id":"mysql.1bc0749a-c3f5-11f1-9a89-6e5583e808c4","config_digest":digest,"schema_digest":digest});
        assert_eq!(validate_gtid_scope(&mysql, &binding), Ok(()));
        let mut unregistered_epoch = scope.clone();
        unregistered_epoch["source_epoch"] = serde_json::json!(0);
        assert_eq!(
            validate_gtid_scope(&unregistered_epoch, &binding),
            Err("schema")
        );
        let mut xid = scope.clone();
        xid["xid"] = serde_json::json!(7);
        assert_eq!(validate_gtid_scope(&xid, &binding), Err("schema"));
    }

    #[test]
    fn c4_scope_rejects_partial_or_crossed_observation() {
        let digest = "ab".repeat(32);
        let binding = serde_json::json!({"source_engine":"clickhouse","source_id":"clickhouse_268","source_generation":1,"schema_digest":digest,"type_digest":digest});
        let objects = vec![BoundObject {
            logical_schema: "live".to_string(),
            logical_table: "readings".to_string(),
            row_count: 2,
            arrow_sha256: digest.clone(),
        }];
        let mut scope = serde_json::json!({
            "source_id":"clickhouse_268","source_generation":1,"logical_schema":"live","logical_table":"readings",
            "partition_id":"202401","observed_partition_ids":["202401","202403"],"complete":true,
            "table_epoch":"3cad2d69-4353-4ab2-88b0-dd2ef92ee466","schema_fingerprint":"reading_id UInt64",
            "schema_digest":digest,"type_digest":digest,"arrow_sha256":digest,"row_count":2,
            "current_signature":"202401|2|parts","mutations_pending":false
        });
        assert_eq!(validate_partition_scope(&scope, &binding, &objects), Ok(()));
        scope["observed_partition_ids"] = serde_json::json!(["202403", "202401"]);
        assert_eq!(
            validate_partition_scope(&scope, &binding, &objects),
            Err("schema")
        );
        scope["observed_partition_ids"] = serde_json::json!(["202401", "202403"]);
        scope["mutations_pending"] = serde_json::json!(true);
        assert_eq!(
            validate_partition_scope(&scope, &binding, &objects),
            Err("schema")
        );
        scope["mutations_pending"] = serde_json::json!(false);
        scope["arrow_sha256"] = serde_json::json!("cd".repeat(32));
        assert_eq!(
            validate_partition_scope(&scope, &binding, &objects),
            Err("schema")
        );
    }

    #[test]
    fn library_integration_release_has_exact_resource_length() {
        const DRIVER: &str = "9ad546ac9c12c44c7e40b679df4556f89d53376e3644a98e4bafc1795a9c5806";
        assert!(driver_length_allowed(DRIVER, 24_095_272));
        assert!(!driver_length_allowed(DRIVER, 24_095_271));
        assert!(!driver_length_allowed(DRIVER, 24_095_273));
        assert!(!approved_admission_bin(DRIVER));
        assert!(approved_admission_bin(
            "229e4247258eae3cb15def2d66d2a049a4c43c87a05952c78315362e6ad478c2"
        ));
    }

    #[test]
    fn merged_linux_mysql_has_exact_resource_length() {
        const DRIVER: &str = "5251dc061491f34dd65e442695865d7a055e5457d2b4caf53aad054d57412256";
        assert!(driver_length_allowed(DRIVER, 325_704_944));
        assert!(!driver_length_allowed(DRIVER, 325_704_943));
        assert!(!driver_length_allowed(DRIVER, 325_704_945));
        assert!(!driver_length_allowed(DRIVER, 0));
        assert!(!approved_admission_bin(DRIVER));
    }

    #[test]
    fn merged_linux_admission_has_exact_resource_length() {
        const ADMISSION: &str = "4afd8fdbe68ddf8b0230716ff09b63fe648d0df1ce635365577bb9c7146230cb";
        assert!(driver_length_allowed(ADMISSION, 68_556_064));
        assert!(!driver_length_allowed(ADMISSION, 68_556_063));
        assert!(!driver_length_allowed(ADMISSION, 68_556_065));
        assert!(!driver_length_allowed(ADMISSION, 0));
        assert!(approved_admission_bin(ADMISSION));
    }

    #[test]
    fn layered_mysql_reader_has_exact_resource_length() {
        const DRIVER: &str = "58c2027585336b1ca10cc0c7e042cfffa5f75b0df711cc1fd6ce77ba70c93093";
        assert!(driver_length_allowed(DRIVER, 49_264_152));
        assert!(!driver_length_allowed(DRIVER, 49_264_151));
        assert!(!driver_length_allowed(DRIVER, 49_264_153));
        assert!(!driver_length_allowed(DRIVER, 0));
        assert!(!approved_admission_bin(DRIVER));
    }

    #[test]
    fn c5_range_linux_admission_has_exact_resource_length() {
        const ADMISSION: &str = "f61711e3118cf125af588673596d256de2f5cd6374f69baeda40e5e91be9751c";
        assert!(driver_length_allowed(ADMISSION, 68_115_528));
        assert!(!driver_length_allowed(ADMISSION, 68_115_527));
        assert!(!driver_length_allowed(ADMISSION, 68_115_529));
        assert!(!driver_length_allowed(ADMISSION, 0));
        assert!(approved_admission_bin(ADMISSION));
        assert!(!approved_admission_bin(&"f".repeat(64)));
    }

    #[test]
    fn grant_enforced_linux_driver_has_exact_resource_length() {
        const DRIVER: &str = "96810e8bbf548ae892809c3f9961c7305fe46ee5b1c670c4595e90f323717263";
        assert!(driver_length_allowed(DRIVER, 46_639_448));
        assert!(!driver_length_allowed(DRIVER, 46_639_447));
        assert!(!driver_length_allowed(DRIVER, 46_639_449));
        assert!(!driver_length_allowed(DRIVER, 0));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_GRANT_ENFORCED));
    }

    #[test]
    fn approved_driver_resource_is_the_declared_doris_reader() {
        const DORIS: &str = "d5dc8555ad76ecc9d4550ac5d2abd6b00bb2fee6ca5a17c7c206d294b206010c";
        assert!(driver_length_allowed(DORIS, 115_155_008));
        assert!(!driver_length_allowed(DORIS, 115_155_007));
        assert!(!driver_length_allowed(DORIS, 32 * 1024 * 1024));
        assert!(driver_length_allowed(DIGEST, 32 * 1024 * 1024));
        assert!(driver_length_allowed(DIGEST, 32 * 1024 * 1024 + 1));
        assert!(driver_length_allowed(DIGEST, 115_155_008));
        assert!(!driver_length_allowed(DORIS, 0));
        const OPENSEARCH: &str = "874d2588595d63b597139358b47e3097fcc5f45f772ffb5000ecd371e8e88b2e";
        assert!(driver_length_allowed(OPENSEARCH, 116_863_584));
        assert!(!driver_length_allowed(OPENSEARCH, 116_863_583));
        assert!(!driver_length_allowed(OPENSEARCH, 115_155_008));
        assert!(driver_length_allowed(DIGEST, 116_863_584));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_E465));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_T13C));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_MYSQL_FULL));
        const MYSQL_FULL: &str = "19bfb02733d52634c26f2fadca3adbcd47ce6bf04c7bd9408b0862f3e012164e";
        assert!(driver_length_allowed(MYSQL_FULL, 40_940_392));
        assert!(!driver_length_allowed(MYSQL_FULL, 40_940_391));
        assert!(driver_length_allowed(
            "c9a41a434b70c6626dd98efc5743ab26d56b48bab4bcaf0bd32290f7bf6a5aab",
            365_673_136
        ));
        assert!(!driver_length_allowed(
            "c9a41a434b70c6626dd98efc5743ab26d56b48bab4bcaf0bd32290f7bf6a5aab",
            365_673_135
        ));
        const MYSQL_FULL_TRANSPORT: &str =
            "940155d19b2b20276d6f217432fbbab181b36fb6ee4070dd033d755cc69e7f91";
        assert!(driver_length_allowed(MYSQL_FULL_TRANSPORT, 40_957_904));
        assert!(!driver_length_allowed(MYSQL_FULL_TRANSPORT, 40_957_903));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_NATIVE_MINIMAL));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_PG_TRANSACTION));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_PG_PER_TABLE));
        assert!(approved_admission_bin(ADMISSION_BIN_SHA256_SQLSERVER_CT));
        const MARIA: &str = "7ee551ecadd2fb66e3dea5564dab7336cafcce0e6fb0dd57d5816a7685338474";
        assert!(driver_length_allowed(MARIA, 314_875_904));
        assert!(!driver_length_allowed(MARIA, 314_875_903));
        assert!(driver_length_allowed(DIGEST, 314_875_904));
        const STAR_C4: &str = "683194dab7fd5ba58efb6da4009e82a8683ea3d07913153e8a7ac931a1caef4e";
        const DORIS_BASELINE: &str =
            "29fbbeb07880dccffbfe149262f99422e96b643c597b645bdaf43794b1c2734d";
        assert!(driver_length_allowed(STAR_C4, 113_981_880));
        assert!(!driver_length_allowed(STAR_C4, 113_981_879));
        assert!(driver_length_allowed(DORIS_BASELINE, 116_576_104));
        assert!(!driver_length_allowed(DORIS_BASELINE, 116_576_105));
        assert!(driver_length_allowed(DIGEST, 113_981_880));
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
