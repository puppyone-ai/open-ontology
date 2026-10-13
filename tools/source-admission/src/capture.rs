//! Capture witness. Methods, grades, and deletes come from the capturer bytes.
//!
//! A caller hash, a caller `trusted` bit, and `secret_ref` are not copied into
//! the public decision, the receipt, the registry, or the catalog.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::admit::{admit, simple_id};
use crate::decision_json;
use crate::model::{
    BINDING_REGISTRY_PROTOCOL, Block, CAPTURE_BINDING_PROTOCOL, CAPTURE_RECEIPT_PROTOCOL,
    CONSISTENCY, CaptureMethod, Decision, SEALED_CATALOG_PROTOCOL, SourceConfig, TargetReport,
    WITNESS_PROTOCOL, known,
};
use crate::native_map::{engine_for_compat, seal_engine};
use crate::parse::parse_strict;

const DELETE_MODES: &[&str] = &[
    "reconcile",
    "partition_replace",
    "unknown",
    "none",
    "key_absent",
    "native",
];
const PII: &[&str] = &["none", "personal", "sensitive"];
const BLANK: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug)]
pub struct InputFault {
    pub fact: String,
}

pub struct CapturedInput<'a> {
    pub witness: &'a [u8],
    pub capturer: &'a [u8],
    pub arrow: &'a [u8],
    pub driver: &'a [u8],
    pub schema_preimage: &'a [u8],
    pub type_preimage: &'a [u8],
    pub report: &'a [u8],
    pub config: &'a [u8],
}

pub struct WitnessFiles {
    pub capturer_path: String,
    pub arrow_path: String,
    pub driver_path: String,
    pub schema_preimage_path: String,
    pub type_preimage_path: String,
}

pub struct OutputPaths<'a> {
    pub report_path: &'a str,
    pub config_path: &'a str,
    pub catalog_path: &'a str,
    pub receipt_path: &'a str,
}

pub struct SealBundle {
    pub decision: Vec<u8>,
    pub receipt: Vec<u8>,
    pub registry: Vec<u8>,
    pub catalog: Vec<u8>,
    pub registry_sha256: String,
    pub preimage_sha256: String,
    pub arrow_schema_sha256: String,
    pub schema_digest: String,
}

pub struct SealResult {
    pub decision: Decision,
    pub decision_bytes: Vec<u8>,
    pub bundle: Option<SealBundle>,
    /// Set when the decision is admitted and no registry is emitted.
    pub refusal: Option<&'static str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Witness {
    protocol: String,
    source_engine: String,
    source_id: String,
    source_generation: i64,
    snapshot: SnapshotName,
    run_id: String,
    logical_schema: String,
    logical_table: String,
    capturer_path: String,
    arrow_path: String,
    driver_path: String,
    schema_preimage_path: String,
    type_preimage_path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotName {
    kind: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreimageTable {
    table: String,
    columns: Vec<PreimageColumn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreimageColumn {
    name: String,
    #[serde(default)]
    logical_name: Option<String>,
    arrow_type: String,
    nullable: bool,
    #[serde(default)]
    precision: Option<i32>,
    #[serde(default)]
    scale: Option<i32>,
    #[serde(default)]
    key: Option<bool>,
    #[serde(default)]
    document: Option<bool>,
    #[serde(default)]
    char_length: Option<i32>,
    #[serde(default)]
    pii: Option<String>,
    #[serde(default)]
    source_oid: Option<u32>,
    #[serde(default)]
    source_typmod: Option<i32>,
    #[serde(default)]
    source_format: Option<String>,
    #[serde(default)]
    bson_type: Option<BsonType>,
    #[serde(default)]
    bson_types: Option<Vec<BsonType>>,
    #[serde(default)]
    bson_present_rows: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BsonType {
    variant: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    subtype: Option<String>,
}

struct FieldOut {
    name: String,
    arrow_type: String,
    nullable: bool,
    precision: Option<i32>,
    scale: Option<i32>,
    timezone: Option<String>,
    char_length: Option<i32>,
    key: Option<bool>,
    document: Option<bool>,
    source_oid: Option<u32>,
    source_typmod: Option<i32>,
    source_format: Option<String>,
    bson_type: Option<BsonType>,
}

struct Material {
    engine: String,
    source_id: String,
    generation: i64,
    snapshot_kind: String,
    snapshot_value: String,
    run_id: String,
    logical_schema: String,
    logical_table: String,
    row_count: u64,
    quality: Option<String>,
    cross_table: Option<String>,
    delete_observation: String,
    capture_method: String,
    consistency: String,
    delete_detection: String,
    /// Present only after `bind_writes` observed zero attempts.
    write_attempts: u64,
    write_class: String,
    dba_objects: Option<Value>,
    source_cursor: Option<String>,
    /// Present only after `bind_writes` observed `snapshot_token` false.
    snapshot_token: bool,
    schema_digest: String,
    type_digest: String,
    config_digest: String,
    input_digest: String,
    arrow_schema_sha256: String,
    driver_digest: String,
    driver_kind: String,
    driver_name: String,
    driver_version: String,
    fields: Vec<FieldOut>,
    partition_scope: Option<Value>,
    transaction_scope: Option<Value>,
}

struct Prepared {
    decision: Decision,
    material: Option<Material>,
    witness: Witness,
}

pub fn witness_paths(bytes: &[u8]) -> Result<WitnessFiles, InputFault> {
    let witness = parse_witness(bytes)?;
    Ok(files_of(&witness))
}

pub fn decide_captured(
    report: &TargetReport,
    config: &SourceConfig,
    input: &CapturedInput<'_>,
) -> Result<Decision, InputFault> {
    Ok(prepare(report, config, input)?.decision)
}

pub fn seal_captured(
    report: &TargetReport,
    config: &SourceConfig,
    input: &CapturedInput<'_>,
    paths: &OutputPaths<'_>,
) -> Result<SealResult, InputFault> {
    for path in [
        paths.report_path,
        paths.config_path,
        paths.catalog_path,
        paths.receipt_path,
    ] {
        if !absolute_path(path) {
            return Err(fault("path"));
        }
    }
    let prepared = prepare(report, config, input)?;
    let decision_bytes = decision_json(&prepared.decision).map_err(|_| fault("decision"))?;
    if !prepared.decision.admitted {
        return Ok(SealResult {
            decision: prepared.decision,
            decision_bytes,
            bundle: None,
            refusal: None,
        });
    }
    if let Some(refusal) = seal_refusal(report, config, &prepared.decision) {
        return Ok(SealResult {
            decision: prepared.decision,
            decision_bytes,
            bundle: None,
            refusal: Some(refusal),
        });
    }
    let material = prepared.material.ok_or_else(|| fault("seal"))?;
    let bundle = pack(&decision_bytes, &material, &prepared.witness, input, paths)?;
    Ok(SealResult {
        decision: prepared.decision,
        decision_bytes,
        bundle: Some(bundle),
        refusal: None,
    })
}

fn files_of(witness: &Witness) -> WitnessFiles {
    WitnessFiles {
        capturer_path: witness.capturer_path.clone(),
        arrow_path: witness.arrow_path.clone(),
        driver_path: witness.driver_path.clone(),
        schema_preimage_path: witness.schema_preimage_path.clone(),
        type_preimage_path: witness.type_preimage_path.clone(),
    }
}

fn prepare(
    report: &TargetReport,
    config: &SourceConfig,
    input: &CapturedInput<'_>,
) -> Result<Prepared, InputFault> {
    let witness = parse_witness(input.witness)?;
    let capturer: Value =
        parse_strict(input.capturer).map_err(|err| fault(&safe_fact(&err.fact)))?;
    if !capturer.is_object() {
        return Err(fault("capturer"));
    }
    let schema_value: Value =
        parse_strict(input.schema_preimage).map_err(|err| fault(&safe_fact(&err.fact)))?;
    let type_value: Value =
        parse_strict(input.type_preimage).map_err(|err| fault(&safe_fact(&err.fact)))?;
    let tables: Vec<PreimageTable> =
        serde_json::from_value(schema_value.clone()).map_err(|_| fault("schema preimage"))?;

    if contains_secret(&capturer) || contains_secret(&schema_value) || contains_secret(&type_value)
    {
        let mut decision = admit(report, config);
        decision.blocks.push(block("SECRET_LEAK", "capturer"));
        return Ok(Prepared {
            decision: finish_public(decision)?,
            material: None,
            witness,
        });
    }

    let mut observed = observe(report, config, &witness, &capturer, &tables, &type_value, input);
    let mut policy = report.clone();
    if policy.capture_methods.is_empty()
        && let Some(method) = &observed.method
    {
        policy.capture_methods = vec![method.clone()];
    }
    let mut decision = admit(&policy, config);
    if r1_objects(&capturer, &witness).is_some()
        && witness.source_engine == "postgresql"
        && config.requested_write_class == "R1"
        && report.permissions == config.permissions {
        for (prefix, permissions) in [("report.permissions", &report.permissions), ("config.permissions", &config.permissions)] {
            for (index, permission) in permissions.iter().enumerate() {
                if permission.name == "REPLICATION" && permission.status == "known" && permission.granted {
                    let fact = format!("{prefix}[{index}]");
                    decision.blocks.retain(|item| !(item.code == "PERMISSION_NOT_READONLY" && item.fact == fact));
                }
            }
        }
    }
    if witness.source_engine == "sqlserver" && config.requested_write_class == "R1"
        && report.secret_ref == config.secret_ref && r1_objects(&capturer, &witness).is_some()
        && private_sql_file_reference(&report.secret_ref) {
        decision.blocks.retain(|item| !(item.code == "SECRET_REF_INVALID" && item.fact == "secret_ref"));
    }
    if !report.capture_methods.is_empty() {
        match &observed.method {
            Some(method) if methods_agree(&report.capture_methods, method) => {
                decision.model.capture_methods = vec![method.clone()];
            }
            Some(method) => {
                decision.model.capture_methods = vec![method.clone()];
                decision
                    .blocks
                    .push(block("CAPTURE_UNTRUSTED", "capture_methods"));
            }
            None => decision
                .blocks
                .push(block("CAPTURE_UNTRUSTED", "capture_methods")),
        }
    }
    decision.blocks.append(&mut observed.blocks);
    if report.capabilities.is_empty() {
        decision
            .blocks
            .push(block("CAPABILITY_ABSENT", "capabilities"));
    }
    let admitted = decision.blocks.is_empty();
    decision.admitted = admitted;
    if !admitted {
        observed.material = None;
    }
    Ok(Prepared {
        decision: finish_public(decision)?,
        material: observed.material,
        witness,
    })
}

struct Observation {
    blocks: Vec<Block>,
    method: Option<CaptureMethod>,
    material: Option<Material>,
}

struct WriteSeen {
    attempts_zero: bool,
    token_observed: bool,
}

fn observe(
    report: &TargetReport,
    config: &SourceConfig,
    witness: &Witness,
    capturer: &Value,
    tables: &[PreimageTable],
    type_value: &Value,
    input: &CapturedInput<'_>,
) -> Observation {
    let mut blocks = Vec::new();
    match engine_for_compat(&report.compat_mode) {
        Some(engine) if engine == witness.source_engine => {}
        Some(_) => blocks.push(block("ENGINE_MISMATCH", "source_engine")),
        None => blocks.push(block("ENGINE_UNSUPPORTED", "compat_mode")),
    }
    if witness.source_id != report.source_id || !simple_id(&witness.source_id) {
        blocks.push(block("IDENTITY_MISMATCH", "source_id"));
    }
    if witness.source_generation < 1 {
        blocks.push(block("GENERATION_INVALID", "source_generation"));
    }
    let protocol = capturer
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or("");
    let producer = producer_protocol(protocol);
    match capturer_engine(protocol) {
        Some(engine) if engine == witness.source_engine => {}
        _ => blocks.push(block("PROTOCOL_UNSUPPORTED", "capturer.protocol")),
    }
    if let Some(value) = capturer.get("source_engine")
        && value.as_str() != Some(witness.source_engine.as_str())
    {
        blocks.push(block("ENGINE_MISMATCH", "capturer.source_engine"));
    }
    if let Some(value) = capturer.get("engine")
        && value.as_str() != Some(witness.source_engine.as_str())
    {
        blocks.push(block("ENGINE_MISMATCH", "capturer.engine"));
    }
    if let Some(value) = capturer.get("source_id")
        && value.as_str() != Some(witness.source_id.as_str())
    {
        blocks.push(block("CROSS_SOURCE", "source_id"));
    }
    if let Some(value) = capturer.get("source_generation")
        && value.as_i64() != Some(witness.source_generation)
    {
        blocks.push(block("GENERATION_MISMATCH", "source_generation"));
    }
    let writes = bind_writes(capturer, producer, &mut blocks);
    let requires_r1 = capturer.get("protocol").and_then(Value::as_str) == Some("oo.postgres-preview.native-page-capture/v1")
        || capturer.get("protocol").and_then(Value::as_str) == Some("oo.mssql-preview.capture/v1");
    let dba_objects = r1_objects(capturer, witness);
    if requires_r1 && dba_objects.is_none() {
        blocks.push(block("R1_DBA_UNOBSERVED", "dba_objects"));
    }
    let table = match select_table(capturer, &witness.logical_table) {
        Some(table) => Some(table),
        None => {
            blocks.push(block("OBJECT_UNBOUND", "logical_table"));
            None
        }
    };
    bind_snapshot(capturer, table, witness, producer, &mut blocks);
    bind_schema_name(capturer, table, witness, &mut blocks);
    if let Some(table) = table
        && !std::ptr::eq(table, capturer)
        && let Some(value) = table.get("source_id")
        && value.as_str() != Some(witness.source_id.as_str())
    {
        blocks.push(block("CROSS_SOURCE", "source_id"));
    }
    if !object_scope_ok(
        report,
        &witness.logical_schema,
        &witness.logical_table,
        capturer,
        table,
    ) {
        blocks.push(block("OBJECT_UNAPPROVED", "logical_table"));
    }
    let source = table.unwrap_or(capturer);
    let method_token = take_member(
        fallback(source, capturer, method_member(source), method_member),
        "CAPTURE_UNKNOWN",
        "route",
        &mut blocks,
    );
    let grade = take_member(
        fallback(
            source,
            capturer,
            closed_member(source, &["consistency", "consistency_grade"], CONSISTENCY),
            |value| closed_member(value, &["consistency", "consistency_grade"], CONSISTENCY),
        ),
        "CONSISTENCY_UNKNOWN",
        "consistency",
        &mut blocks,
    );
    let delete = take_member(
        fallback(
            source,
            capturer,
            closed_member(source, &["delete_mode", "delete_detection"], DELETE_MODES),
            |value| closed_member(value, &["delete_mode", "delete_detection"], DELETE_MODES),
        ),
        "CAPTURE_UNKNOWN",
        "delete_mode",
        &mut blocks,
    );
    let observed_cursor = bind_cursor(source, producer, method_token.as_deref(), &mut blocks);
    let quality_value = source.get("quality").or_else(|| {
        if std::ptr::eq(source, capturer) {
            None
        } else {
            capturer.get("quality")
        }
    });
    if grade.as_deref() == Some("T") && quality_text(quality_value) == Some("DELETES_LAGGED") {
        blocks.push(block("GRADE_CONTRADICTION", "quality"));
    }
    let quality = quality_token(quality_value);
    let cross_table = cross_token(capturer.get("cross_table_consistency"), &mut blocks);
    let row_len = table
        .or(producer.then_some(capturer))
        .and_then(|table| row_len(table, producer, &mut blocks));
    let arrow_name_ok = match table.or(producer.then_some(capturer)) {
        Some(table) => arrow_name_ok(
            table,
            &witness.arrow_path,
            input.arrow,
            producer,
            &mut blocks,
        ),
        None => false,
    };

    let parsed = match parse_arrow(input.arrow) {
        Ok(parsed) => Some(parsed),
        Err("unsupported") => {
            blocks.push(block("SCHEMA_UNSUPPORTED", "arrow"));
            None
        }
        Err(_) => {
            blocks.push(block("ARROW_UNREADABLE", "arrow"));
            None
        }
    };
    if let Some(parsed) = parsed.as_ref()
        && config.budget.unit == "Rows" && parsed.rows > config.budget.hard_limit {
        blocks.push(block("BUDGET_EXCEEDED", "arrow.rows"));
    }
    if let Some(parsed) = parsed.as_ref()
        && unclassified_in_schema(capturer, parsed)
    {
        blocks.push(block("SCHEMA_UNSUPPORTED", "unclassified"));
    }
    if let (Some(len), Some(parsed)) = (row_len, parsed.as_ref())
        && len != parsed.rows
    {
        blocks.push(block("ROW_COUNT", "rows"));
    }

    let columns = bind_columns(
        capturer,
        &witness.source_engine,
        tables,
        &witness.logical_table,
        parsed.as_ref(),
        type_value,
        &mut blocks,
    );
    let schema_digest = sha256_hex(input.schema_preimage);
    let type_digest = sha256_hex(input.type_preimage);
    let config_digest = sha256_hex(input.config);
    let input_digest = sha256_hex(input.arrow);
    let partition_scope = match (witness.source_engine.as_str(), method_token.as_deref()) {
        ("clickhouse" | "doris" | "starrocks", Some("C4")) =>
            bind_partition_scope(capturer, witness, &schema_digest, &type_digest, &input_digest,
                parsed.as_ref().map(|arrow| arrow.rows), &mut blocks),
        (_, Some("C4")) => {
            blocks.push(block("SOURCE_METADATA_UNBOUND", "partition_scope"));
            None
        }
        _ if capturer.get("partition_scope").is_some() => {
            blocks.push(block("SOURCE_METADATA_UNBOUND", "partition_scope"));
            None
        }
        _ => None,
    };
    let transaction_scope = match (witness.source_engine.as_str(), method_token.as_deref(), grade.as_deref()) {
        ("postgres" | "postgresql", Some("C1"), Some("T")) if cross_table.as_deref() == Some("T") =>
            bind_transaction_scope(capturer, witness, &input_digest,
                parsed.as_ref().map(|arrow| arrow.rows), &mut blocks),
        _ if capturer.get("transaction_scope").is_some() => {
            blocks.push(block("SOURCE_METADATA_UNBOUND", "transaction_scope"));
            None
        }
        _ => None,
    };
    if let Some(parsed) = parsed.as_ref()
        && schema_digest == parsed.schema_sha256
    {
        blocks.push(block("SCHEMA_DIGEST_COLLISION", "schema_digest"));
    }
    let driver_digest = sha256_hex(input.driver);
    if input.driver.is_empty()
        || report.driver.kind.is_empty()
        || report.driver.name.is_empty()
        || report.driver.version.is_empty()
    {
        blocks.push(block("DRIVER_MISSING", "driver"));
    } else if !hex64(&report.driver.digest) || report.driver.digest != driver_digest {
        blocks.push(block("DRIVER_DIGEST_MISMATCH", "driver.digest"));
    }

    let method = match (method_token, grade, delete) {
        (Some(method), Some(grade), Some(delete)) => Some(CaptureMethod {
            method: method.clone(),
            write_class: if requires_r1 { "R1" } else { "R0" }.to_string(),
            consistency: grade,
            token_order: match observed_cursor.as_deref() {
                Some(value) if value.starts_with("pg-export-snapshot/v1:") => "Opaque",
                Some(_) => "Total",
                None => "None",
            }.to_string(),
            retention_window: None,
            cursor: if requires_r1 && method == "C2" { Some("Sequence".to_string()) } else { None },
            feed: if requires_r1 && method == "C2" { Some("ChangeTracking".to_string()) } else { None },
            cdc: if requires_r1 && method == "C1" { Some("LogicalDecoding".to_string()) } else { None },
            delete_detection: Some(delete),
        }),
        _ => None,
    };
    if blocks.is_empty() && !writes.token_observed {
        blocks.push(block("WRITE_UNOBSERVED", "snapshot_token"));
    }
    let material = if blocks.is_empty()
        && writes.attempts_zero
        && writes.token_observed
        && arrow_name_ok
        && row_len.is_some()
        && let Some(method) = method.clone()
        && let Some(delete_detection) = method.delete_detection.clone()
        && let Some(parsed) = parsed
        && let Some(fields) = columns
    {
        Some(Material {
            engine: witness.source_engine.clone(),
            source_id: witness.source_id.clone(),
            generation: witness.source_generation,
            snapshot_kind: witness.snapshot.kind.clone(),
            snapshot_value: witness.snapshot.value.clone(),
            run_id: witness.run_id.clone(),
            logical_schema: witness.logical_schema.clone(),
            logical_table: witness.logical_table.clone(),
            row_count: parsed.rows,
            quality,
            cross_table,
            delete_observation: delete_observation(table),
            capture_method: method.method,
            consistency: method.consistency,
            delete_detection,
            write_attempts: 0,
            write_class: method.write_class,
            dba_objects,
            source_cursor: observed_cursor,
            snapshot_token: capturer.get("snapshot_token").and_then(Value::as_bool).unwrap_or(false),
            schema_digest,
            type_digest,
            config_digest,
            input_digest,
            arrow_schema_sha256: parsed.schema_sha256,
            driver_digest,
            driver_kind: report.driver.kind.clone(),
            driver_name: report.driver.name.clone(),
            driver_version: report.driver.version.clone(),
            fields,
            partition_scope,
            transaction_scope,
        })
    } else {
        None
    };
    Observation {
        blocks,
        method,
        material,
    }
}

fn bind_partition_scope(
    capturer: &Value,
    witness: &Witness,
    schema_digest: &str,
    type_digest: &str,
    arrow_digest: &str,
    rows: Option<u64>,
    blocks: &mut Vec<Block>,
) -> Option<Value> {
    let Some(scope) = capturer.get("partition_scope").filter(|scope| scope.is_object()) else {
        blocks.push(block("SOURCE_METADATA_UNBOUND", "partition_scope"));
        return None;
    };
    let partition = scope.get("partition_id").and_then(Value::as_str).unwrap_or("");
    let ids = scope.get("observed_partition_ids").and_then(Value::as_array);
    let complete = ids.is_some_and(|ids| !ids.is_empty()
        && ids.iter().all(|id| id.as_str().is_some_and(|s| !s.is_empty()))
        && ids.windows(2).all(|pair| pair[0].as_str() < pair[1].as_str())
        && ids.iter().any(|id| id.as_str() == Some(partition)));
    let epoch = scope.get("table_epoch").and_then(Value::as_str).unwrap_or("");
    let olap = matches!(witness.source_engine.as_str(), "doris" | "starrocks");
    let epoch_ok = if olap { partition_digest(epoch) } else {
        epoch.len() == 36 && epoch.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) { byte == b'-' } else { byte.is_ascii_hexdigit() }
        })
    };
    let native_id = scope.get("native_partition_id").and_then(Value::as_str).unwrap_or("");
    let signature = scope.get("current_signature");
    let signature_ok = match signature {
        Some(Value::String(value)) if olap => value.strip_prefix(&format!("{native_id}|")).is_some_and(partition_digest),
        Some(Value::String(value)) => !value.is_empty() && value.starts_with(&format!("{partition}|")),
        Some(Value::Null) if !olap => rows == Some(0),
        _ => false,
    };
    let scope_keys = ["source_id", "source_generation", "logical_schema", "logical_table",
        "partition_id", "observed_partition_ids", "complete", "table_epoch",
        "schema_fingerprint", "schema_digest", "type_digest", "arrow_sha256",
        "row_count", "current_signature", "mutations_pending"];
    let olap_keys = ["source_engine", "key_model", "routing_column", "partition_range",
        "native_partition_id", "native_partition_version"];
    let keys_ok = scope.as_object().is_some_and(|object|
        object.len() == scope_keys.len() + if olap { olap_keys.len() } else { 0 }
            && scope_keys.iter().all(|key| object.contains_key(*key))
            && (!olap || olap_keys.iter().all(|key| object.contains_key(*key))));
    let olap_ok = !olap || (scope.get("source_engine").and_then(Value::as_str) == Some(witness.source_engine.as_str())
        && matches!(scope.get("key_model").and_then(Value::as_str), Some("duplicate" | "unique" | "primary"))
        && scope.get("routing_column").and_then(Value::as_str).is_some_and(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
        && partition_decimal(native_id)
        && scope.get("native_partition_version").and_then(Value::as_str).is_some_and(partition_decimal)
        && scope.get("schema_fingerprint").and_then(Value::as_str).is_some_and(partition_digest)
        && scope.get("partition_range").is_some_and(partition_range));
    if partition.is_empty() || !complete || !keys_ok || scope.get("complete").and_then(Value::as_bool) != Some(true)
        || !epoch_ok || !olap_ok
        || scope.get("mutations_pending").and_then(Value::as_bool) != Some(false)
        || scope.get("schema_fingerprint").and_then(Value::as_str).is_none_or(str::is_empty)
        || scope.get("schema_digest").and_then(Value::as_str) != Some(schema_digest)
        || scope.get("type_digest").and_then(Value::as_str) != Some(type_digest)
        || scope.get("arrow_sha256").and_then(Value::as_str) != Some(arrow_digest)
        || scope.get("row_count").and_then(Value::as_u64) != rows
        || scope.get("source_id").and_then(Value::as_str) != Some(witness.source_id.as_str())
        || scope.get("source_generation").and_then(Value::as_i64) != Some(witness.source_generation)
        || scope.get("logical_schema").and_then(Value::as_str) != Some(witness.logical_schema.as_str())
        || scope.get("logical_table").and_then(Value::as_str) != Some(witness.logical_table.as_str())
        || !signature_ok
        || scope.get("previous_signature").is_some() {
        blocks.push(block("SOURCE_METADATA_UNBOUND", "partition_scope"));
        return None;
    }
    Some(scope.clone())
}

fn bind_transaction_scope(
    capturer: &Value,
    witness: &Witness,
    arrow_digest: &str,
    rows: Option<u64>,
    blocks: &mut Vec<Block>,
) -> Option<Value> {
    let Some(scope) = capturer.get("transaction_scope") else {
        blocks.push(block("SOURCE_METADATA_UNBOUND", "transaction_scope"));
        return None;
    };
    let valid = (|| {
        let object = scope.as_object()?;
        let keys = ["source_id", "slot", "publication", "xid", "begin_message_lsn",
            "begin_final_lsn", "commit_lsn", "end_lsn", "complete", "tables"];
        if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key))
            || scope["source_id"] != witness.source_id || scope["complete"] != true
            || !["slot", "publication"].iter().all(|key| scope[*key].as_str().is_some_and(|s|
                !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')))
            || scope["xid"].as_i64().is_none_or(|xid| xid <= 0) { return None; }
        let begin = pg_lsn(scope["begin_message_lsn"].as_str()?)?;
        let final_lsn = pg_lsn(scope["begin_final_lsn"].as_str()?)?;
        let commit = pg_lsn(scope["commit_lsn"].as_str()?)?;
        let end = pg_lsn(scope["end_lsn"].as_str()?)?;
        if begin == 0 || begin > commit || final_lsn != commit || commit > end
            || witness.snapshot.kind != "pg_commit_lsn"
            || witness.snapshot.value != end.to_string() { return None; }
        let tables = scope["tables"].as_array()?;
        if tables.is_empty() || tables.len() > 32 { return None; }
        let mut prior = None;
        let mut current = false;
        for table in tables {
            let fields = table.as_object()?;
            let keys = ["logical_schema", "logical_table", "source_generation",
                "parent_source_generation", "arrow_sha256", "row_count", "tombstones_sha256"];
            if fields.len() != keys.len() || keys.iter().any(|key| !fields.contains_key(*key))
                || !table["arrow_sha256"].as_str().is_some_and(hex64)
                || !table["tombstones_sha256"].as_str().is_some_and(hex64) { return None; }
            let schema = table["logical_schema"].as_str()?;
            let name = table["logical_table"].as_str()?;
            if schema.is_empty() || name.is_empty() || prior.is_some_and(|old| old >= (schema, name)) {
                return None;
            }
            prior = Some((schema, name));
            let generation = table["source_generation"].as_i64()?;
            if generation < 1 || match table.get("parent_source_generation") {
                Some(Value::Null) => generation != 1,
                Some(value) => value.as_i64().is_none_or(|parent| parent < 1 || parent + 1 != generation),
                None => true,
            } { return None; }
            if schema == witness.logical_schema && name == witness.logical_table {
                if generation != witness.source_generation || table["arrow_sha256"] != arrow_digest
                    || table["row_count"].as_u64() != rows { return None; }
                current = true;
            }
        }
        current.then_some(())
    })().is_some();
    if !valid { blocks.push(block("SOURCE_METADATA_UNBOUND", "transaction_scope")); return None; }
    Some(scope.clone())
}

fn pg_lsn(value: &str) -> Option<u64> {
    let (hi, lo) = value.split_once('/')?;
    if hi.is_empty() || hi.len() > 8 || lo.is_empty() || lo.len() > 8
        || !hi.bytes().chain(lo.bytes()).all(|b| b.is_ascii_hexdigit()) { return None; }
    Some((u64::from(u32::from_str_radix(hi, 16).ok()?) << 32)
        | u64::from(u32::from_str_radix(lo, 16).ok()?))
}

fn partition_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn partition_decimal(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|number| number > 0)
}

fn partition_range(value: &Value) -> bool {
    let Some(object) = value.as_object() else { return false; };
    if object.len() != 2 { return false; }
    let (Some(lower), Some(upper)) = (object.get("lower").and_then(Value::as_str),
        object.get("upper").and_then(Value::as_str)) else { return false; };
    let iso = |date: &str| date.len() == 10 && date.bytes().enumerate().all(|(index, byte)|
        if matches!(index, 4 | 7) { byte == b'-' } else { byte.is_ascii_digit() });
    iso(lower) && iso(upper) && lower < upper
}

fn bind_snapshot(
    capturer: &Value,
    table: Option<&Value>,
    witness: &Witness,
    producer: bool,
    blocks: &mut Vec<Block>,
) {
    if producer {
        if let Some(label) = capturer.get("label").and_then(Value::as_str)
            && label != witness.snapshot.value
        {
            blocks.push(block("SNAPSHOT_MISMATCH", "label"));
        }
    } else if capturer.pointer("/snapshot/value").is_none()
        && capturer.get("label").and_then(Value::as_str) != Some(witness.snapshot.value.as_str())
    {
        blocks.push(block("SNAPSHOT_MISMATCH", "label"));
    }
    if let Some(value) = capturer.pointer("/snapshot/value") {
        if value.as_str() != Some(witness.snapshot.value.as_str()) {
            blocks.push(block("SNAPSHOT_MISMATCH", "snapshot.value"));
        }
    } else if producer && !snapshot_claim_matches(capturer, table, witness) {
        blocks.push(block("SNAPSHOT_MISMATCH", "snapshot.value"));
    }
    if let Some(value) = capturer.pointer("/snapshot/kind")
        && value.as_str() != Some(witness.snapshot.kind.as_str())
    {
        blocks.push(block("SNAPSHOT_MISMATCH", "snapshot.kind"));
    }
}

fn snapshot_claim_matches(capturer: &Value, table: Option<&Value>, witness: &Witness) -> bool {
    let expect = witness.snapshot.value.as_str();
    claim_eq(capturer, expect)
        || table.is_some_and(|table| !std::ptr::eq(table, capturer) && claim_eq(table, expect))
}

fn claim_eq(value: &Value, expect: &str) -> bool {
    value.get("position").and_then(Value::as_str) == Some(expect)
        || value.get("snapshot_sha256").and_then(Value::as_str) == Some(expect)
}

fn bind_schema_name(
    capturer: &Value,
    table: Option<&Value>,
    witness: &Witness,
    blocks: &mut Vec<Block>,
) {
    let mut saw = false;
    let mut check = |source: &Value| {
        for key in ["database", "logical_schema", "owner"] {
            if let Some(value) = source.get(key) {
                saw = true;
                if value.as_str() != Some(witness.logical_schema.as_str()) {
                    blocks.push(block("SCHEMA_UNBOUND", key));
                }
            }
        }
    };
    check(capturer);
    if let Some(table) = table
        && !std::ptr::eq(table, capturer)
    {
        check(table);
    }
    if !saw {
        blocks.push(block("SCHEMA_UNBOUND", "database"));
    }
}

fn bind_writes(capturer: &Value, producer: bool, blocks: &mut Vec<Block>) -> WriteSeen {
    let attempts_zero = match capturer
        .get("product_reader_write_attempts")
        .and_then(Value::as_u64)
    {
        Some(0) => true,
        Some(_) => {
            blocks.push(block("WRITE_OBSERVED", "product_reader_write_attempts"));
            false
        }
        None if producer
            && capturer.get("product_read").and_then(Value::as_str) == Some("zero") =>
        {
            true
        }
        None => {
            blocks.push(block("WRITE_UNOBSERVED", "product_reader_write_attempts"));
            false
        }
    };
    let token_observed = match capturer.get("snapshot_token") {
        Some(Value::Bool(false)) => true,
        Some(Value::Bool(true)) if capturer.get("protocol").and_then(Value::as_str)
            == Some("oo.postgres-preview.native-page-capture/v1")
            && capturer.pointer("/snapshot/kind").and_then(Value::as_str) == Some("pg_snapshot")
            && pg_cursor(capturer).is_some_and(|(_, codec)| codec == "pg-export-snapshot/v1") => true,
        Some(Value::Bool(true)) => {
            blocks.push(block("SNAPSHOT_TOKEN", "snapshot_token"));
            false
        }
        Some(_) => {
            blocks.push(block("WRITE_UNOBSERVED", "snapshot_token"));
            false
        }
        None if producer => false,
        None => {
            blocks.push(block("WRITE_UNOBSERVED", "snapshot_token"));
            false
        }
    };
    WriteSeen {
        attempts_zero,
        token_observed,
    }
}

fn private_sql_file_reference(reference: &str) -> bool {
    let path = reference.strip_prefix("file://").unwrap_or(reference);
    if !absolute_path(path) { return false; }
    let Ok(metadata) = std::fs::symlink_metadata(path) else { return false; };
    if !metadata.file_type().is_file() { return false; }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777 == 0o600
    }
    #[cfg(not(unix))]
    { false }
}

fn r1_objects(capturer: &Value, witness: &Witness) -> Option<Value> {
    let protocol = capturer.get("protocol")?.as_str()?;
    let method = match method_member(capturer) { Member::One(value) => value, _ => return None };
    let required = match (protocol, witness.source_engine.as_str(), method.as_str()) {
        ("oo.postgres-preview.native-page-capture/v1", "postgresql", "C1") => ["publication", "replication_slot"],
        ("oo.mssql-preview.capture/v1", "sqlserver", "C2") => ["ct_database", "ct_table"],
        _ => return None,
    };
    let objects = capturer.get("dba_objects")?.as_array()?;
    if objects.len() != 2 { return None; }
    for kind in required {
        let matching: Vec<_> = objects.iter().filter(|item| item.get("kind").and_then(Value::as_str) == Some(kind)).collect();
        let [object] = matching.as_slice() else { return None; };
        let observation_path = object.get("observation_path")?.as_str()?;
        if !absolute_path(observation_path) { return None; }
        let observation = std::fs::read(observation_path).ok()?;
        if observation.is_empty() || object.get("observation_sha256").and_then(Value::as_str) != Some(sha256_hex(&observation).as_str()) { return None; }
        if object.get("created_by").and_then(Value::as_str) != Some("dba")
            || object.get("observed").and_then(Value::as_bool) != Some(true)
            || object.get("platform_created").and_then(Value::as_bool) != Some(false)
            || object.get("platform_write_attempts").and_then(Value::as_u64) != Some(0)
            || !object.get("observation_sha256").and_then(Value::as_str).is_some_and(hex64)
            || ["name", "risk", "rollback"].iter().any(|key| object.get(*key).and_then(Value::as_str).is_none_or(|text| text.trim().is_empty())) {
            return None;
        }
    }
    Some(Value::Array(objects.clone()))
}

fn pg_cursor(table: &Value) -> Option<(String, &str)> {
    let codec = table.pointer("/cursor/codec")?.as_str()?;
    let position = table.pointer("/cursor/position")?.as_str()?;
    let kind = table.pointer("/snapshot/kind")?.as_str()?;
    let value = table.pointer("/snapshot/value")?.as_str()?;
    if value != position { return None; }
    match (codec, kind) {
        ("pg-commit-lsn/v1", "pg_commit_lsn") if position.parse::<u64>().ok().is_some_and(|n| n > 0) => {},
        ("pg-export-snapshot/v1", "pg_snapshot") if position.split('-').count() == 3
            && position.chars().all(|c| c.is_ascii_hexdigit() || c == '-') => {},
        _ => return None,
    }
    Some((format!("{codec}:{position}"), codec))
}

fn mssql_cursor(capturer: &Value) -> Option<String> {
    if !capturer.get("tds_query_executed")?.as_bool()? { return None; }
    let candidate = capturer.get("candidate_position")?;
    if candidate.get("protocol")?.as_str()? != "oo.mssql-preview.ct-position/v1"
        || candidate.get("source_id")? != capturer.get("source_id")? { return None; }
    let guid = candidate.get("database_guid")?.as_str()?;
    let fork = candidate.get("recovery_fork_guid")?.as_str()?;
    let valid_guid = |s: &str| s.len() == 36 && s.split('-').map(str::len).collect::<Vec<_>>() == [8,4,4,4,12]
        && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if !valid_guid(guid) || !valid_guid(fork) || candidate.get("database")?.as_str()?.is_empty() { return None; }
    let schema = capturer.get("logical_schema")?.as_str()?;
    let table = capturer.get("logical_table")?.as_str()?;
    let rows = candidate.get("tables")?.as_array()?;
    let matching: Vec<_> = rows.iter().filter(|item| item.get("schema").and_then(Value::as_str) == Some(schema)
        && item.get("name").and_then(Value::as_str) == Some(table)).collect();
    let [selected] = matching.as_slice() else { return None; };
    let version = selected.get("version")?.as_u64()?;
    let object_id = selected.get("object_id")?.as_u64()?;
    if object_id == 0 || !selected.get("schema_sha256")?.as_str().is_some_and(hex64) { return None; }
    let keys = selected.get("keys")?.as_array()?;
    if keys.is_empty() || keys.iter().any(|key| key.as_str().is_none_or(|s| s.is_empty())) { return None; }
    let mut unique = std::collections::BTreeSet::new();
    if keys.iter().any(|key| !unique.insert(key.as_str().unwrap_or(""))) { return None; }
    if capturer.pointer("/snapshot/kind")?.as_str()? != "mssql_ct_version"
        || capturer.pointer("/snapshot/value")?.as_str()? != version.to_string() { return None; }
    if let Some(prior) = capturer.get("prior_position").filter(|value| !value.is_null()) {
        for key in ["protocol", "source_id", "database_guid", "recovery_fork_guid", "database"] {
            if prior.get(key)? != candidate.get(key)? { return None; }
        }
        let old = prior.get("tables")?.as_array()?.iter().find(|item| item.get("schema").and_then(Value::as_str) == Some(schema)
            && item.get("name").and_then(Value::as_str) == Some(table))?;
        if old.get("object_id")? != selected.get("object_id")? || old.get("schema_sha256")? != selected.get("schema_sha256")?
            || old.get("keys")? != selected.get("keys")? || old.get("version")?.as_u64()? > version { return None; }
    }
    Some(format!("mssql-ct-version/v1:{guid}:{fork}:{object_id}:{version}"))
}

fn bind_cursor(table: &Value, producer: bool, method: Option<&str>, blocks: &mut Vec<Block>) -> Option<String> {
    if table.get("protocol").and_then(Value::as_str) == Some("oo.postgres-preview.native-page-capture/v1")
        && method == Some("C1") {
        if let Some((position, _)) = pg_cursor(table) { return Some(position); }
        blocks.push(block("CURSOR_UNOBSERVED", "cursor"));
        return None;
    }
    if table.get("protocol").and_then(Value::as_str) == Some("oo.mssql-preview.capture/v1") && method == Some("C2") {
        if let Some(position) = mssql_cursor(table) { return Some(position); }
        blocks.push(block("CURSOR_UNOBSERVED", "candidate_position"));
        return None;
    }
    match table.get("cursor_safety").and_then(Value::as_str) {
        Some("no_cursor") => {}
        Some(_) => blocks.push(block("CAPTURE_UNTRUSTED", "cursor_safety")),
        None if producer && matches!(method, Some("C4" | "C5")) => {}
        None => blocks.push(block("CURSOR_UNOBSERVED", "cursor_safety")),
    }
    None
}

fn select_table<'a>(capturer: &'a Value, table: &str) -> Option<&'a Value> {
    if let Some(raw) = capturer.get("tables") {
        let tables = raw.as_array()?;
        let mut found = None;
        for item in tables {
            if table_name_is(item, table) {
                if found.is_some() {
                    return None;
                }
                found = Some(item);
            }
        }
        return found;
    }
    if capturer.get("logical_table").and_then(Value::as_str) == Some(table) {
        Some(capturer)
    } else {
        None
    }
}

fn table_name_is(item: &Value, table: &str) -> bool {
    let name = item.get("name").and_then(Value::as_str);
    let alt = item.get("table").and_then(Value::as_str);
    match (name, alt) {
        (Some(name), Some(alt)) if name != alt => false,
        (Some(name), _) => name == table,
        (_, Some(alt)) => alt == table,
        _ => false,
    }
}

enum Member {
    One(String),
    Missing,
    Bad,
}

fn closed_member(table: &Value, keys: &[&str], set: &[&str]) -> Member {
    let mut found: Option<String> = None;
    for key in keys {
        if let Some(value) = table.get(*key) {
            let Some(text) = value.as_str() else {
                return Member::Bad;
            };
            if !known(set, text) {
                return Member::Bad;
            }
            if let Some(previous) = &found
                && previous != text
            {
                return Member::Bad;
            }
            found = Some(text.to_string());
        }
    }
    match found {
        Some(value) => Member::One(value),
        None => Member::Missing,
    }
}

fn quality_text(value: Option<&Value>) -> Option<&str> {
    let value = value?;
    if let Some(text) = value.as_str() {
        return Some(text);
    }
    let items = value.as_array()?;
    if items.len() != 1 {
        return None;
    }
    items[0].as_str()
}

fn quality_token(value: Option<&Value>) -> Option<String> {
    let text = quality_text(value)?;
    if token_ok(text) && text.len() <= 64 && !text.contains("secret://") {
        Some(text.to_string())
    } else {
        None
    }
}

fn cross_token(value: Option<&Value>, blocks: &mut Vec<Block>) -> Option<String> {
    let value = value?;
    match value.as_str() {
        Some(text) if known(CONSISTENCY, text) => Some(text.to_string()),
        _ => {
            blocks.push(block("CONSISTENCY_UNKNOWN", "cross_table_consistency"));
            None
        }
    }
}

fn row_len(table: &Value, producer: bool, blocks: &mut Vec<Block>) -> Option<u64> {
    if let Some(rows) = table.get("rows").and_then(Value::as_array) {
        let len = u64::try_from(rows.len()).ok()?;
        if let Some(stated) = table.get("row_count") {
            let stated_len = stated.as_u64();
            if stated_len != Some(len) {
                blocks.push(block("ROW_COUNT", "row_count"));
            }
        }
        return Some(len);
    }
    if producer && let Some(stated) = table.get("row_count").and_then(Value::as_u64) {
        return Some(stated);
    }
    blocks.push(block("ROW_COUNT", "rows"));
    None
}

fn arrow_name_ok(
    table: &Value,
    path: &str,
    arrow: &[u8],
    producer: bool,
    blocks: &mut Vec<Block>,
) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or("");
    if let Some(stated) = table.get("arrow") {
        if stated.as_str() == Some(file_name) && !file_name.is_empty() {
            return true;
        }
        blocks.push(block("ARROW_UNBOUND", "arrow"));
        return false;
    }
    if !producer {
        blocks.push(block("ARROW_UNBOUND", "arrow"));
        return false;
    }
    match table.get("arrow_sha256") {
        Some(Value::Null) => {
            blocks.push(block("ARROW_UNBOUND", "arrow_sha256"));
            false
        }
        Some(Value::String(text)) if hex64(text) && text == &sha256_hex(arrow) => true,
        Some(_) => {
            blocks.push(block("ARROW_UNBOUND", "arrow_sha256"));
            false
        }
        None => !file_name.is_empty(),
    }
}

fn delete_observation(table: Option<&Value>) -> String {
    let Some(table) = table else {
        return "unknown".to_string();
    };
    match table.get("tombstones").and_then(Value::as_array) {
        Some(items) if !items.is_empty() => "observed".to_string(),
        _ => "unknown".to_string(),
    }
}

fn object_scope_ok(
    report: &TargetReport,
    schema: &str,
    table: &str,
    capturer: &Value,
    selected: Option<&Value>,
) -> bool {
    let mut qualified = false;
    let mut matched = false;
    for item in &report.permissions {
        if item.status != "known" || !item.granted {
            continue;
        }
        let Some(scope) = item.scope.as_ref() else {
            continue;
        };
        if scope.database.is_none() && scope.table.is_none() && scope.index.is_none() {
            continue;
        }
        qualified = true;
        if scope_matches(scope, schema, table, capturer, selected) {
            matched = true;
        }
    }
    !qualified || matched
}

fn scope_matches(
    scope: &crate::model::GrantScope,
    schema: &str,
    table: &str,
    capturer: &Value,
    selected: Option<&Value>,
) -> bool {
    if scope.database.as_ref().is_some_and(|value| value != schema) {
        return false;
    }
    if scope.table.as_ref().is_some_and(|value| value != table) {
        return false;
    }
    if let Some(index) = scope.index.as_deref() {
        if scope.database.is_some() || scope.table.is_some() {
            return index == table;
        }
        return recorded_index(capturer, selected) == Some(index);
    }
    if scope.table.is_none() {
        return scope.database.is_some();
    }
    true
}

/// An index grant names the source object. The logical table may differ.
/// A declared field wins. `reader_audit` supplies the index only when no field does.
fn recorded_index<'a>(capturer: &'a Value, table: Option<&'a Value>) -> Option<&'a str> {
    let mut declared: Option<&str> = None;
    let mut scan = |source: &'a Value| -> bool {
        for key in ["index", "source_index", "source_object", "approved_index"] {
            let Some(value) = source.get(key).and_then(Value::as_str) else {
                continue;
            };
            if declared.is_some_and(|previous| previous != value) {
                return false;
            }
            declared = Some(value);
        }
        true
    };
    if let Some(table) = table
        && !std::ptr::eq(table, capturer)
        && !scan(table)
    {
        return None;
    }
    if !scan(capturer) {
        return None;
    }
    match (declared, audit_index(capturer)) {
        (Some(declared), Some(index)) if declared == index => Some(declared),
        (Some(declared), None) => Some(declared),
        (None, Some(index)) => Some(index),
        _ => None,
    }
}

fn audit_index(capturer: &Value) -> Option<&str> {
    let rows = capturer.get("reader_audit")?.as_array()?;
    let mut found = None;
    for row in rows {
        let Some(path) = row.get("path").and_then(Value::as_str) else {
            continue;
        };
        let Some(index) = index_from_path(path) else {
            continue;
        };
        if found.is_some_and(|previous| previous != index) {
            return None;
        }
        found = Some(index);
    }
    found
}

fn index_from_path(path: &str) -> Option<&str> {
    let path = path.split('?').next()?;
    let mut parts = path.split('/');
    if parts.next() != Some("") {
        return None;
    }
    let index = parts.next()?;
    if crate::native_map::opensearch_index(index) {
        Some(index)
    } else {
        None
    }
}

fn unclassified_in_schema(capturer: &Value, parsed: &ParsedArrow) -> bool {
    let Some(items) = capturer.get("unclassified").and_then(Value::as_array) else {
        return false;
    };
    items
        .iter()
        .any(|item| match item.get("name").and_then(Value::as_str) {
            Some(name) => parsed.fields.iter().any(|field| field.name == name),
            None => true,
        })
}

fn approval_token(value: Option<&Value>) -> bool {
    match value {
        None => true,
        Some(Value::String(text)) => {
            matches!(text.as_str(), "approved" | "loader" | "not_approved")
        }
        Some(_) => false,
    }
}

fn source_columns_agree(capturer: &Value, engine: &str, columns: &[PreimageColumn]) -> bool {
    let required = capturer.get("protocol").and_then(Value::as_str) == Some("oo.postgres-preview.native-page-capture/v1");
    let any = columns.iter().any(|column| column.source_oid.is_some() || column.source_typmod.is_some() || column.source_format.is_some());
    if !required && !any { return true; }
    if engine != "postgresql" { return false; }
    let Some(rows) = capturer.get("source_columns").and_then(Value::as_array) else { return false; };
    if rows.len() != columns.len() { return false; }
    for column in columns {
        let (Some(oid), Some(typmod), Some(format)) = (column.source_oid, column.source_typmod, column.source_format.as_deref()) else { return false; };
        if oid == 0 || format.trim().is_empty() { return false; }
        let matching: Vec<_> = rows.iter().filter(|row| row.get("name").and_then(Value::as_str) == Some(column.name.as_str())).collect();
        let [row] = matching.as_slice() else { return false; };
        if row.get("source_oid").and_then(Value::as_u64) != Some(u64::from(oid))
            || row.get("source_typmod").and_then(Value::as_i64) != Some(i64::from(typmod))
            || row.get("source_format").and_then(Value::as_str) != Some(format)
            || row.get("arrow_type").and_then(Value::as_str) != Some(column.arrow_type.as_str())
            || row.get("nullable").and_then(Value::as_bool) != Some(column.nullable) { return false; }
    }
    true
}

fn bind_columns(
    capturer: &Value,
    engine: &str,
    tables: &[PreimageTable],
    logical_table: &str,
    parsed: Option<&ParsedArrow>,
    type_value: &Value,
    blocks: &mut Vec<Block>,
) -> Option<Vec<FieldOut>> {
    let Some(preimage) = tables.iter().find(|table| table.table == logical_table) else {
        blocks.push(block("OBJECT_UNBOUND", "schema_preimage"));
        return None;
    };
    if preimage.columns.is_empty() || preimage.columns.len() > 64 {
        blocks.push(block("SCHEMA_MISMATCH", "columns"));
        return None;
    }
    if !source_columns_agree(capturer, engine, &preimage.columns) {
        blocks.push(block("SOURCE_METADATA_UNBOUND", "source_columns"));
        return None;
    }
    if engine != "mongodb" && preimage.columns.iter().any(|column| column.bson_type.is_some()) {
        blocks.push(block("SOURCE_METADATA_UNBOUND", "bson_type"));
        return None;
    }
    let parsed = parsed?;
    if engine == "mongodb" && preimage.columns.iter().any(|column| column.bson_types.is_some()) {
        if preimage.columns.iter().any(|column| {
            let (Some(types), Some(count)) = (&column.bson_types, column.bson_present_rows) else { return true; };
            count > parsed.rows || (types.is_empty() != (count == 0))
                || types.windows(2).any(|pair| pair[0].variant >= pair[1].variant)
                || (column.bson_type.as_ref() != if types.len() == 1 { types.first() } else { None })
        }) {
            blocks.push(block("SOURCE_METADATA_UNBOUND", "bson_types"));
            return None;
        }
    }
    if parsed.fields.len() != preimage.columns.len() {
        blocks.push(block("SCHEMA_MISMATCH", "columns"));
        return None;
    }
    let mut fields = Vec::with_capacity(parsed.fields.len());
    for (arrow, column) in parsed.fields.iter().zip(&preimage.columns) {
        match bind_column(arrow, column) {
            Ok(field) => fields.push(field),
            Err("pii") => {
                blocks.push(block("PII_UNAPPROVED", "pii"));
                return None;
            }
            Err("unsupported") => {
                blocks.push(block("SCHEMA_UNSUPPORTED", "arrow_type"));
                return None;
            }
            Err(_) => {
                blocks.push(block("SCHEMA_MISMATCH", "columns"));
                return None;
            }
        }
    }
    if !type_rows_agree(type_value, logical_table, &preimage.columns) {
        blocks.push(block("TYPE_UNBOUND", "type_preimage"));
        return None;
    }
    Some(fields)
}

fn bind_column(arrow: &ArrowField, column: &PreimageColumn) -> Result<FieldOut, &'static str> {
    if !token_ok(&column.name) || column.name != arrow.name {
        return Err("name");
    }
    if column
        .logical_name
        .as_ref()
        .is_some_and(|value| value != &column.name)
    {
        return Err("name");
    }
    let decimal_token = match (arrow.typed.precision, arrow.typed.scale) {
        (Some(p), Some(s)) if arrow.typed.arrow_type == "decimal128" || arrow.typed.arrow_type == "decimal256" =>
            Some(format!("{}({p},{s})", arrow.typed.arrow_type)),
        _ => None,
    };
    if column.arrow_type != arrow.typed.arrow_type && decimal_token.as_deref() != Some(column.arrow_type.as_str()) {
        return Err("type");
    }
    if column.nullable != arrow.nullable {
        return Err("nullable");
    }
    match column.pii.as_deref() {
        None => {}
        Some(value) if known(PII, value) => {}
        Some(_) => return Err("pii"),
    }
    if column.document == Some(true) && column.arrow_type != "utf8" {
        return Err("type");
    }
    if let Some(bson) = &column.bson_type {
        let expected = match (bson.variant.as_str(), bson.subtype.as_deref()) {
            ("07", None) => "fixedsizebinary(12)",
            ("05", Some("04")) => "fixedsizebinary(16)",
            ("05", Some("00")) => "binary",
            ("02", None) => "utf8",
            ("10", None) => "int32",
            ("12", None) => "int64",
            ("13", None) => "decimal128",
            ("09", None) => "timestamp(ms,utc)",
            ("01", None) => "float64",
            ("04", None) => "list(utf8)",
            _ => return Err("type"),
        };
        if column.arrow_type != expected { return Err("type"); }
    }
    if let Some(types) = &column.bson_types && types.len() > 1 {
        let variants: Vec<_> = types.iter().map(|tag| (tag.variant.as_str(), tag.subtype.as_deref())).collect();
        let expected = match variants.as_slice() {
            [("02", None), ("12", None)] => "union(dense,0:utf8,1:int64)",
            [("03", None), ("0a", None)] => "struct(kind:utf8,n:int32)",
            _ => return Err("type"),
        };
        if column.arrow_type != expected { return Err("type"); }
    }
    let (precision, scale) = match arrow.typed.arrow_type.as_str() {
        "decimal128" | "decimal256" => {
            let precision = column.precision.ok_or("precision")?;
            let scale = column.scale.ok_or("precision")?;
            if Some(precision) != arrow.typed.precision || Some(scale) != arrow.typed.scale {
                return Err("precision");
            }
            (Some(precision), Some(scale))
        }
        "int8" | "int16" | "int32" | "int64" | "uint8" | "uint16" | "uint32" | "uint64" => {
            if column.scale.is_some() {
                return Err("precision");
            }
            if column
                .precision
                .is_some_and(|value| Some(value) != arrow.typed.precision)
            {
                return Err("precision");
            }
            (column.precision, None)
        }
        _ => {
            if column.precision.is_some() || column.scale.is_some() {
                return Err("precision");
            }
            (None, None)
        }
    };
    Ok(FieldOut {
        name: column.name.clone(),
        arrow_type: column.arrow_type.clone(),
        nullable: column.nullable,
        precision,
        scale,
        timezone: arrow.typed.timezone.clone(),
        char_length: column.char_length,
        key: column.key,
        document: column.document,
        source_oid: column.source_oid,
        source_typmod: column.source_typmod,
        source_format: column.source_format.clone(),
        bson_type: column.bson_type.clone(),
    })
}

fn type_rows_agree(type_value: &Value, table: &str, columns: &[PreimageColumn]) -> bool {
    let Some(rows) = type_value.as_array() else {
        return false;
    };
    for column in columns {
        let mut saw = false;
        for row in rows {
            if row.get("table").and_then(Value::as_str) != Some(table)
                || row.get("name").and_then(Value::as_str) != Some(column.name.as_str())
            {
                continue;
            }
            saw = true;
            if let Some(oid) = column.source_oid
                && (row.get("source_oid").and_then(Value::as_u64) != Some(u64::from(oid))
                    || row.get("source_typmod").and_then(Value::as_i64) != column.source_typmod.map(i64::from)
                    || row.get("source_format").and_then(Value::as_str) != column.source_format.as_deref()) { return false; }
            if row.get("bson_type") != column.bson_type.as_ref().and_then(|tag| serde_json::to_value(tag).ok()).as_ref() { return false; }
            if row.get("bson_types") != column.bson_types.as_ref().and_then(|tags| serde_json::to_value(tags).ok()).as_ref()
                || row.get("bson_present_rows").and_then(Value::as_u64) != column.bson_present_rows { return false; }
            if row.get("arrow_type").and_then(Value::as_str) != Some(column.arrow_type.as_str()) {
                return false;
            }
            if !approval_token(row.get("approval")) {
                return false;
            }
            if !number_agrees(row.get("precision"), column.precision)
                || !number_agrees(row.get("scale"), column.scale)
            {
                return false;
            }
        }
        if !saw {
            return false;
        }
    }
    true
}

fn number_agrees(value: Option<&Value>, expect: Option<i32>) -> bool {
    match (value, expect) {
        (None, None) | (Some(Value::Null), None) => true,
        (Some(Value::Number(number)), Some(expect)) => number.as_i64() == Some(i64::from(expect)),
        (None, Some(_)) | (Some(Value::Null), Some(_)) => false,
        _ => false,
    }
}

fn methods_agree(user: &[CaptureMethod], derived: &CaptureMethod) -> bool {
    let [only] = user else {
        return false;
    };
    only.method == derived.method
        && only.write_class == derived.write_class
        && only.consistency == derived.consistency
        && only.token_order == derived.token_order
        && only.retention_window == derived.retention_window
        && only.cursor == derived.cursor
        && only.feed == derived.feed
        && only.cdc == derived.cdc
        && only
            .delete_detection
            .as_ref()
            .is_none_or(|value| derived.delete_detection.as_ref() == Some(value))
}

fn seal_refusal(
    report: &TargetReport,
    config: &SourceConfig,
    decision: &Decision,
) -> Option<&'static str> {
    let engine = engine_for_compat(&report.compat_mode)?;
    if !seal_engine(engine) {
        return Some("source engine");
    }
    if config.budget.unit != "Rows" || config.budget.hard_limit == 0 {
        return Some("budget unit");
    }
    let r0 = config.requested_write_class == "R0" && decision_r0(decision);
    let r1 = config.requested_write_class == "R1"
        && matches!(engine, "postgresql" | "sqlserver")
        && decision.model.capture_methods.len() == 1
        && decision.model.capture_methods[0].write_class == "R1"
        && ((engine == "postgresql" && decision.model.capture_methods[0].method == "C1")
            || (engine == "sqlserver" && decision.model.capture_methods[0].method == "C2"))
        && !decision.model.capabilities.is_empty()
        && decision.model.capabilities.iter().all(|cap| matches!(cap.write_class.as_str(), "R0" | "R1"));
    if config.r1w.is_some() || !(r0 || r1) {
        return Some("not r0");
    }
    None
}

fn decision_r0(decision: &Decision) -> bool {
    if decision.model.capabilities.is_empty() || decision.model.capture_methods.is_empty() {
        return false;
    }
    decision
        .model
        .capabilities
        .iter()
        .all(|item| item.write_class == "R0")
        && decision
            .model
            .capture_methods
            .iter()
            .all(|item| item.write_class == "R0")
}

fn finish_public(mut decision: Decision) -> Result<Decision, InputFault> {
    decision.identity.secret_ref.clear();
    decision.admitted = decision.blocks.is_empty();
    if public_leaks(&decision)? {
        decision.blocks.push(block("SECRET_LEAK", "decision"));
        decision.admitted = false;
        decision.dba_checklist.clear();
        decision.model.capture_methods.clear();
        decision.model.capabilities.clear();
        decision.identity.secret_ref.clear();
        if public_leaks(&decision)? {
            return Err(fault("decision"));
        }
    }
    Ok(decision)
}

fn public_leaks(decision: &Decision) -> Result<bool, InputFault> {
    let bytes = decision_json(decision).map_err(|_| fault("decision"))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| fault("decision"))?;
    Ok(contains_secret(&value))
}

fn pack(
    decision_bytes: &[u8],
    material: &Material,
    witness: &Witness,
    input: &CapturedInput<'_>,
    paths: &OutputPaths<'_>,
) -> Result<SealBundle, InputFault> {
    let catalog = catalog_bytes(material)?;
    let catalog_sha = sha256_hex(&catalog);
    let preimage = registry_bytes(
        material,
        witness,
        input,
        paths,
        &catalog_sha,
        BLANK,
        decision_bytes,
    )?;
    if count_slice(&preimage, BLANK.as_bytes()) != 1 {
        return Err(fault("registry"));
    }
    let preimage_sha = sha256_hex(&preimage);
    if preimage_sha == BLANK {
        return Err(fault("registry"));
    }
    let receipt = receipt_bytes(material, &catalog_sha, &preimage_sha)?;
    let receipt_sha = sha256_hex(&receipt);
    if receipt_sha == BLANK || receipt_sha.len() != 64 {
        return Err(fault("registry"));
    }
    let pos = find_slice(&preimage, BLANK.as_bytes()).ok_or_else(|| fault("registry"))?;
    let mut registry = preimage.clone();
    registry[pos..pos + 64].copy_from_slice(receipt_sha.as_bytes());
    let mut check = registry.clone();
    check[pos..pos + 64].copy_from_slice(BLANK.as_bytes());
    if check != preimage {
        return Err(fault("registry"));
    }
    let registry_sha = sha256_hex(&registry);
    if registry_sha == preimage_sha {
        return Err(fault("registry"));
    }
    for bytes in [&catalog, &receipt, &registry] {
        let value: Value = serde_json::from_slice(bytes).map_err(|_| fault("registry"))?;
        if contains_secret(&value) {
            return Err(fault("registry"));
        }
    }
    Ok(SealBundle {
        decision: decision_bytes.to_vec(),
        receipt,
        registry,
        catalog,
        registry_sha256: registry_sha,
        preimage_sha256: preimage_sha,
        arrow_schema_sha256: material.arrow_schema_sha256.clone(),
        schema_digest: material.schema_digest.clone(),
    })
}

fn catalog_bytes(material: &Material) -> Result<Vec<u8>, InputFault> {
    with_newline(&serde_json::json!({
        "protocol": SEALED_CATALOG_PROTOCOL,
        "engine": material.engine,
        "source_id": material.source_id,
        "source_generation": material.generation,
        "schema_digest": material.schema_digest,
        "type_digest": material.type_digest,
        "config_digest": material.config_digest,
        "input_digest": material.input_digest
    }))
}

fn receipt_bytes(
    material: &Material,
    catalog_sha: &str,
    registry_sha: &str,
) -> Result<Vec<u8>, InputFault> {
    let mut map = serde_json::Map::new();
    map.insert("schema".to_string(), json_str(CAPTURE_RECEIPT_PROTOCOL));
    map.insert("source_bound".to_string(), Value::Bool(true));
    map.insert("source_engine".to_string(), json_str(&material.engine));
    map.insert("write_class".to_string(), json_str(&material.write_class));
    if let Some(objects) = &material.dba_objects { map.insert("dba_objects".to_string(), objects.clone()); }
    if let Some(cursor) = &material.source_cursor { map.insert("source_cursor".to_string(), json_str(cursor)); }
    map.insert("source_id".to_string(), json_str(&material.source_id));
    map.insert(
        "source_generation".to_string(),
        serde_json::json!(material.generation),
    );
    map.insert(
        "snapshot".to_string(),
        serde_json::json!({"kind": material.snapshot_kind, "value": material.snapshot_value}),
    );
    map.insert("run_id".to_string(), json_str(&material.run_id));
    map.insert(
        "logical_schema".to_string(),
        json_str(&material.logical_schema),
    );
    map.insert(
        "logical_table".to_string(),
        json_str(&material.logical_table),
    );
    map.insert(
        "row_count".to_string(),
        serde_json::json!(material.row_count),
    );
    map.insert(
        "delete_observation".to_string(),
        json_str(&material.delete_observation),
    );
    map.insert(
        "capture_method".to_string(),
        json_str(&material.capture_method),
    );
    map.insert("consistency".to_string(), json_str(&material.consistency));
    map.insert(
        "delete_detection".to_string(),
        json_str(&material.delete_detection),
    );
    map.insert(
        "write_attempts".to_string(),
        serde_json::json!(material.write_attempts),
    );
    map.insert(
        "snapshot_token".to_string(),
        Value::Bool(material.snapshot_token),
    );
    if let Some(quality) = &material.quality {
        map.insert("quality".to_string(), json_str(quality));
    }
    if let Some(cross) = &material.cross_table {
        map.insert("cross_table_consistency".to_string(), json_str(cross));
    }
    map.insert(
        "schema_digest".to_string(),
        json_str(&material.schema_digest),
    );
    map.insert("type_digest".to_string(), json_str(&material.type_digest));
    map.insert(
        "config_digest".to_string(),
        json_str(&material.config_digest),
    );
    map.insert("input_digest".to_string(), json_str(&material.input_digest));
    map.insert("catalog_sha256".to_string(), json_str(catalog_sha));
    map.insert("registry_sha256".to_string(), json_str(registry_sha));
    map.insert(
        "driver_digest".to_string(),
        json_str(&material.driver_digest),
    );
    map.insert(
        "arrow_schema_sha256".to_string(),
        json_str(&material.arrow_schema_sha256),
    );
    map.insert(
        "arrow_sha256".to_string(),
        Value::Array(vec![json_str(&material.input_digest)]),
    );
    with_newline(&Value::Object(map))
}

fn registry_bytes(
    material: &Material,
    witness: &Witness,
    input: &CapturedInput<'_>,
    paths: &OutputPaths<'_>,
    catalog_sha: &str,
    receipt_sha: &str,
    decision_bytes: &[u8],
) -> Result<Vec<u8>, InputFault> {
    let mut binding = serde_json::json!({
        "protocol": CAPTURE_BINDING_PROTOCOL,
        "source_id": material.source_id,
        "source_engine": material.engine,
        "source_generation": material.generation,
        "snapshot": {"kind": material.snapshot_kind, "value": material.snapshot_value},
        "run_id": material.run_id,
        "report_path": paths.report_path,
        "config_path": paths.config_path,
        "report_sha256": sha256_hex(input.report),
        "config_sha256": sha256_hex(input.config),
        "decision_sha256": sha256_hex(decision_bytes),
        "driver_digest": material.driver_digest,
        "driver_sha256": material.driver_digest,
        "driver_path": witness.driver_path,
        "driver_kind": material.driver_kind,
        "driver_name": material.driver_name,
        "driver_version": material.driver_version,
        "capture_receipt_path": witness.capturer_path,
        "capture_receipt_sha256": sha256_hex(input.capturer),
        "catalog_path": paths.catalog_path,
        "catalog_sha256": catalog_sha,
        "admission_receipt_path": paths.receipt_path,
        "admission_receipt_sha256": receipt_sha,
        "schema_digest": material.schema_digest,
        "type_digest": material.type_digest,
        "config_digest": material.config_digest,
        "input_digest": material.input_digest,
        "arrow_schema_sha256": material.arrow_schema_sha256,
        "objects": [{
            "logical_schema": material.logical_schema,
            "logical_table": material.logical_table,
            "row_count": material.row_count,
            "arrow_sha256": material.input_digest
        }],
        "fields": material.fields.iter().map(field_json).collect::<Vec<_>>()
    });
    if let Some(scope) = &material.partition_scope {
        binding["partition_scope"] = scope.clone();
    }
    if let Some(scope) = &material.transaction_scope {
        binding["transaction_scope"] = scope.clone();
    }
    with_newline(&serde_json::json!({
        "protocol": BINDING_REGISTRY_PROTOCOL,
        "bindings": [binding]
    }))
}

fn field_json(field: &FieldOut) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("name".to_string(), json_str(&field.name));
    map.insert("arrow_type".to_string(), json_str(&field.arrow_type));
    map.insert("nullable".to_string(), Value::Bool(field.nullable));
    if let Some(precision) = field.precision {
        map.insert("precision".to_string(), serde_json::json!(precision));
    }
    if let Some(scale) = field.scale {
        map.insert("scale".to_string(), serde_json::json!(scale));
    }
    if let Some(timezone) = &field.timezone {
        map.insert("timezone".to_string(), json_str(timezone));
    }
    if let Some(char_length) = field.char_length {
        map.insert("char_length".to_string(), serde_json::json!(char_length));
    }
    if let Some(key) = field.key {
        map.insert("key".to_string(), Value::Bool(key));
    }
    if let Some(document) = field.document {
        map.insert("document".to_string(), Value::Bool(document));
    }
    if let Some(oid) = field.source_oid { map.insert("source_oid".to_string(), serde_json::json!(oid)); }
    if let Some(typmod) = field.source_typmod { map.insert("source_typmod".to_string(), serde_json::json!(typmod)); }
    if let Some(format) = &field.source_format { map.insert("source_format".to_string(), json_str(format)); }
    if let Some(tag) = &field.bson_type {
        let mut bson = serde_json::Map::new();
        bson.insert("variant".to_string(), json_str(&tag.variant));
        if let Some(subtype) = &tag.subtype { bson.insert("subtype".to_string(), json_str(subtype)); }
        map.insert("bson_type".to_string(), Value::Object(bson));
    }
    Value::Object(map)
}

fn parse_witness(bytes: &[u8]) -> Result<Witness, InputFault> {
    let witness: Witness = parse_strict(bytes).map_err(|err| fault(&safe_fact(&err.fact)))?;
    if witness.protocol != WITNESS_PROTOCOL {
        return Err(fault("protocol"));
    }
    if !token_ok(&witness.source_engine)
        || !simple_id(&witness.source_id)
        || !token_ok(&witness.snapshot.kind)
        || !token_ok(&witness.snapshot.value)
        || !token_ok(&witness.logical_schema)
        || !token_ok(&witness.logical_table)
        || !run_ok(&witness.run_id)
    {
        return Err(fault("witness"));
    }
    for path in [
        &witness.capturer_path,
        &witness.arrow_path,
        &witness.driver_path,
        &witness.schema_preimage_path,
        &witness.type_preimage_path,
    ] {
        if !absolute_path(path) {
            return Err(fault("path"));
        }
    }
    Ok(witness)
}

fn capturer_engine(protocol: &str) -> Option<&str> {
    if let Some(engine) = tick_engine(protocol) {
        return Some(engine);
    }
    producer_engine(protocol)
}

fn producer_protocol(protocol: &str) -> bool {
    producer_engine(protocol).is_some() || matches!(protocol, "oo.mysql.capture-tick/v1" | "oo.clickhouse.capture-tick/v1")
}

fn producer_engine(protocol: &str) -> Option<&str> {
    match protocol {
        "oo.oracle.c5-producer-catalog/v1" => Some("oracle"),
        "oo.mariadb.c5-capture-catalog/v1" => Some("mariadb"),
        "oo.postgres-preview.capture-check/v1" | "oo.postgres-preview.native-page-capture/v1" => Some("postgresql"),
        "oo.mysql-customer.audit-receipt/v1" => Some("mysql"),
        "oo.mssql-preview.capture/v1" => Some("sqlserver"),
        _ => None,
    }
}

fn method_member(value: &Value) -> Member {
    let mut found: Option<String> = None;
    for key in ["route", "capture_method", "method"] {
        let Some(raw) = value.get(key) else {
            continue;
        };
        let Some(text) = raw.as_str() else {
            return Member::Bad;
        };
        let Some(canon) = canonical_method(text) else {
            return Member::Bad;
        };
        if let Some(previous) = &found
            && previous != canon
        {
            return Member::Bad;
        }
        found = Some(canon.to_string());
    }
    match found {
        Some(value) => Member::One(value),
        None => Member::Missing,
    }
}

fn canonical_method(text: &str) -> Option<&str> {
    match text {
        "C1" | "C2" | "C3" | "C4" | "C5" => Some(text),
        "c1-logical" => Some("C1"),
        _ => None,
    }
}

fn fallback(
    primary: &Value,
    parent: &Value,
    first: Member,
    again: impl Fn(&Value) -> Member,
) -> Member {
    match first {
        Member::Missing if !std::ptr::eq(primary, parent) => again(parent),
        other => other,
    }
}

fn take_member(member: Member, code: &str, fact: &str, blocks: &mut Vec<Block>) -> Option<String> {
    match member {
        Member::One(value) => Some(value),
        Member::Missing | Member::Bad => {
            blocks.push(block(code, fact));
            None
        }
    }
}

fn tick_engine(protocol: &str) -> Option<&str> {
    let rest = protocol.strip_prefix("oo.")?;
    let (engine, tail) = rest.split_once('.')?;
    if tail == "capture-tick/v1"
        && token_ok(engine)
        && engine
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        Some(engine)
    } else {
        None
    }
}

fn finish_newline(bytes: &mut Vec<u8>) {
    bytes.push(b'\n');
}

fn with_newline(value: &Value) -> Result<Vec<u8>, InputFault> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| fault("json"))?;
    finish_newline(&mut bytes);
    Ok(bytes)
}

fn json_str(value: &str) -> Value {
    Value::String(value.to_string())
}

fn contains_secret(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("secret://"),
        Value::Array(items) => items.iter().any(contains_secret),
        Value::Object(map) => map.contains_key("secret_ref") || map.values().any(contains_secret),
        _ => false,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex64(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn token_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| byte >= 0x20)
        && !value.contains("secret://")
}

fn run_ok(value: &str) -> bool {
    token_ok(value)
}

fn absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 4096
        && !value.contains('\0')
        && !value.contains('\n')
        && !value.contains("secret://")
}

fn block(code: &str, fact: &str) -> Block {
    Block {
        code: code.to_string(),
        fact: fact.to_string(),
    }
}

fn fault(fact: &str) -> InputFault {
    InputFault {
        fact: fact.to_string(),
    }
}

fn safe_fact(fact: &str) -> String {
    if fact.contains("secret://") {
        return "secret".to_string();
    }
    let line = fact.lines().next().unwrap_or("json");
    let mut out = String::new();
    for ch in line.chars().take(180) {
        if ch.is_control() {
            break;
        }
        out.push(ch);
    }
    if out.is_empty() {
        "json".to_string()
    } else {
        out
    }
}

fn count_slice(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn find_slice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

struct ParsedArrow {
    fields: Vec<ArrowField>,
    rows: u64,
    schema_sha256: String,
}

struct ArrowField {
    dictionary_id: Option<i64>,
    name: String,
    nullable: bool,
    typed: NamedType,
}

struct NamedType {
    arrow_type: String,
    precision: Option<i32>,
    scale: Option<i32>,
    timezone: Option<String>,
}

fn parse_arrow(data: &[u8]) -> Result<ParsedArrow, &'static str> {
    if data.starts_with(b"ARROW1") || data.len() < 8 {
        return Err("truncated");
    }
    let mut offset = 0usize;
    let mut schema = None;
    let mut rows = 0u64;
    let mut saw_batch = false;
    let mut dictionaries = std::collections::BTreeSet::new();
    while offset + 8 <= data.len() {
        let continuation = i32_at(data, offset).ok_or("truncated")?;
        let meta_len = i32_at(data, offset + 4).ok_or("truncated")?;
        if meta_len == 0 && (continuation == -1 || continuation == 0) {
            offset += 8;
            break;
        }
        if continuation != -1 || meta_len <= 0 {
            return Err("continuation");
        }
        let meta_len = meta_len as usize;
        let meta_end = offset
            .checked_add(8)
            .and_then(|value| value.checked_add(meta_len))
            .ok_or("truncated")?;
        if meta_end > data.len() {
            return Err("truncated");
        }
        let meta = &data[offset + 8..meta_end];
        let message = read_message(meta)?;
        let body_len = usize::try_from(message.body_length).map_err(|_| "truncated")?;
        let next = align8(meta_end)
            .and_then(|start| align8(start.checked_add(body_len)?))
            .ok_or("truncated")?;
        if next > data.len() {
            return Err("truncated");
        }
        match message.header {
            Header::Schema(fields) if schema.is_none() && !saw_batch => {
                schema = Some((fields, sha256_hex(meta)));
            }
            Header::Dictionary(id) if schema.is_some() => {
                let fields = &schema.as_ref().ok_or("schema")?.0;
                if !fields.iter().any(|field| field.dictionary_id == Some(id)) {
                    return Err("schema");
                }
                dictionaries.insert(id);
            }
            Header::Batch(len) if schema.is_some() => {
                if schema.as_ref().ok_or("schema")?.0.iter().any(|field|
                    field.dictionary_id.is_some_and(|id| !dictionaries.contains(&id))) {
                    return Err("schema");
                }
                rows = rows.checked_add(len).ok_or("truncated")?;
                saw_batch = true;
            }
            _ => return Err("schema"),
        }
        offset = next;
    }
    if offset != data.len() {
        return Err("trailing");
    }
    let Some((fields, schema_sha256)) = schema else {
        return Err("schema");
    };
    if fields.is_empty() {
        return Err("schema");
    }
    Ok(ParsedArrow {
        fields,
        rows,
        schema_sha256,
    })
}

enum Header {
    Schema(Vec<ArrowField>),
    Batch(u64),
    Dictionary(i64),
}

struct Message {
    header: Header,
    body_length: i64,
}

fn read_message(meta: &[u8]) -> Result<Message, &'static str> {
    let root = u32_at(meta, 0).ok_or("truncated")? as usize;
    if root >= meta.len() {
        return Err("truncated");
    }
    let header_type = read_u8_field(meta, root, 1).ok_or("schema")?;
    // Message.bodyLength defaults to 0. A schema message omits the slot.
    let body_length = match field_slot(meta, root, 3) {
        None => 0,
        Some(pos) => i64_at(meta, pos).ok_or("truncated")?,
    };
    if body_length < 0 {
        return Err("truncated");
    }
    let header = match header_type {
        1 => {
            let table = read_uoffset_field(meta, root, 2).ok_or("schema")?;
            Header::Schema(read_schema(meta, table)?)
        }
        2 => {
            let table = read_uoffset_field(meta, root, 2).ok_or("schema")?;
            let id = read_i64_field(meta, table, 0).unwrap_or(0);
            let batch = read_uoffset_field(meta, table, 1).ok_or("schema")?;
            if read_i64_field(meta, batch, 0).unwrap_or(0) < 0 {
                return Err("schema");
            }
            Header::Dictionary(id)
        }
        3 => {
            let table = read_uoffset_field(meta, root, 2).ok_or("schema")?;
            let len = match field_slot(meta, table, 0) {
                Some(pos) => i64_at(meta, pos).ok_or("schema")?,
                None => 0,
            };
            if len < 0 {
                return Err("schema");
            }
            Header::Batch(len as u64)
        }
        _ => return Err("unsupported"),
    };
    Ok(Message {
        header,
        body_length,
    })
}

fn read_schema(meta: &[u8], table: usize) -> Result<Vec<ArrowField>, &'static str> {
    if read_i16_field(meta, table, 0).is_some_and(|endian| endian != 0) {
        return Err("unsupported");
    }
    let slot = field_slot(meta, table, 1).ok_or("schema")?;
    let tables = read_offset_vector(meta, slot)?;
    if tables.is_empty() || tables.len() > 64 {
        return Err("schema");
    }
    let mut fields = Vec::with_capacity(tables.len());
    for table in tables {
        fields.push(read_field(meta, table)?);
    }
    Ok(fields)
}

fn read_field(meta: &[u8], table: usize) -> Result<ArrowField, &'static str> {
    let name = read_string_at(meta, field_slot(meta, table, 0).ok_or("schema")?).ok_or("schema")?;
    let nullable = match field_slot(meta, table, 1) {
        // Arrow Field.nullable defaults to false when the vtable slot is absent.
        None => false,
        Some(pos) => read_bool_at(meta, pos).ok_or("schema")?,
    };
    let dictionary = field_slot(meta, table, 4)
        .map(|slot| read_uoffset_at(meta, slot).ok_or("schema"))
        .transpose()?;
    let dictionary_id = if let Some(dictionary) = dictionary {
        let index = read_uoffset_field(meta, dictionary, 1).ok_or("schema")?;
        if int_type(meta, index)?.arrow_type != "int32" {
            return Err("unsupported");
        }
        Some(read_i64_field(meta, dictionary, 0).unwrap_or(0))
    } else { None };
    let children = match field_slot(meta, table, 5) {
        None => Vec::new(),
        Some(slot) => read_offset_vector(meta, slot)?,
    };
    let type_byte = read_u8_field(meta, table, 2).ok_or("unsupported")?;
    let type_table = field_slot(meta, table, 3).and_then(|pos| read_uoffset_at(meta, pos));
    let mut typed = if type_byte == 12 {
        list_type(meta, &children)?
    } else if type_byte == 13 || type_byte == 14 {
        composite_type(meta, type_byte, type_table, &children)?
    } else if children.is_empty() {
        name_arrow_type(meta, type_byte, type_table)?
    } else {
        return Err("unsupported");
    };
    if dictionary_id.is_some() {
        if typed.arrow_type != "binary" { return Err("unsupported"); }
        typed.arrow_type = "dictionary_int32_binary".to_string();
    }
    Ok(ArrowField {
        dictionary_id,
        name,
        nullable,
        typed,
    })
}

fn name_arrow_type(
    meta: &[u8],
    type_byte: u8,
    table: Option<usize>,
) -> Result<NamedType, &'static str> {
    match type_byte {
        2 => int_type(meta, table.ok_or("schema")?),
        4 => Ok(plain("binary")),
        5 => Ok(plain("utf8")),
        6 => Ok(plain("bool")),
        7 => decimal_type(meta, table.ok_or("schema")?),
        8 => date_type(meta, table.ok_or("schema")?),
        3 => float_type(meta, table.ok_or("schema")?),
        10 => timestamp_type(meta, table.ok_or("schema")?),
        9 => {
            let table = table.ok_or("schema")?;
            match (read_i16_field(meta, table, 0).unwrap_or(1), read_i32_field(meta, table, 1).unwrap_or(32)) {
                (3, 64) => Ok(plain("time64_nanos")),
                (2, 64) => Ok(plain("time64_micros")),
                _ => Err("unsupported"),
            }
        }
        18 => {
            let table = table.ok_or("schema")?;
            match read_i16_field(meta, table, 0) {
                Some(2) => Ok(plain("duration_micros")),
                _ => Err("unsupported"),
            }
        }
        15 => {
            match read_i32_field(meta, table.ok_or("schema")?, 0) {
                Some(16) => Ok(plain("fixedsizebinary(16)")),
                Some(12) => Ok(plain("fixedsizebinary(12)")),
                _ => Err("unsupported"),
            }
        }
        _ => Err("unsupported"),
    }
}

fn float_type(meta: &[u8], table: usize) -> Result<NamedType, &'static str> {
    // Arrow Precision: HALF=0, SINGLE=1, DOUBLE=2. Only DOUBLE is float64.
    let precision = read_i16_field(meta, table, 0).ok_or("unsupported")?;
    if precision == 2 {
        Ok(plain("float64"))
    } else {
        Err("unsupported")
    }
}

fn timestamp_type(meta: &[u8], table: usize) -> Result<NamedType, &'static str> {
    // Arrow TimeUnit defaults to SECOND when the slot is absent.
    let unit = read_i16_field(meta, table, 0).unwrap_or(0);
    let timezone = match field_slot(meta, table, 1) {
        None => None,
        Some(pos) => Some(read_string_at(meta, pos).ok_or("unsupported")?),
    };
    let zone = timezone.as_deref().filter(|text| !text.is_empty());
    let (name, timezone) = match (unit, zone) {
        (0, None) => ("timestamp_second_naive", None),
        (0, Some("UTC")) => ("timestamp_second_utc", Some("UTC".to_string())),
        (1, None) => ("timestamp_millis_naive", None),
        (1, Some("UTC")) => ("timestamp(ms,utc)", None),
        (2, None) => ("timestamp_micros", None),
        (2, Some(text)) => ("timestamp_micros_utc", Some(text.to_string())),
        (3, Some("UTC")) => ("timestamp_nanos_utc", Some("UTC".to_string())),
        _ => return Err("unsupported"),
    };
    Ok(NamedType {
        arrow_type: name.to_string(),
        precision: None,
        scale: None,
        timezone,
    })
}

fn composite_type(meta: &[u8], kind: u8, table: Option<usize>, children: &[usize]) -> Result<NamedType, &'static str> {
    let [left, right] = children else { return Err("unsupported"); };
    let left = read_field(meta, *left)?;
    let right = read_field(meta, *right)?;
    if left.dictionary_id.is_some() || right.dictionary_id.is_some() { return Err("unsupported"); }
    match kind {
        13 if left.name == "kind" && right.name == "n"
            && left.typed.arrow_type == "utf8" && right.typed.arrow_type == "int32" => Ok(plain("struct(kind:utf8,n:int32)")),
        14 if left.typed.arrow_type == "utf8" && right.typed.arrow_type == "int64" => {
            let table = table.ok_or("schema")?;
            if read_i16_field(meta, table, 0).unwrap_or(0) != 1 { return Err("unsupported"); }
            let slot = field_slot(meta, table, 1).ok_or("schema")?;
            let vector = read_uoffset_at(meta, slot).ok_or("schema")?;
            if u32_at(meta, vector) != Some(2) || i32_at(meta, vector + 4) != Some(0)
                || i32_at(meta, vector + 8) != Some(1) { return Err("unsupported"); }
            Ok(plain("union(dense,0:utf8,1:int64)"))
        }
        _ => Err("unsupported"),
    }
}

fn list_type(meta: &[u8], children: &[usize]) -> Result<NamedType, &'static str> {
    let [child] = children else {
        return Err("unsupported");
    };
    if field_slot(meta, *child, 4).is_some() {
        return Err("unsupported");
    }
    if let Some(slot) = field_slot(meta, *child, 5) {
        let nested = read_offset_vector(meta, slot)?;
        if !nested.is_empty() {
            return Err("unsupported");
        }
    }
    let _name =
        read_string_at(meta, field_slot(meta, *child, 0).ok_or("schema")?).ok_or("schema")?;
    let nullable = match field_slot(meta, *child, 1) {
        None => false,
        Some(pos) => read_bool_at(meta, pos).ok_or("schema")?,
    };
    let type_byte = read_u8_field(meta, *child, 2).ok_or("unsupported")?;
    let type_table = field_slot(meta, *child, 3).and_then(|pos| read_uoffset_at(meta, pos));
    let name = match type_byte {
        2 => {
            let typed = int_type(meta, type_table.ok_or("schema")?)?;
            if typed.arrow_type == "int64" && !nullable {
                "list_int64"
            } else {
                return Err("unsupported");
            }
        }
        4 if type_table.is_some() => "list_binary",
        5 if type_table.is_some() => "list(utf8)",
        _ => return Err("unsupported"),
    };
    Ok(plain(name))
}

fn plain(name: &str) -> NamedType {
    NamedType {
        arrow_type: name.to_string(),
        precision: None,
        scale: None,
        timezone: None,
    }
}

fn int_type(meta: &[u8], table: usize) -> Result<NamedType, &'static str> {
    let width = read_i32_field(meta, table, 0).ok_or("unsupported")?;
    // Flatbuffer bool default is false. An absent is_signed slot is unsigned.
    let signed = match field_slot(meta, table, 1) {
        None => false,
        Some(pos) => read_bool_at(meta, pos).ok_or("schema")?,
    };
    let name = match (signed, width) {
        (true, 8) => "int8",
        (true, 16) => "int16",
        (true, 32) => "int32",
        (true, 64) => "int64",
        (false, 8) => "uint8",
        (false, 16) => "uint16",
        (false, 32) => "uint32",
        (false, 64) => "uint64",
        _ => return Err("unsupported"),
    };
    Ok(NamedType {
        arrow_type: name.to_string(),
        precision: Some(width),
        scale: None,
        timezone: None,
    })
}

fn decimal_type(meta: &[u8], table: usize) -> Result<NamedType, &'static str> {
    let precision = read_i32_field(meta, table, 0).ok_or("unsupported")?;
    // Arrow Decimal.scale defaults to 0; scale-zero sources omit the slot.
    let scale = read_i32_field(meta, table, 1).unwrap_or(0);
    // Arrow Decimal.bitWidth defaults to 128 when the slot is absent.
    let bits = read_i32_field(meta, table, 2).unwrap_or(128);
    let name = match bits {
        128 => "decimal128",
        256 => "decimal256",
        _ => return Err("unsupported"),
    };
    let limit = if bits == 128 { 38 } else { 76 };
    if !(1..=limit).contains(&precision) || !(0..=precision).contains(&scale) {
        return Err("unsupported");
    }
    Ok(NamedType {
        arrow_type: name.to_string(),
        precision: Some(precision),
        scale: Some(scale),
        timezone: None,
    })
}

fn date_type(meta: &[u8], table: usize) -> Result<NamedType, &'static str> {
    // Arrow DateUnit defaults to DAY (0); valid builders omit this slot.
    let unit = read_i16_field(meta, table, 0).unwrap_or(0);
    let name = match unit {
        0 => "date32",
        1 => "date64",
        _ => return Err("unsupported"),
    };
    Ok(plain(name))
}

fn field_slot(buf: &[u8], table: usize, index: usize) -> Option<usize> {
    let soffset = i32_at(buf, table)?;
    // FlatBuffers stores the signed distance from the table to its vtable.
    // A shared vtable can sit after the table, so the distance is negative.
    if soffset == 0 {
        return None;
    }
    let vtable = if soffset > 0 {
        table.checked_sub(soffset as usize)?
    } else {
        table.checked_add(soffset.unsigned_abs() as usize)?
    };
    let size = u16_at(buf, vtable)? as usize;
    if size < 4 || vtable.checked_add(size)? > buf.len() {
        return None;
    }
    let slot = 4 + index * 2;
    if slot + 2 > size {
        return None;
    }
    let rel = u16_at(buf, vtable + slot)? as usize;
    if rel == 0 {
        return None;
    }
    table.checked_add(rel)
}

fn read_uoffset_field(buf: &[u8], table: usize, index: usize) -> Option<usize> {
    read_uoffset_at(buf, field_slot(buf, table, index)?)
}

fn read_uoffset_at(buf: &[u8], pos: usize) -> Option<usize> {
    let rel = u32_at(buf, pos)? as usize;
    pos.checked_add(rel)
}

fn read_offset_vector(buf: &[u8], slot: usize) -> Result<Vec<usize>, &'static str> {
    let start = read_uoffset_at(buf, slot).ok_or("truncated")?;
    let count = u32_at(buf, start).ok_or("truncated")? as usize;
    if count > 64 {
        return Err("schema");
    }
    let mut tables = Vec::with_capacity(count);
    for index in 0..count {
        let elem = start + 4 + index * 4;
        tables.push(read_uoffset_at(buf, elem).ok_or("truncated")?);
    }
    Ok(tables)
}

fn read_string_at(buf: &[u8], slot: usize) -> Option<String> {
    let start = read_uoffset_at(buf, slot)?;
    let len = u32_at(buf, start)? as usize;
    let bytes = buf.get(start + 4..start + 4 + len)?;
    let text = std::str::from_utf8(bytes).ok()?;
    if token_ok(text) {
        Some(text.to_string())
    } else {
        None
    }
}

fn read_bool_at(buf: &[u8], pos: usize) -> Option<bool> {
    match buf.get(pos)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn read_u8_field(buf: &[u8], table: usize, index: usize) -> Option<u8> {
    buf.get(field_slot(buf, table, index)?).copied()
}

fn read_i16_field(buf: &[u8], table: usize, index: usize) -> Option<i16> {
    i16_at(buf, field_slot(buf, table, index)?)
}

fn read_i32_field(buf: &[u8], table: usize, index: usize) -> Option<i32> {
    i32_at(buf, field_slot(buf, table, index)?)
}

fn read_i64_field(buf: &[u8], table: usize, index: usize) -> Option<i64> {
    i64_at(buf, field_slot(buf, table, index)?)
}

fn u16_at(buf: &[u8], at: usize) -> Option<u16> {
    let bytes = buf.get(at..at + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(buf: &[u8], at: usize) -> Option<u32> {
    let bytes = buf.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn i16_at(buf: &[u8], at: usize) -> Option<i16> {
    let bytes = buf.get(at..at + 2)?;
    Some(i16::from_le_bytes([bytes[0], bytes[1]]))
}

fn i32_at(buf: &[u8], at: usize) -> Option<i32> {
    let bytes = buf.get(at..at + 4)?;
    Some(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn i64_at(buf: &[u8], at: usize) -> Option<i64> {
    let bytes = buf.get(at..at + 8)?;
    Some(i64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn align8(value: usize) -> Option<usize> {
    value.checked_add(7).map(|sum| sum & !7)
}

#[cfg(test)]
mod native_type_tests {
    use super::*;

    #[test]
    fn date_default_day_is_valid_and_unknown_unit_is_refused() {
        // Minimal valid FlatBuffers table: absent Date.unit uses DAY.
        let mut meta = vec![4, 0, 4, 0, 4, 0, 0, 0];
        assert_eq!(date_type(&meta, 4).unwrap().arrow_type, "date32");
        meta = vec![6, 0, 6, 0, 4, 0, 0, 0, 8, 0, 0, 0, 2, 0];
        assert!(date_type(&meta, 8).is_err());
    }

    #[test]
    fn pg_cursor_binds_actual_snapshot_position_and_observed_token() {
        let good = serde_json::json!({"protocol":"oo.postgres-preview.native-page-capture/v1", "snapshot_token":true,
            "product_reader_write_attempts":0, "snapshot":{"kind":"pg_snapshot","value":"0000000A-00000042-1"},
            "cursor":{"codec":"pg-export-snapshot/v1","position":"0000000A-00000042-1"}});
        let mut blocks = Vec::new();
        assert!(pg_cursor(&good).is_some());
        assert!(bind_writes(&good, true, &mut blocks).token_observed);
        assert!(blocks.is_empty());
        let mut wrong = good;
        wrong["cursor"]["position"] = serde_json::json!("0000000A-00000043-1");
        assert!(pg_cursor(&wrong).is_none());
        assert!(!bind_writes(&wrong, true, &mut blocks).token_observed);
        assert!(!blocks.is_empty());
    }

    #[test]
    fn r1_dba_objects_require_actual_raw_hash_and_zero_platform_writes() {
        let path = std::env::temp_dir().join(format!("oo-dl01w1-02-r1-test-{}", std::process::id()));
        std::fs::write(&path, b"unit-test DBA observation\n").unwrap();
        let digest = sha256_hex(&std::fs::read(&path).unwrap());
        let witness: Witness = serde_json::from_value(serde_json::json!({
            "protocol":WITNESS_PROTOCOL,"source_engine":"postgresql","source_id":"pg_test","source_generation":1,
            "snapshot":{"kind":"pg_snapshot","value":"A-B-1"},"run_id":"test","logical_schema":"public","logical_table":"test",
            "capturer_path":"/test","arrow_path":"/test","driver_path":"/test","schema_preimage_path":"/test","type_preimage_path":"/test"})).unwrap();
        let mut capture = serde_json::json!({"protocol":"oo.postgres-preview.native-page-capture/v1","capture_method":"C1","dba_objects":[]});
        for kind in ["publication", "replication_slot"] {
            capture["dba_objects"].as_array_mut().unwrap().push(serde_json::json!({"kind":kind,"name":"test",
                "created_by":"dba","observed":true,"platform_created":false,"platform_write_attempts":0,
                "risk":"retained WAL","rollback":"DBA removes only test objects",
                "observation_path":path.to_str().unwrap(),"observation_sha256":digest}));
        }
        assert!(r1_objects(&capture, &witness).is_some());
        capture["capture_method"] = serde_json::json!("C5");
        assert!(r1_objects(&capture, &witness).is_none());
        capture["capture_method"] = serde_json::json!("C1");
        capture["dba_objects"][0]["platform_write_attempts"] = serde_json::json!(1);
        assert!(r1_objects(&capture, &witness).is_none());
        capture["dba_objects"][0]["platform_write_attempts"] = serde_json::json!(0);
        capture["dba_objects"][0]["observation_sha256"] = serde_json::json!("0".repeat(64));
        assert!(r1_objects(&capture, &witness).is_none());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn record_batch_zero_length_uses_arrow_format_default() {
        let mut meta = vec![0u8; 68];
        meta[0..4].copy_from_slice(&32u32.to_le_bytes());
        meta[4..6].copy_from_slice(&12u16.to_le_bytes());
        meta[6..8].copy_from_slice(&12u16.to_le_bytes());
        meta[10..12].copy_from_slice(&4u16.to_le_bytes());
        meta[12..14].copy_from_slice(&8u16.to_le_bytes());
        meta[32..36].copy_from_slice(&28i32.to_le_bytes());
        meta[36] = 3;
        meta[40..44].copy_from_slice(&24u32.to_le_bytes());
        meta[56..58].copy_from_slice(&4u16.to_le_bytes());
        meta[58..60].copy_from_slice(&4u16.to_le_bytes());
        meta[64..68].copy_from_slice(&8i32.to_le_bytes());
        assert!(matches!(read_message(&meta).unwrap().header, Header::Batch(0)));
    }

    #[test]
    fn decimal_zero_scale_uses_arrow_default() {
        let meta = vec![6, 0, 8, 0, 4, 0, 0, 0, 8, 0, 0, 0, 20, 0, 0, 0];
        let typed = decimal_type(&meta, 8).unwrap();
        assert_eq!(typed.arrow_type, "decimal128");
        assert_eq!(typed.precision, Some(20));
        assert_eq!(typed.scale, Some(0));
    }

    #[test]
    fn native_source_metadata_must_match_observed_pg_column() {
        let columns: Vec<PreimageColumn> = serde_json::from_value(serde_json::json!([{"name":"id","arrow_type":"fixedsizebinary(16)",
            "nullable":false,"source_oid":2950,"source_typmod":-1,"source_format":"uuid"}])).unwrap();
        let mut capture = serde_json::json!({"protocol":"oo.postgres-preview.native-page-capture/v1","source_columns":[{
            "name":"id","arrow_type":"fixedsizebinary(16)","nullable":false,"source_oid":2950,"source_typmod":-1,"source_format":"uuid"}]});
        assert!(source_columns_agree(&capture, "postgresql", &columns));
        assert!(!source_columns_agree(&capture, "mysql", &columns));
        capture["source_columns"][0]["source_oid"] = serde_json::json!(17);
        assert!(!source_columns_agree(&capture, "postgresql", &columns));
    }

    #[test]
    fn bson_semantics_require_matching_type_preimage_and_arrow_storage() {
        let mut columns: Vec<PreimageColumn> = serde_json::from_value(serde_json::json!([{
            "name":"uid","arrow_type":"fixedsizebinary(16)","nullable":false,
            "bson_type":{"variant":"05","subtype":"04"}
        }])).unwrap();
        let mut types = serde_json::json!([{"table":"shipments","name":"uid","arrow_type":"fixedsizebinary(16)",
            "approval":"approved","bson_type":{"variant":"05","subtype":"04"}}]);
        assert!(type_rows_agree(&types, "shipments", &columns));
        columns[0].bson_types = Some(vec![BsonType { variant: "05".to_string(), subtype: Some("04".to_string()) }]);
        columns[0].bson_present_rows = Some(14);
        types[0]["bson_types"] = serde_json::json!([{"variant":"05","subtype":"04"}]);
        types[0]["bson_present_rows"] = serde_json::json!(14);
        assert!(type_rows_agree(&types, "shipments", &columns));
        types[0]["bson_type"]["subtype"] = serde_json::json!("00");
        assert!(!type_rows_agree(&types, "shipments", &columns));
        columns[0].arrow_type = "binary".to_string();
        let field = ArrowField { dictionary_id: None, name: "uid".to_string(), nullable: false,
            typed: plain("binary") };
        assert!(bind_column(&field, &columns[0]).is_err());
    }

    #[test]
    fn c4_partition_scope_binds_source_and_bytes_without_claiming_prior_ack() {
        assert!(producer_protocol("oo.clickhouse.capture-tick/v1"));
        let witness: Witness = serde_json::from_value(serde_json::json!({
            "protocol":WITNESS_PROTOCOL,"source_engine":"clickhouse","source_id":"clickhouse_268",
            "source_generation":1,"snapshot":{"kind":"seal","value":"test"},"run_id":"test",
            "logical_schema":"oo_dl01w40","logical_table":"readings","capturer_path":"/test",
            "arrow_path":"/test","driver_path":"/test","schema_preimage_path":"/test",
            "type_preimage_path":"/test"})).unwrap();
        let mut capture = serde_json::json!({"partition_scope":{
            "source_id":"clickhouse_268","source_generation":1,"logical_schema":"oo_dl01w40",
            "logical_table":"readings","partition_id":"202401",
            "observed_partition_ids":["202401","202403","209912"],"complete":true,
            "table_epoch":"3cad2d69-4353-4ab2-88b0-dd2ef92ee466","schema_fingerprint":"columns",
            "schema_digest":"a","type_digest":"b","arrow_sha256":"c","row_count":2,
            "current_signature":"202401|2|parts","mutations_pending":false}});
        let mut blocks = Vec::new();
        assert!(bind_partition_scope(&capture, &witness, "a", "b", "c", Some(2), &mut blocks).is_some());
        capture["partition_scope"]["previous_signature"] = serde_json::Value::Null;
        assert!(bind_partition_scope(&capture, &witness, "a", "b", "c", Some(2), &mut blocks).is_none());
        capture["partition_scope"].as_object_mut().unwrap().remove("previous_signature");
        capture["partition_scope"]["mutations_pending"] = serde_json::json!(true);
        assert!(bind_partition_scope(&capture, &witness, "a", "b", "c", Some(2), &mut blocks).is_none());
        capture["partition_scope"]["mutations_pending"] = serde_json::json!(false);
        assert!(bind_partition_scope(&capture, &witness, "a", "b", "wrong", Some(2), &mut blocks).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn sql_private_reference_requires_regular_0600_file() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("oo-dl01w1-02-private-ref-test-{}", std::process::id()));
        std::fs::write(&path, b"unit-test only").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(private_sql_file_reference(path.to_str().unwrap()));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!private_sql_file_reference(path.to_str().unwrap()));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn sqlserver_serializer_empty_stream_preserves_schema() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sqlserver/empty.arrow")).unwrap();
        let parsed = parse_arrow(&bytes).unwrap();
        assert_eq!(parsed.rows, 0);
        assert_eq!(parsed.fields.len(), 8);
    }

    #[test]
    fn clickhouse_absent_partition_schema_only_is_zero_rows() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/clickhouse/empty.arrow")).unwrap();
        let parsed = parse_arrow(&bytes).unwrap();
        assert_eq!(parsed.rows, 0);
        assert_eq!(parsed.fields.len(), 12);
        assert!(producer_protocol("oo.mysql.capture-tick/v1"));
    }

    #[test]
    fn sqlserver_time_and_uuid_keep_original_arrow_types() {
        // Time.unit=NANOSECOND, bitWidth=64.
        let time = vec![8, 0, 12, 0, 4, 0, 8, 0, 8, 0, 0, 0, 3, 0, 0, 0, 64, 0, 0, 0];
        assert_eq!(name_arrow_type(&time, 9, Some(8)).unwrap().arrow_type, "time64_nanos");
        // FixedSizeBinary.byteWidth=16; other widths are outside this source contract.
        let uuid = vec![6, 0, 8, 0, 4, 0, 0, 0, 8, 0, 0, 0, 16, 0, 0, 0];
        assert_eq!(name_arrow_type(&uuid, 15, Some(8)).unwrap().arrow_type, "fixedsizebinary(16)");
        let mut wrong = uuid;
        wrong[12] = 15;
        assert!(name_arrow_type(&wrong, 15, Some(8)).is_err());
    }

    #[test]
    fn mysql_duration_requires_microsecond_unit() {
        let mut duration = vec![8, 0, 12, 0, 4, 0, 8, 0, 8, 0, 0, 0, 2, 0, 0, 0];
        assert_eq!(name_arrow_type(&duration, 18, Some(8)).unwrap().arrow_type, "duration_micros");
        duration[12] = 1;
        assert!(name_arrow_type(&duration, 18, Some(8)).is_err());
        assert!(name_arrow_type(&duration, 18, None).is_err());
    }
}
