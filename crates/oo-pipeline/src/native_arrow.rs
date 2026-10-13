//! Native Arrow physical import.
//!
//! Protocol `oo.pipeline.native-arrow-import/v1` is the physical batch path.
//! `oo.pipeline.sealed-import-catalog/v1` is a semantic catalog and is refused
//! here. Admission is the observed readonly receipt `5df40d24` / `f96d3650`.
//! Decision/v1 is not given Arrow fields. A separate capture binding supplies
//! those digests when the operator registry is configured. This module does
//! not write an admission decision and does not mint `admitted`. A plan that
//! is not ready does not open PostgreSQL.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};
use oo_core::canonical::LogicalType;
use sqlx::postgres::PgConnection;
use sqlx::{ConnectOptions, Connection};

use crate::copy_encoder::{
    ArrowPhysical, ArrowScalar, CellPresence, ColumnPrivileges, CopyBudgets, CopyColumn, DecodedCell,
    ExactDecimal, ExactInt, LogicalValue, RunBinding, SourceColumn, TypeFacts, TypeGraph,
    TypeSupport, TypeUnsupportedReason, assemble_batch, map_column,
};
use crate::copy_sink::{self, SinkError};
use crate::full_import::{hook_digest, quality_preimage};
use crate::physical_mapping::{Level, MappingRequest, plan_physical_mapping, quote_physical_name};
use crate::native_binding::{self, BoundField, BoundObject};
use crate::target_installer::verify_full_import_readiness;

pub use crate::native_binding::{
    approved_admission_bin, check_admission_receipt, AdmissionReceiptKind, CaptureReceipt,
    ADMISSION_BIN_SHA256, ADMISSION_BIN_SHA256_3C55, ADMISSION_BIN_SHA256_E465,
    BINDING_REGISTRY_PROTOCOL,
    CAPTURE_BINDING_PROTOCOL, CAPTURE_RECEIPT_PROTOCOL, OperatorEnv,
};

/// Physical import request.
pub const IMPORT_PROTOCOL: &str = "oo.pipeline.native-arrow-import/v1";
/// Physical import result. `awaiting_publish` is set only after commit.
pub const RESULT_PROTOCOL: &str = "oo.pipeline.native-arrow-import-result/v1";
/// Semantic catalog. It has no Arrow batch payload.
pub const SEMANTIC_CATALOG_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";
/// MySQL sealed manifest. That path stays on `pg-builder import`.
pub const MYSQL_MANIFEST_PROTOCOL: &str = "oo.mysql-preview.manifest/v1";
/// Admission decision written by `oo-source-admission`.
pub const ADMISSION_DECISION_PROTOCOL: &str = "oo.source-admission.decision/v1";
/// Receipt schema of the readonly completion.
pub const ADMISSION_RECEIPT_SCHEMA: &str = "oo.native-readonly-completion/receipt/v1";

/// Content commit of the readonly admission.
pub const CONTENT_COMMIT: &str = "5df40d24eefa4fbcfa00c4415cfcfde5d368db89";
/// Seal commit that recorded that receipt.
pub const SEAL_COMMIT: &str = "f96d3650789bd1a0241247a255b1437ebd71a8cb";
/// SHA-256 of the sealed historical receipt file.
pub const RECEIPT_SHA256: &str = crate::native_binding::HISTORICAL_ADMISSION_RECEIPT_SHA256;

/// Frozen Oracle P2 stream. SCN 2175138, five rows. Not an admitted import.
pub const ORACLE_P2_BATCH_SHA256: &str =
    "fc547b45aa6743e243de59b16719cb9efffcdd403dfab47d5e9eaec4f65b523b";

const CLICKHOUSE_DECISION_SHA256: &str =
    "aedc6b434c4c5f2c9f2a4c4dd96258e4fd6dbdaeb2587da046dedc85add1ece7";
const OPENSEARCH_DECISION_SHA256: &str =
    "46eb2b48fdcc030728ea47b4e7fe68eac45ec4131ad662282241a263aa002953";

const MAX_ARROW: usize = 32 * 1024 * 1024;
const MAX_JSON: usize = 1024 * 1024;

/// One source the readonly receipt actually admitted, and the batches it binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmittedPin {
    /// SHA-256 of the `admit()` stdout.
    pub decision_sha256: &'static str,
    /// Decision `identity.source_id`.
    pub source_id: &'static str,
    /// Source engine named by that completion.
    pub source_engine: &'static str,
    /// Arrow batch digests named by that decision. Empty binds no physical batch.
    pub batches: &'static [&'static str],
}

/// Observed decisions from seal `f96d3650`. Neither decision names an Arrow batch.
pub fn admitted_pins() -> &'static [AdmittedPin] {
    const PINS: &[AdmittedPin] = &[
        AdmittedPin {
            decision_sha256: CLICKHOUSE_DECISION_SHA256,
            source_id: "clickhouse_268",
            source_engine: "clickhouse",
            batches: &[],
        },
        AdmittedPin {
            decision_sha256: OPENSEARCH_DECISION_SHA256,
            source_id: "oo_dl01w1_44",
            source_engine: "opensearch",
            batches: &[],
        },
    ];
    PINS
}

/// Why a stream or a column cannot be imported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowFault {
    /// Zero bytes, or a declared row count of zero.
    Empty,
    /// The stream ended before the end-of-stream marker or a body.
    Truncated,
    /// The type, name, nullability, or width is not the declared physical shape.
    Shape,
    /// Decimal precision or scale does not match the declaration.
    Precision,
}

impl ArrowFault {
    /// Closed reason token stored on the result.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Truncated => "truncated",
            Self::Shape => "shape",
            Self::Precision => "precision",
        }
    }
}

/// One decoded Arrow field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedField {
    /// Arrow field name.
    pub name: String,
    /// Flatbuffer nullable bit. Absent means false.
    pub nullable: bool,
    /// Closed physical kind.
    pub arrow_type: String,
    /// Decimal precision, integer bit width, or character length when known.
    pub precision: Option<i32>,
    /// Decimal scale.
    pub scale: Option<i32>,
    /// Timestamp timezone. Absent for a naive timestamp.
    pub timezone: Option<String>,
}

/// One decoded cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeCell {
    /// SQL NULL.
    Null,
    /// Signed integer. `bit_width` is 8, 16, 32, or 64.
    Int { bit_width: u8, value: i64 },
    /// Unscaled decimal coefficient.
    Decimal { precision: u8, scale: i8, coefficient: i128 },
    /// Microseconds since the Unix epoch. No timezone.
    TimestampMicros(i64),
    /// Microseconds since the Unix epoch, with the Arrow timezone string.
    TimestampMicrosUtc { value: i64, timezone: String },
    /// UTF-8 text, including CHAR padding.
    Utf8(String),
    /// Raw bytes.
    Binary(Vec<u8>),
    /// Boolean.
    Bool(bool),
    /// IEEE-754 bits.
    Float64(u64),
    /// Unsigned 64-bit integer. It is not sign-extended into [`Self::Int`].
    UInt64(u64),
    /// Unsigned 8-bit integer. Values 128..=255 stay positive. This is not a bool.
    UInt8(u8),
    /// Little-endian two's complement coefficient. Precision is above 38.
    Decimal256 {
        /// Arrow precision.
        precision: u8,
        /// Arrow scale.
        scale: i8,
        /// 32-byte little-endian coefficient.
        coefficient: [u8; 32],
    },
    /// Nanoseconds since the Unix epoch. The timezone is the Arrow string.
    TimestampNanos {
        /// Raw nanoseconds. Not divided into microseconds.
        value: i64,
        /// Arrow timezone. Only `UTC` is imported.
        timezone: String,
    },
    /// Canonical JSON text for a list value. The field type stays a list.
    Json(String),
    /// Days since the Unix epoch. Arrow Date32.
    Date32(i32),
}

/// One decoded stream. Row `i` is `columns[*][i]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedStream {
    /// Record-batch rows, in stream order.
    pub rows: usize,
    /// Schema fields.
    pub fields: Vec<DecodedField>,
    /// One vector per field.
    pub columns: Vec<Vec<NativeCell>>,
}

/// Column the caller declares for one physical unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyColumnDecl {
    /// Arrow field name.
    pub name: String,
    /// `pgident/v1` column.
    pub logical_name: String,
    /// Closed arrow type token.
    pub arrow_type: String,
    /// Nulls are allowed.
    pub nullable: bool,
    /// Decimal precision or integer precision.
    pub precision: Option<i32>,
    /// Decimal scale.
    pub scale: Option<i32>,
    /// `CHAR(n)` length. Absent means `varchar`.
    pub char_length: Option<i32>,
    /// Utf8 full document stored as jsonb oid 3802 typmod -1. Absent is varchar.
    pub document: bool,
}

/// Planned import. `ready` is set only when admission binds every batch digest.
pub struct Plan {
    /// Result document. `load_state` stays `not_sealed` until `load_verified` commits.
    pub result: serde_json::Value,
    ready: Option<VerifiedImport>,
}

impl Plan {
    /// True when admission, schema, and batch digests allow a loader call.
    pub fn loader_ready(&self) -> bool {
        self.ready.is_some()
    }
}

struct VerifiedImport {
    source_id: String,
    source_generation: i64,
    config_digest: String,
    schema_digest: String,
    type_digest: String,
    input_digest: String,
    tables: Vec<VerifiedTable>,
}

struct VerifiedTable {
    index: usize,
    logical_schema: String,
    logical_table: String,
    physical_name: String,
    row_count: u64,
    canonical_digest: String,
    payload_digest: String,
    applied_pos: String,
    columns: Vec<VerifiedColumn>,
    batch: crate::copy_encoder::TypedBatch,
    copy_columns: Vec<CopyColumn>,
    positions: Vec<ExactDecimal>,
}

pub(crate) struct VerifiedColumn {
    pub(crate) logical_name: String,
    pub(crate) physical_name: String,
    pub(crate) source_arrow: String,
    pub(crate) oid: i64,
    pub(crate) typmod: i32,
    pub(crate) not_null: bool,
}

/// Decode one Arrow IPC stream or file. This does not read a catalog and does not open PostgreSQL.
///
/// A stream starts at the first encapsulated message and must end on the end-of-stream marker.
/// A file starts and ends with `ARROW1`, uses only zero padding before the first message, and
/// places the footer after the messages. A terminal end-of-stream marker immediately before the
/// footer is accepted. An end-of-stream marker is still required for a stream.
pub fn decode_arrow_stream(bytes: &[u8]) -> Result<DecodedStream, ArrowFault> {
    if bytes.is_empty() {
        return Err(ArrowFault::Empty);
    }
    let (messages, require_eos) = frame_container(bytes)?;
    let mut schema: Option<Vec<FieldSpec>> = None;
    let mut columns: Vec<Vec<NativeCell>> = Vec::new();
    let mut rows = 0usize;
    let mut saw_eos = false;
    let mut dicts: BTreeMap<i64, Vec<NativeCell>> = BTreeMap::new();
    for message in &messages {
        if message.eos {
            saw_eos = true;
            continue;
        }
        let cursor = Cursor { data: message.meta };
        let root = root_table(&cursor)?;
        let header = root.u8_field(1)?.ok_or(ArrowFault::Shape)?;
        match header {
            1 => {
                if schema.is_some() {
                    return Err(ArrowFault::Shape);
                }
                schema = Some(read_schema(&root)?);
            }
            2 => {
                let fields = schema.as_ref().ok_or(ArrowFault::Shape)?;
                read_dictionary(&root, message.body, fields, &mut dicts)?;
            }
            3 => {
                let fields = schema.as_ref().ok_or(ArrowFault::Shape)?;
                let batch = read_record_batch(&root, message.body, fields, &dicts)?;
                if columns.is_empty() {
                    columns = batch;
                } else if batch.len() != columns.len() {
                    return Err(ArrowFault::Shape);
                } else {
                    for (into, more) in columns.iter_mut().zip(batch) {
                        into.extend(more);
                    }
                }
                rows = columns.first().map(Vec::len).unwrap_or(0);
            }
            4 | 5 => return Err(ArrowFault::Shape),
            _ => return Err(ArrowFault::Shape),
        }
    }
    if require_eos && !saw_eos {
        return Err(ArrowFault::Truncated);
    }
    let fields = schema.ok_or(ArrowFault::Shape)?;
    if fields.is_empty() {
        return Err(ArrowFault::Empty);
    }
    if rows == 0 && columns.is_empty() {
        columns = vec![Vec::new(); fields.len()];
    }
    if columns.len() != fields.len() || columns.iter().any(|column| column.len() != rows) {
        return Err(ArrowFault::Shape);
    }
    Ok(DecodedStream {
        rows,
        fields: fields.into_iter().map(FieldSpec::decoded).collect(),
        columns,
    })
}

/// Hash the COPY bytes for one decoded file. The digest is of the encoder output.
///
/// The file digest is the config and input identity for this helper. `plan_import`
/// uses the manifest digests instead.
pub fn copy_digest(
    bytes: &[u8],
    source_id: &str,
    schema: &str,
    table: &str,
    generation: u64,
    columns: &[CopyColumnDecl],
) -> Result<String, ArrowFault> {
    let digest = file_digest(bytes);
    let prepared = prepare_table(
        bytes,
        source_id,
        schema,
        table,
        generation,
        &hex_bytes(&digest),
        digest,
        "t0.b0",
        columns,
        None,
    )?;
    Ok(prepared.copy_hex)
}

/// Read the manifest, bind admission, and decode declared batches.
///
/// This function does not connect to PostgreSQL. `loader_ready` stays false
/// unless every batch digest is listed on the matched admission pin.
pub fn plan_import(document: &serde_json::Value) -> Plan {
    // The command path always reads the process environment. Callers that pass
    // `OperatorEnv::Fixed` are the process operator, not the manifest author.
    plan_with_operator(document, OperatorEnv::Process)
}

/// Plan with an explicit operator source.
///
/// `Absent` keeps the empty-pin path even when the process environment names a
/// registry. `Fixed` supplies the same three values as that environment. Neither
/// mode reads `admission_bin` or `registry_path` from the document.
pub fn plan_with_operator(document: &serde_json::Value, operator: OperatorEnv) -> Plan {
    native_binding::with_operator_env(operator, || match plan_inner(document) {
        Ok(plan) => plan,
        Err(stopped) => Plan {
            result: stopped,
            ready: None,
        },
    })
}

/// Install mappings, copy, seal, and commit on one transaction.
///
/// A failure rolls back. The returned id exists only after commit.
pub async fn load_verified(
    conn: &mut PgConnection,
    plan: &Plan,
    shard: &str,
    owner: &str,
) -> Result<String, LoadFault> {
    let ready = plan.ready.as_ref().ok_or(LoadFault::NotReady)?;
    verify_full_import_readiness(&mut *conn)
        .await
        .map_err(|_| LoadFault::NotReady)?;
    let mut tx = conn.begin().await.map_err(|_| LoadFault::NotReady)?;
    let loaded = {
        let mut writer = PgWriter { tx: &mut tx };
        apply(&mut writer, ready, shard, owner).await
    };
    match loaded {
        Ok(run_id) => tx.commit().await.map(|_| run_id).map_err(db_fault),
        Err(err) => {
            let _ = tx.rollback().await;
            Err(err)
        }
    }
}

/// Failure while opening or sealing a ready plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadFault {
    /// The plan was not ready, or `0005` readiness failed before a write.
    NotReady,
    /// The target rejected the transaction. The body is a SQLSTATE, or empty.
    Database(String),
    /// Encoder or binding failure. The transaction was rolled back.
    Refused(&'static str),
}

impl std::fmt::Display for LoadFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotReady => formatter.write_str("native import is not ready"),
            Self::Database(sqlstate) => write!(formatter, "native import database {sqlstate}"),
            Self::Refused(token) => write!(formatter, "native import refused {token}"),
        }
    }
}

impl std::error::Error for LoadFault {}

pub(crate) struct PreparedTable {
    pub(crate) copy_hex: String,
    pub(crate) canonical_digest: String,
    pub(crate) payload_digest: String,
    pub(crate) applied_pos: String,
    pub(crate) batch: crate::copy_encoder::TypedBatch,
    pub(crate) copy_columns: Vec<CopyColumn>,
    pub(crate) positions: Vec<ExactDecimal>,
    pub(crate) verified_columns: Vec<VerifiedColumn>,
    pub(crate) physical_name: String,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_table(
    bytes: &[u8],
    source_id: &str,
    schema: &str,
    table: &str,
    generation: u64,
    config_digest: &str,
    input_digest: [u8; 32],
    batch_id: &str,
    columns: &[CopyColumnDecl],
    bound_fields: Option<&[BoundField]>,
) -> Result<PreparedTable, ArrowFault> {
    let decoded = decode_arrow_stream(bytes)?;
    if decoded.fields.len() != columns.len() {
        return Err(ArrowFault::Shape);
    }
    let mut specs = Vec::with_capacity(columns.len());
    let mut cells = Vec::with_capacity(decoded.rows);
    for _ in 0..decoded.rows {
        cells.push(Vec::with_capacity(columns.len()));
    }
    let mut facts = Vec::new();
    let mut sources = Vec::new();
    let mut verified_columns = Vec::new();
    for (index, decl) in columns.iter().enumerate() {
        let field = &decoded.fields[index];
        if field.name != decl.name || field.nullable != decl.nullable {
            return Err(ArrowFault::Shape);
        }
        let mut target = target_of(decl, field)?;
        if let Some(bound) = bound_fields.and_then(|fields| fields.get(index)) {
            preserve_pg_storage(&mut target, bound)?;
            preserve_bson_storage(&mut target, bound)?;
        }
        facts.push(target.facts.clone());
        sources.push(source_column(decl, &target, i32::try_from(index + 1).unwrap_or(i32::MAX)));
        verified_columns.push(VerifiedColumn {
            logical_name: decl.logical_name.clone(),
            physical_name: String::new(),
            source_arrow: decl.arrow_type.clone(),
            oid: target.oid,
            typmod: target.typmod,
            not_null: !decl.nullable,
        });
        specs.push(target);
        for (row_index, cell) in decoded.columns[index].iter().enumerate() {
            cells[row_index].push(cell_of(cell, &specs[index])?);
        }
    }
    let graph = TypeGraph::new(facts);
    let plans: Vec<_> = sources
        .iter()
        .zip(&specs)
        .map(|(column, target)| {
            let mut plan = map_column(column, &graph, false);
            if matches!(target.kind, TargetKind::Interval) {
                // Native MySQL TIME is a signed duration. Generic PostgreSQL
                // interval mapping retains its months/days structure.
                plan.logical = Some(LogicalType::DurationMicros);
                plan.arrow = Some(ArrowPhysical::DurationMicros);
                plan.mapping.arrow_type = "duration[us]".to_string();
            }
            plan
        })
        .collect();
    if plans.iter().any(|plan| {
        !matches!(
            plan.mapping.decision,
            crate::copy_encoder::MapDecision::Lossless
        )
    }) {
        return Err(ArrowFault::Shape);
    }
    let binding = RunBinding {
        source_id: source_id.to_string(),
        schema: schema.to_string(),
        table: table.to_string(),
        run_generation: generation,
        input_digest,
    };
    let batch = assemble_batch(&binding, plans, &[], cells).map_err(|_| ArrowFault::Shape)?;
    let copy_columns: Vec<CopyColumn> = std::iter::once(CopyColumn::SystemPosition)
        .chain((1..=columns.len()).map(|attnum| CopyColumn::User {
            attnum: i32::try_from(attnum).unwrap_or(i32::MAX),
        }))
        .collect();
    let positions = (0..decoded.rows)
        .map(|index| ExactDecimal::new(false, index.to_string(), 0).map_err(|_| ArrowFault::Shape))
        .collect::<Result<Vec<_>, _>>()?;
    let budgets = CopyBudgets {
        max_value_bytes: 1_048_576,
        max_row_bytes: 1_048_576,
        max_chunk_bytes: 1_048_576,
        max_rows_per_chunk: 1024,
    };
    let hashed = copy_sink::hash_batch(&batch, &copy_columns, &positions, budgets)
        .map_err(|_| ArrowFault::Shape)?;
    let mut canonical = Sha256::new();
    for row in &batch.rows {
        canonical.update(row.canonical_hash);
    }
    let canonical_digest = hex_bytes(&canonical.finalize());
    let applied_pos = positions
        .last()
        .map(ExactDecimal::to_plain_string)
        .unwrap_or_else(|| "0".to_string());
    let quality = quality_preimage(batch_id, decoded.rows as u64, 0, &hex_bytes(&input_digest), &[]);
    let mut payload = hashed.hasher.clone();
    payload.update(&quality);
    let physical = plan_physical_mapping(&MappingRequest {
        source: source_id.to_string(),
        config_digest: config_digest.to_string(),
        run_generation: i64::try_from(generation).unwrap_or(0),
        level: Level::Table,
        logical_namespace: schema.to_string(),
        logical_table: table.to_string(),
        logical_column: String::new(),
    })
    .map_err(|_| ArrowFault::Shape)?;
    for (column, decl) in verified_columns.iter_mut().zip(columns) {
        let planned = plan_physical_mapping(&MappingRequest {
            source: source_id.to_string(),
            config_digest: config_digest.to_string(),
            run_generation: i64::try_from(generation).unwrap_or(0),
            level: Level::Column,
            logical_namespace: schema.to_string(),
            logical_table: table.to_string(),
            logical_column: mapping_ident(&decl.logical_name).map_err(|_| ArrowFault::Shape)?,
        })
        .map_err(|_| ArrowFault::Shape)?;
        column.physical_name = planned.physical_name;
    }
    Ok(PreparedTable {
        copy_hex: hex_bytes(&hashed.hasher.clone().finalize()),
        canonical_digest,
        payload_digest: hex_bytes(&payload.finalize()),
        applied_pos,
        batch,
        copy_columns,
        positions,
        verified_columns,
        physical_name: physical.physical_name,
    })
}

struct TargetType {
    oid: i64,
    typmod: i32,
    type_name: &'static str,
    facts: TypeFacts,
    kind: TargetKind,
}

#[derive(Clone, Copy)]
enum TargetKind {
    Int { width: u8 },
    Decimal { precision: i32, scale: i32 },
    Decimal256 { precision: i32, scale: i32 },
    UInt64,
    UInt8,
    Nanos,
    Text { char_length: Option<i32> },
    Bytes,
    FixedBytes { width: usize },
    BsonObjectId,
    BsonUuid,
    Json,
    Jsonb,
    Timestamp,
    TimestampUtc,
    Time,
    Interval,
    Bool,
    Float64,
    Date,
}

fn preserve_pg_storage(target: &mut TargetType, field: &BoundField) -> Result<(), ArrowFault> {
    let Some(oid) = field.source_oid else { return Ok(()); };
    let typmod = field.source_typmod.ok_or(ArrowFault::Shape)?;
    field.source_format.as_ref().filter(|value| !value.is_empty()).ok_or(ArrowFault::Shape)?;
    let (name, send, len) = match (oid, &target.kind) {
        (2950, TargetKind::FixedBytes { width: 16 }) if typmod == -1 => ("uuid", "uuid_send", 16),
        (25, TargetKind::Text { char_length: None }) if typmod == -1 => ("text", "textsend", -1),
        (2950 | 25, _) => return Err(ArrowFault::Shape),
        _ => return Ok(()),
    };
    target.oid = i64::from(oid);
    target.typmod = typmod;
    target.type_name = name;
    target.facts.oid = i64::from(oid);
    target.facts.name = name.to_string();
    target.facts.typlen = len;
    target.facts.typsend_name = send.to_string();
    Ok(())
}

fn preserve_bson_storage(target: &mut TargetType, field: &BoundField) -> Result<(), ArrowFault> {
    let Some(bson) = field.bson_type.as_ref() else { return Ok(()); };
    let kind = match (bson.variant.as_str(), bson.subtype.as_deref(), target.kind) {
        ("07", None, TargetKind::FixedBytes { width: 12 }) => Some(TargetKind::BsonObjectId),
        ("05", Some("04"), TargetKind::FixedBytes { width: 16 }) => Some(TargetKind::BsonUuid),
        ("05", Some("00"), TargetKind::Bytes | TargetKind::FixedBytes { .. }) => None,
        ("07" | "05", _, _) => return Err(ArrowFault::Shape),
        _ => None,
    };
    if let Some(kind) = kind { *target = target_type(kind)?; }
    Ok(())
}

fn target_of(decl: &CopyColumnDecl, field: &DecodedField) -> Result<TargetType, ArrowFault> {
    if decl.document && decl.arrow_type != "utf8" {
        return Err(ArrowFault::Shape);
    }
    if let Some(kind) = decimal_target(decl, field)? {
        return target_type(kind);
    }
    let kind = match decl.arrow_type.as_str() {
        "int8" if field.arrow_type == "int8" => signed_int(decl, field, 8)?,
        "int16" if field.arrow_type == "int16" => signed_int(decl, field, 16)?,
        "int32" if field.arrow_type == "int32" => signed_int(decl, field, 32)?,
        "int64" if field.arrow_type == "int64" => signed_int(decl, field, 64)?,
        "time64_micros" if field.arrow_type == "time64_micros" => TargetKind::Time,
        "duration_micros" if field.arrow_type == "duration_micros" => TargetKind::Interval,
        "time64_nanos" if field.arrow_type == "time64_nanos" => TargetKind::Int { width: 64 },
        "uint64" if field.arrow_type == "uint64" && field.precision == Some(64) => {
            if decl.precision != Some(64) || decl.scale.is_some() {
                return Err(ArrowFault::Shape);
            }
            TargetKind::UInt64
        }
        "timestamp_nanos_utc"
            if field.arrow_type == "timestamp_nanos_utc" && field.timezone.as_deref() == Some("UTC") =>
        {
            if decl.precision.is_some() || decl.scale.is_some() {
                return Err(ArrowFault::Shape);
            }
            TargetKind::Nanos
        }
        "dictionary_binary" | "dictionary_int32_binary" if field.arrow_type == "dictionary_binary" => TargetKind::Bytes,
        "list_int64" if field.arrow_type == "list_int64" => TargetKind::Json,
        "list_binary" if field.arrow_type == "list_binary" => TargetKind::Json,
        "utf8" if field.arrow_type == "utf8" => {
            if decl.document {
                if decl.precision.is_some() || decl.scale.is_some() || decl.char_length.is_some() {
                    return Err(ArrowFault::Shape);
                }
                TargetKind::Jsonb
            } else {
                TargetKind::Text {
                    char_length: decl.char_length,
                }
            }
        }
        "binary" if field.arrow_type == "binary" => TargetKind::Bytes,
        "timestamp_micros" if field.arrow_type == "timestamp_micros" && field.timezone.is_none() => {
            TargetKind::Timestamp
        }
        "timestamp_second_naive"
            if field.arrow_type == "timestamp_second_naive" && field.timezone.is_none() =>
        {
            if decl.precision.is_some() || decl.scale.is_some() {
                return Err(ArrowFault::Precision);
            }
            TargetKind::Timestamp
        }
        "timestamp_second_utc"
            if field.arrow_type == "timestamp_second_utc" && field.timezone.as_deref() == Some("UTC") =>
        {
            if decl.precision.is_some() || decl.scale.is_some() {
                return Err(ArrowFault::Precision);
            }
            TargetKind::TimestampUtc
        }
        "timestamp_millis_naive"
            if field.arrow_type == "timestamp_millis_naive" && field.timezone.is_none() =>
        {
            if decl.precision.is_some() || decl.scale.is_some() {
                return Err(ArrowFault::Precision);
            }
            TargetKind::Timestamp
        }
        "date32" if field.arrow_type == "date32" && field.timezone.is_none() => {
            if decl.precision.is_some() || decl.scale.is_some() {
                return Err(ArrowFault::Precision);
            }
            TargetKind::Date
        }
        "date64" if field.arrow_type == "date64" && field.timezone.is_none() => {
            if decl.precision.is_some() || decl.scale.is_some() {
                return Err(ArrowFault::Precision);
            }
            TargetKind::Timestamp
        }
        "timestamp_micros_utc"
            if field.arrow_type == "timestamp_micros_utc" && field.timezone.is_some() =>
        {
            TargetKind::TimestampUtc
        }
        "bool" if field.arrow_type == "bool" => TargetKind::Bool,
        "float64" if field.arrow_type == "float64" => TargetKind::Float64,
        "uint8" if field.arrow_type == "uint8" => {
            if decl.scale.is_some() || decl.precision.is_some_and(|value| value != 8) {
                return Err(ArrowFault::Shape);
            }
            TargetKind::UInt8
        }
        "timestamp(ms,utc)" if field.arrow_type == "timestamp(ms,utc)" && field.timezone.is_none() => {
            if decl.precision.is_some() || decl.scale.is_some() {
                return Err(ArrowFault::Shape);
            }
            TargetKind::TimestampUtc
        }
        "list(utf8)" if field.arrow_type == "list(utf8)" => {
            if decl.precision.is_some() || decl.scale.is_some() {
                return Err(ArrowFault::Shape);
            }
            TargetKind::Json
        }
        other if fixed_binary_width(other).is_some() => {
            let width = fixed_binary_width(other).ok_or(ArrowFault::Shape)?;
            if field.arrow_type != other || !matches!(width, 12 | 16) || decl.precision.is_some() || decl.scale.is_some()
            {
                return Err(ArrowFault::Shape);
            }
            TargetKind::FixedBytes {
                width: usize::try_from(width).map_err(|_| ArrowFault::Shape)?,
            }
        }
        other if (other.starts_with("struct(") || other.starts_with("union(dense,")) && other == field.arrow_type => {
            if decl.precision.is_some() || decl.scale.is_some() {
                return Err(ArrowFault::Shape);
            }
            TargetKind::Json
        }
        "int8" | "int16" | "int32" | "int64" | "uint64" | "uint8" | "decimal128" | "decimal256" | "utf8"
        | "binary" | "dictionary_binary" | "dictionary_int32_binary" | "timestamp_micros" | "timestamp_micros_utc"
        | "timestamp_second_naive" | "timestamp_second_utc" | "timestamp_millis_naive" | "date32"
        | "date64" | "timestamp_nanos_utc" | "timestamp(ms,utc)" | "bool" | "float64" | "list_int64"
        | "list_binary" | "list(utf8)" | "fixedsizebinary(12)" | "fixedsizebinary(16)"
        | "time64_micros" | "time64_nanos" | "duration_micros" => {
            return Err(ArrowFault::Shape);
        }
        _ => return Err(ArrowFault::Shape),
    };
    target_type(kind)
}

fn signed_int(decl: &CopyColumnDecl, field: &DecodedField, width: i32) -> Result<TargetKind, ArrowFault> {
    if field.precision != Some(width) || decl.scale.is_some() || decl.precision.is_some_and(|value| value != width) {
        return Err(ArrowFault::Precision);
    }
    let width = u8::try_from(width).map_err(|_| ArrowFault::Shape)?;
    Ok(TargetKind::Int { width })
}

fn decimal_target(decl: &CopyColumnDecl, field: &DecodedField) -> Result<Option<TargetKind>, ArrowFault> {
    let spelled = parse_decimal_spell(&decl.arrow_type);
    let family = if decl.arrow_type == "decimal128" || decl.arrow_type == "decimal256" {
        decl.arrow_type.as_str()
    } else if let Some((name, _, _)) = spelled {
        name
    } else {
        return Ok(None);
    };
    if field.arrow_type != family {
        return Err(ArrowFault::Shape);
    }
    let precision = decl.precision.ok_or(ArrowFault::Precision)?;
    let scale = decl.scale.ok_or(ArrowFault::Precision)?;
    if let Some((_, spelled_precision, spelled_scale)) = spelled
        && (spelled_precision != precision || spelled_scale != scale)
    {
        return Err(ArrowFault::Precision);
    }
    if field.precision != Some(precision) || field.scale != Some(scale) {
        return Err(ArrowFault::Precision);
    }
    let limit = if family == "decimal128" { 38 } else { 76 };
    if !(1..=limit).contains(&precision) || !(0..=precision).contains(&scale) {
        return Err(ArrowFault::Precision);
    }
    if family == "decimal128" {
        Ok(Some(TargetKind::Decimal { precision, scale }))
    } else {
        Ok(Some(TargetKind::Decimal256 { precision, scale }))
    }
}

fn target_type(kind: TargetKind) -> Result<TargetType, ArrowFault> {
    let (oid, typmod, typlen, send, type_name) = match kind {
        TargetKind::Int { width: 8 | 16 } => (21, -1, 2, "int2send", "int2"),
        TargetKind::Int { width: 32 } => (23, -1, 4, "int4send", "int4"),
        TargetKind::Int { width: 64 } => (20, -1, 8, "int8send", "int8"),
        TargetKind::Int { .. } => return Err(ArrowFault::Shape),
        TargetKind::Decimal { precision, scale } | TargetKind::Decimal256 { precision, scale } => {
            (1700, numeric_typmod(precision, scale), -1, "numeric_send", "numeric")
        }
        TargetKind::UInt64 | TargetKind::Nanos => (1700, numeric_typmod(20, 0), -1, "numeric_send", "numeric"),
        TargetKind::UInt8 => (21, -1, 2, "int2send", "int2"),
        TargetKind::Json => (114, -1, -1, "json_send", "json"),
        TargetKind::Jsonb => (3802, -1, -1, "jsonb_send", "jsonb"),
        TargetKind::Text { char_length: Some(len) } => {
            if !(1..=63).contains(&len) {
                return Err(ArrowFault::Shape);
            }
            (1042, len + 4, -1, "bpcharsend", "bpchar")
        }
        TargetKind::Text { char_length: None } => (1043, -1, -1, "varcharsend", "varchar"),
        TargetKind::Bytes | TargetKind::FixedBytes { .. } => (17, -1, -1, "byteasend", "bytea"),
        TargetKind::BsonObjectId => (25, -1, -1, "textsend", "text"),
        TargetKind::BsonUuid => (2950, -1, 16, "uuid_send", "uuid"),
        TargetKind::Timestamp => (1114, -1, 8, "timestamp_send", "timestamp"),
        TargetKind::TimestampUtc => (1184, -1, 8, "timestamptz_send", "timestamptz"),
        TargetKind::Time => (1083, -1, 8, "time_send", "time"),
        TargetKind::Interval => (1186, -1, 16, "interval_send", "interval"),
        TargetKind::Date => (1082, -1, 4, "date_send", "date"),
        TargetKind::Bool => (16, -1, 1, "boolsend", "bool"),
        TargetKind::Float64 => (701, -1, 8, "float8send", "float8"),
    };
    Ok(TargetType {
        oid,
        typmod,
        type_name,
        facts: TypeFacts {
            oid,
            name: type_name.to_string(),
            schema: "pg_catalog".to_string(),
            typtype: "b".to_string(),
            typlen,
            typelem: 0,
            typbasetype: 0,
            typtypmod: -1,
            typsend_oid: 0,
            typsend_name: send.to_string(),
            typsend_schema: "pg_catalog".to_string(),
        },
        kind,
    })
}

fn source_column(decl: &CopyColumnDecl, target: &TargetType, attnum: i32) -> SourceColumn {
    SourceColumn {
        attnum,
        name: decl.name.clone(),
        name_ident: decl.logical_name.clone(),
        type_oid: target.oid,
        type_schema: "pg_catalog".to_string(),
        type_name: target.type_name.to_string(),
        typtype: "b".to_string(),
        typmod: target.typmod,
        base_type_oid: None,
        numeric_precision: decl.precision,
        numeric_scale: decl.scale,
        not_null: !decl.nullable,
        collation_oid: 0,
        collation_name: None,
        identity: String::new(),
        generated: String::new(),
        default_expr: None,
        comment: None,
        enum_labels: Vec::new(),
        domain_checks: Vec::new(),
        type_support: TypeSupport {
            converted: false,
            reason: TypeUnsupportedReason::KnownBuiltinNotConverted,
        },
        current_privileges: ColumnPrivileges {
            select: Some(true),
            insert: None,
            update: None,
        },
    }
}

fn cell_of(cell: &NativeCell, target: &TargetType) -> Result<DecodedCell, ArrowFault> {
    if matches!(cell, NativeCell::Null) {
        return Ok(DecodedCell::sql_null());
    }
    let (value, arrow, precision, scale) = match (cell, target.kind) {
        (NativeCell::Int { bit_width: 64, value }, TargetKind::Time)
            if (0..86_400_000_000).contains(value) => {
                (LogicalValue::TimeMicros(*value), ArrowScalar::Time64(*value), None, None)
            }
        (NativeCell::Int { bit_width: 64, value }, TargetKind::Interval) => {
            (LogicalValue::DurationMicros(*value), ArrowScalar::DurationMicros(*value), None, None)
        }
        (NativeCell::Int { bit_width, value }, TargetKind::Int { width }) if *bit_width == width => {
            let number = ExactInt::parse(&value.to_string()).map_err(|_| ArrowFault::Shape)?;
            let arrow = match width {
                8 | 16 => ArrowScalar::Int16(i16::try_from(*value).map_err(|_| ArrowFault::Shape)?),
                32 => ArrowScalar::Int32(i32::try_from(*value).map_err(|_| ArrowFault::Shape)?),
                _ => ArrowScalar::Int64(*value),
            };
            (LogicalValue::Int(number), arrow, None, None)
        }
        (
            NativeCell::Decimal {
                precision,
                scale,
                coefficient,
            },
            TargetKind::Decimal {
                precision: declared,
                scale: declared_scale,
            },
        ) => {
            if i32::from(*precision) != declared || i32::from(*scale) != declared_scale {
                return Err(ArrowFault::Precision);
            }
            let exact = ExactDecimal::new(*coefficient < 0, coefficient.unsigned_abs().to_string(), i32::from(*scale))
                .map_err(|_| ArrowFault::Precision)?;
            if exact.coefficient().len() > usize::try_from(declared).unwrap_or(0) {
                return Err(ArrowFault::Precision);
            }
            let actual_precision = i32::try_from(exact.storage_precision()).unwrap_or(i32::MAX);
            (
                LogicalValue::Decimal(exact),
                ArrowScalar::Decimal128(*coefficient),
                Some(actual_precision),
                Some(i32::from(*scale)),
            )
        }
        (
            NativeCell::Decimal256 {
                precision,
                scale,
                coefficient,
            },
            TargetKind::Decimal256 {
                precision: declared,
                scale: declared_scale,
            },
        ) => {
            if i32::from(*precision) != declared || i32::from(*scale) != declared_scale {
                return Err(ArrowFault::Precision);
            }
            let (negative, digits) = i256_digits(coefficient);
            let exact = ExactDecimal::new(negative, digits, i32::from(*scale)).map_err(|_| ArrowFault::Precision)?;
            if exact.coefficient().len() > usize::try_from(declared).unwrap_or(0) {
                return Err(ArrowFault::Precision);
            }
            let actual_precision = i32::try_from(exact.storage_precision()).unwrap_or(i32::MAX);
            (
                LogicalValue::Decimal(exact),
                ArrowScalar::Decimal256(*coefficient),
                Some(actual_precision),
                Some(i32::from(*scale)),
            )
        }
        (NativeCell::UInt64(value), TargetKind::UInt64) => {
            let exact = ExactDecimal::new(false, value.to_string(), 0).map_err(|_| ArrowFault::Precision)?;
            let coefficient = i128::from(*value);
            let actual_precision = i32::try_from(exact.storage_precision()).unwrap_or(i32::MAX);
            (
                LogicalValue::Decimal(exact),
                ArrowScalar::Decimal128(coefficient),
                Some(actual_precision),
                Some(0),
            )
        }
        (NativeCell::TimestampNanos { value, .. }, TargetKind::Nanos) => {
            let negative = *value < 0;
            let exact = ExactDecimal::new(negative, value.unsigned_abs().to_string(), 0)
                .map_err(|_| ArrowFault::Precision)?;
            let actual_precision = i32::try_from(exact.storage_precision()).unwrap_or(i32::MAX);
            (
                LogicalValue::Decimal(exact),
                ArrowScalar::Decimal128(i128::from(*value)),
                Some(actual_precision),
                Some(0),
            )
        }
        (NativeCell::Json(text), TargetKind::Json) => {
            (LogicalValue::Text(text.clone()), ArrowScalar::Utf8(text.clone()), None, None)
        }
        (NativeCell::Utf8(text), TargetKind::Jsonb) => {
            if text.as_bytes().contains(&0) {
                return Err(ArrowFault::Shape);
            }
            let node = oo_core::canonical::parse_json(text).map_err(|_| ArrowFault::Shape)?;
            let mut wire = Vec::with_capacity(text.len().saturating_add(1));
            wire.push(oo_core::jsonb_doc::JSONB_SEND_VERSION);
            wire.extend_from_slice(text.as_bytes());
            oo_core::jsonb_doc::inspect_jsonb_send(&wire).map_err(|_| ArrowFault::Shape)?;
            (LogicalValue::Json(node), ArrowScalar::Utf8(text.clone()), None, None)
        }
        (NativeCell::Utf8(text), TargetKind::Text { char_length }) => {
            if text.as_bytes().contains(&0) {
                return Err(ArrowFault::Shape);
            }
            if let Some(limit) = char_length {
                let count = i32::try_from(text.chars().count()).unwrap_or(i32::MAX);
                if count != limit {
                    return Err(ArrowFault::Shape);
                }
            }
            (LogicalValue::Text(text.clone()), ArrowScalar::Utf8(text.clone()), None, None)
        }
        (NativeCell::Binary(bytes), TargetKind::Bytes) => (
            LogicalValue::Bytes(bytes.clone()),
            ArrowScalar::Binary(bytes.clone()),
            None,
            None,
        ),
        (NativeCell::Binary(bytes), TargetKind::FixedBytes { width }) if bytes.len() == width => (
            LogicalValue::Bytes(bytes.clone()),
            ArrowScalar::Binary(bytes.clone()),
            None,
            None,
        ),
        (NativeCell::Binary(bytes), TargetKind::BsonObjectId) if bytes.len() == 12 => {
            let value = hex_bytes(bytes);
            (LogicalValue::Text(value.clone()), ArrowScalar::Utf8(value), None, None)
        }
        (NativeCell::Binary(bytes), TargetKind::BsonUuid) if bytes.len() == 16 => (
            LogicalValue::Bytes(bytes.clone()), ArrowScalar::Binary(bytes.clone()), None, None,
        ),
        (NativeCell::UInt8(value), TargetKind::UInt8) => {
            let number = ExactInt::parse(&value.to_string()).map_err(|_| ArrowFault::Shape)?;
            (
                LogicalValue::Int(number),
                ArrowScalar::Int16(i16::from(*value)),
                None,
                None,
            )
        }
        (NativeCell::Date32(days), TargetKind::Date) => {
            let (year, month, day) = civil_from_unix_days(i64::from(*days)).ok_or(ArrowFault::Precision)?;
            oo_core::canonical::validate_date(year, month, day).map_err(|_| ArrowFault::Precision)?;
            (
                LogicalValue::Date { year, month, day },
                ArrowScalar::Date32(*days),
                None,
                None,
            )
        }
        (NativeCell::TimestampMicros(value), TargetKind::Timestamp) => {
            let stamp = civil_timestamp(*value).ok_or(ArrowFault::Shape)?;
            (
                LogicalValue::TimestampNaive {
                    year: stamp.0,
                    month: stamp.1,
                    day: stamp.2,
                    hour: stamp.3,
                    minute: stamp.4,
                    second: stamp.5,
                    micros: stamp.6,
                },
                ArrowScalar::TimestampNaive(*value),
                None,
                None,
            )
        }
        (
            NativeCell::TimestampMicrosUtc { value, .. },
            TargetKind::TimestampUtc,
        ) => (
            LogicalValue::TimestampTzMicros(*value),
            ArrowScalar::TimestampUtc(*value),
            None,
            None,
        ),
        (NativeCell::Bool(value), TargetKind::Bool) => {
            (LogicalValue::Bool(*value), ArrowScalar::Bool(*value), None, None)
        }
        (NativeCell::Float64(bits), TargetKind::Float64) => (
            LogicalValue::Float(oo_core::canonical::classify_f64(f64::from_bits(*bits))),
            ArrowScalar::Float64(*bits),
            None,
            None,
        ),
        _ => return Err(ArrowFault::Shape),
    };
    Ok(DecodedCell {
        presence: CellPresence::Value,
        value: Some(value),
        arrow: Some(arrow),
        reject: None,
        actual_precision: precision,
        actual_scale: scale,
        float_bits: match cell {
            NativeCell::Float64(bits) => Some(*bits),
            _ => None,
        },
    })
}

pub(crate) fn column_provenance_for(oid: i64, arrow_type: &str) -> &'static str {
    if oid == 3802 {
        return "full document stored as jsonb oid 3802 typmod -1; missing keys stay absent; numbers stay exact; not json oid 114";
    }
    column_provenance(arrow_type)
}

pub(crate) fn column_provenance(arrow_type: &str) -> &'static str {
    if let Some((family, _, _)) = parse_decimal_spell(arrow_type) {
        return match family {
            "decimal128" => "decimal128 stored as numeric",
            "decimal256" => "decimal256 stored as numeric; coefficient is not rounded",
            _ => "closed",
        };
    }
    if let Some(width) = fixed_binary_width(arrow_type) {
        return match width {
            12 => "fixed-size binary width 12 stored as bytea; not utf8",
            16 => "fixed-size binary width 16 stored as bytea; not utf8",
            _ => "closed",
        };
    }
    if arrow_type.starts_with("struct(") && arrow_type.ends_with(')') {
        return "struct stored as canonical json text; every child stays in schema order; source type remains struct; not jsonb";
    }
    if arrow_type.starts_with("union(dense,") && arrow_type.ends_with(')') {
        return "dense union stored as canonical json text; type id, child type, and value stay; not jsonb";
    }
    match arrow_type {
        "uint64" => "uint64 stored as numeric(20,0); source width 64",
        "uint8" => "uint8 stored as int2; source width 8 unsigned; not bool",
        "decimal256" => "decimal256 stored as numeric; coefficient is not rounded",
        "timestamp_nanos_utc" => "nanoseconds since epoch stored as numeric(20,0); not timestamptz",
        "timestamp(ms,utc)" => "timestamp milliseconds UTC stored as timestamptz; microseconds are milliseconds * 1000",
        "dictionary_binary" | "dictionary_int32_binary" => "dictionary Int32 binary resolved to bytea oid 17; not utf8; not text",
        "binary" => "arrow binary stored as bytea oid 17; not utf8; not text",
        "list_int64" => "list of int64 stored as canonical json; source type remains list",
        "list_binary" => "list of binary stored as canonical json; source type remains list",
        "time64_micros" => "microseconds since midnight stored as time; out-of-day values refused",
        "duration_micros" => "signed microseconds stored as interval with zero months and days",
        "time64_nanos" => "nanoseconds since midnight stored as bigint exactly; not time(6)",
        "list(utf8)" => "list of utf8 stored as canonical json text; source type remains list; not jsonb",
        "int64" => "int64; arrow bit width 64",
        "int32" => "int32; arrow bit width 32",
        "int16" => "int16; arrow bit width 16",
        "int8" => "int8; arrow bit width 8",
        "decimal128" => "decimal128 stored as numeric",
        "utf8" => "utf8 stored as varchar oid 1043; not bytea; not text",
        "date32" => "date32 days since unix epoch stored as date",
        "date64" => "date64 milliseconds since unix epoch stored as timestamp; microseconds are milliseconds * 1000",
        "timestamp_second_naive" => "timestamp seconds since unix epoch stored as timestamp; microseconds are seconds * 1000000",
        "timestamp_second_utc" => "timestamp seconds since unix epoch with timezone UTC stored as timestamptz",
        "timestamp_millis_naive" => "timestamp milliseconds since unix epoch stored as timestamp; microseconds are milliseconds * 1000",
        "timestamp_micros" => "timestamp microseconds",
        "timestamp_micros_utc" => "timestamp microseconds with timezone",
        "bool" => "bool",
        "float64" => "float64",
        _ => "closed",
    }
}

pub(crate) fn parse_decimal_spell(token: &str) -> Option<(&str, i32, i32)> {
    let (name, rest) = token.split_once('(')?;
    if name != "decimal128" && name != "decimal256" {
        return None;
    }
    let inner = rest.strip_suffix(')')?;
    let (raw_precision, raw_scale) = inner.split_once(',')?;
    if raw_precision.is_empty()
        || raw_scale.is_empty()
        || raw_precision.starts_with('+')
        || raw_scale.starts_with('+')
        || !raw_precision.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-')
        || !raw_scale.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        return None;
    }
    let precision = raw_precision.parse().ok()?;
    let scale = raw_scale.parse().ok()?;
    Some((name, precision, scale))
}

pub(crate) fn fixed_binary_width(token: &str) -> Option<i32> {
    let rest = token.strip_prefix("fixedsizebinary(")?.strip_suffix(')')?;
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

fn arrow_tokens_agree(got: &DecodedField, declared: &str) -> bool {
    if got.arrow_type == declared {
        return true;
    }
    if got.arrow_type == "dictionary_binary" && declared == "dictionary_int32_binary" {
        return true;
    }
    match parse_decimal_spell(declared) {
        Some((name, precision, scale)) => {
            got.arrow_type == name && got.precision == Some(precision) && got.scale == Some(scale)
        }
        None => false,
    }
}

fn precision_scale_agree(got: &DecodedField, precision: Option<i32>, scale: Option<i32>) -> bool {
    let decimal = got.arrow_type == "decimal128" || got.arrow_type == "decimal256";
    if decimal {
        return precision.is_some() && scale.is_some() && got.precision == precision && got.scale == scale;
    }
    if let Some(width) = signed_width(&got.arrow_type) {
        // The Arrow field carries the bit width. A capture token may omit precision.
        // An explicit precision has to be that width. A missing width is not a match.
        if got.precision != Some(width) || scale.is_some() || precision.is_some_and(|value| value != width) {
            return false;
        }
        return true;
    }
    if let Some(value) = precision
        && got.precision != Some(value)
    {
        return false;
    }
    if let Some(value) = scale
        && got.scale != Some(value)
    {
        return false;
    }
    true
}

fn signed_width(token: &str) -> Option<i32> {
    match token {
        "int8" => Some(8),
        "int16" => Some(16),
        "int32" => Some(32),
        "int64" => Some(64),
        _ => None,
    }
}

pub(crate) fn numeric_typmod(precision: i32, scale: i32) -> i32 {
    ((precision << 16) | (scale & 0x7FF)) + 4
}

fn i256_digits(le: &[u8; 32]) -> (bool, String) {
    let negative = le[31] & 0x80 != 0;
    let mut bytes = *le;
    if negative {
        let mut carry = 1u16;
        for byte in bytes.iter_mut() {
            let wide = u16::from(!*byte) + carry;
            *byte = wide as u8;
            carry = wide >> 8;
        }
        if carry != 0 {
            return (true, "1".repeat(80));
        }
    }
    if bytes.iter().all(|byte| *byte == 0) {
        return (false, "0".to_string());
    }
    let mut digits = Vec::new();
    while bytes.iter().any(|byte| *byte != 0) {
        let mut remainder = 0u32;
        for byte in bytes.iter_mut().rev() {
            let current = (remainder << 8) | u32::from(*byte);
            *byte = (current / 10) as u8;
            remainder = current % 10;
        }
        digits.push(b'0' + remainder as u8);
    }
    digits.reverse();
    (negative, String::from_utf8(digits).unwrap_or_else(|_| "0".to_string()))
}

fn civil_timestamp(micros: i64) -> Option<(i32, u8, u8, u8, u8, u8, u32)> {
    let day_us = 86_400_000_000i64;
    let days = micros.div_euclid(day_us);
    let sod = micros.rem_euclid(day_us) as u64;
    let (year, month, day) = civil_from_unix_days(days)?;
    let secs = sod / 1_000_000;
    let hour = u8::try_from(secs / 3600).ok()?;
    let minute = u8::try_from((secs % 3600) / 60).ok()?;
    let second = u8::try_from(secs % 60).ok()?;
    let sub = u32::try_from(sod % 1_000_000).ok()?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((year, month, day, hour, minute, second, sub))
}

fn civil_from_unix_days(days: i64) -> Option<(i32, u8, u8)> {
    let z = days.checked_add(719_468)?;
    let era = if z >= 0 { z } else { z.checked_sub(146_096)? } / 146_097;
    let doe = u64::try_from(z.checked_sub(era.checked_mul(146_097)?)?).ok()?;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let year_of_era = i64::try_from(yoe).ok()?;
    let year = year_of_era.checked_add(era.checked_mul(400)?)?;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u8::try_from(doy - (153 * mp + 2) / 5 + 1).ok()?;
    let month = u8::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).ok()?;
    let year = if month <= 2 { year.checked_add(1)? } else { year };
    Some((i32::try_from(year).ok()?, month, day))
}

fn plan_inner(document: &serde_json::Value) -> Result<Plan, serde_json::Value> {
    let protocol = document.get("protocol").and_then(|value| value.as_str()).unwrap_or("");
    if protocol == SEMANTIC_CATALOG_PROTOCOL {
        return Err(result_of(
            "semantic_catalog",
            document,
            None,
            None,
            false,
            serde_json::json!([]),
        ));
    }
    if protocol == MYSQL_MANIFEST_PROTOCOL {
        return Err(result_of(
            "mysql_entrypoint",
            document,
            None,
            None,
            false,
            serde_json::json!([]),
        ));
    }
    if protocol != IMPORT_PROTOCOL {
        return Err(result_of("schema", document, None, None, false, serde_json::json!([])));
    }
    let engine = document.get("source_engine").and_then(|value| value.as_str()).unwrap_or("");
    if !recognized_engine(engine) {
        return Err(result_of(
            "source_engine_unknown",
            document,
            None,
            None,
            false,
            serde_json::json!([]),
        ));
    }
    let storage = document.get("target_storage").and_then(|value| value.as_str()).unwrap_or("");
    if storage != "postgresql" {
        return Err(result_of("target_storage", document, None, None, false, serde_json::json!([])));
    }
    let source_id = document.get("source_id").and_then(|value| value.as_str()).unwrap_or("");
    if !source_token(source_id) {
        return Err(result_of("schema", document, None, None, false, serde_json::json!([])));
    }
    let generation = document.get("source_generation").and_then(|value| value.as_i64()).unwrap_or(0);
    if generation < 1 {
        return Err(result_of("schema", document, None, None, false, serde_json::json!([])));
    }
    let snapshot = document.get("snapshot").cloned().unwrap_or(serde_json::Value::Null);
    let snapshot_kind = snapshot.get("kind").and_then(|value| value.as_str()).unwrap_or("");
    let snapshot_value = snapshot.get("value").and_then(|value| value.as_str()).unwrap_or("");
    if !matches!(snapshot_kind, "scn" | "generation" | "token")
        || snapshot_value.is_empty()
        || snapshot_value.len() > 128
        || snapshot_value.bytes().any(|byte| byte < 0x20)
    {
        return Err(result_of("shape", document, None, None, false, serde_json::json!([])));
    }
    let identity = document.get("catalog_identity").unwrap_or(&serde_json::Value::Null);
    let digest = |name: &str| {
        digest_field(identity, name).map_err(|reason| {
            result_of(reason, document, None, None, false, serde_json::json!([]))
        })
    };
    let config_digest = digest("config_digest")?;
    let schema_digest = digest("schema_digest")?;
    let type_digest = digest("type_digest")?;
    let input_digest = digest("input_digest")?;
    if client_authority(document) {
        return Err(result_of("nonadmitted", document, None, None, false, serde_json::json!([])));
    }
    let admission = match bind_registry(
        engine,
        source_id,
        generation,
        snapshot_kind,
        snapshot_value,
        &config_digest,
        &schema_digest,
        &type_digest,
        &input_digest,
    ) {
        Ok(Some(bound)) => bound,
        Ok(None) => match bind_admission(document, engine, source_id) {
            Ok(pin) => pin,
            Err(reason) => {
                return Err(result_of(reason, document, None, None, false, serde_json::json!([])));
            }
        },
        Err(reason) => {
            return Err(result_of(reason, document, None, None, false, serde_json::json!([])));
        }
    };
    let tables = document.get("tables").and_then(|value| value.as_array()).ok_or_else(|| {
        result_of("schema", document, Some(&admission), None, false, serde_json::json!([]))
    })?;
    if tables.is_empty() {
        return Err(result_of(
            "empty",
            document,
            Some(&admission),
            None,
            false,
            serde_json::json!([]),
        ));
    }
    let mut described = Vec::new();
    let mut verified = Vec::new();
    for (index, table) in tables.iter().enumerate() {
        let planned = plan_table(
            table,
            index,
            source_id,
            generation,
            &config_digest,
            &input_digest,
            &admission,
        )
        .map_err(|reason| {
            result_of(
                reason,
                document,
                Some(&admission),
                None,
                false,
                serde_json::Value::Array(described.clone()),
            )
        })?;
        described.push(planned.summary);
        verified.push(planned.table);
    }
    Ok(Plan {
        result: result_of(
            "target_absent",
            document,
            Some(&admission),
            None,
            false,
            serde_json::Value::Array(
                verified
                    .iter()
                    .map(|table| table.summary.clone())
                    .collect(),
            ),
        ),
        ready: Some(VerifiedImport {
            source_id: source_id.to_string(),
            source_generation: generation,
            config_digest,
            schema_digest,
            type_digest,
            input_digest,
            tables: verified.into_iter().map(|table| table.verified).collect(),
        }),
    })
}

struct TablePlan {
    summary: serde_json::Value,
    table: PlannedTable,
}

struct PlannedTable {
    summary: serde_json::Value,
    verified: VerifiedTable,
}

fn plan_table(
    table: &serde_json::Value,
    index: usize,
    source_id: &str,
    generation: i64,
    config_digest: &str,
    input_digest: &str,
    admission: &BoundAdmission,
) -> Result<TablePlan, &'static str> {
    let logical_schema = table.get("logical_schema").and_then(|value| value.as_str()).unwrap_or("");
    let logical_table = table.get("logical_table").and_then(|value| value.as_str()).unwrap_or("");
    let row_count = table.get("row_count").and_then(|value| value.as_u64()).unwrap_or(0);
    let batch_sha = table.get("batch_sha256").and_then(|value| value.as_str()).unwrap_or("");
    let arrow_path = table.get("arrow_path").and_then(|value| value.as_str()).unwrap_or("");
    if row_count == 0 {
        return Err("empty");
    }
    if !hex64(batch_sha) || arrow_path.is_empty() {
        return Err("schema");
    }
    if admission.scope == "recomputed_capture_binding" {
        let object = admission
            .objects
            .iter()
            .find(|item| item.logical_schema == logical_schema && item.logical_table == logical_table)
            .ok_or("schema")?;
        if object.row_count != row_count || object.arrow_sha256 != batch_sha {
            return Err("digest_mismatch");
        }
        let decls = column_decls(table)?;
        if decls.len() != admission.fields.len() {
            return Err("schema");
        }
        for (decl, field) in decls.iter().zip(&admission.fields) {
            if decl.name != field.name
                || decl.logical_name.is_empty()
                || decl.arrow_type != field.arrow_type
                || decl.nullable != field.nullable
                || decl.precision != field.precision
                || decl.scale != field.scale
            {
                return Err("schema");
            }
        }
    }
    let bytes = match read_capped(Path::new(arrow_path), MAX_ARROW) {
        Ok(bytes) => bytes,
        Err(ReadStop::Missing) => return Err("arrow_missing"),
        Err(ReadStop::TooLarge) => return Err("truncated"),
    };
    if bytes.is_empty() {
        return Err("empty");
    }
    let actual = hex_bytes(&Sha256::digest(&bytes));
    if actual != batch_sha {
        return Err("digest_mismatch");
    }
    if !admission.batches.iter().any(|item| item == batch_sha) {
        return Err("digest_mismatch");
    }
    if admission.source_bound {
        require_arrow_schema_digest(&bytes, &admission.arrow_schema_sha256)?;
    }
    if admission.scope == "recomputed_capture_binding" {
        let decoded = decode_arrow_stream(&bytes).map_err(|fault| fault.reason())?;
        if decoded.rows != usize::try_from(row_count).unwrap_or(usize::MAX) {
            return Err("shape");
        }
        if decoded.fields.len() != admission.fields.len() {
            return Err("schema");
        }
        for (got, expect) in decoded.fields.iter().zip(&admission.fields) {
            if got.name != expect.name || got.nullable != expect.nullable || !arrow_tokens_agree(got, &expect.arrow_type)
            {
                return Err("schema");
            }
            if !precision_scale_agree(got, expect.precision, expect.scale) {
                return Err("precision");
            }
            if got.timezone != expect.timezone {
                return Err("shape");
            }
        }
    }
    let decls = column_decls(table)?;
    let input = decode_hex32(input_digest).map_err(|_| "digest_mismatch")?;
    let prepared = prepare_table(
        &bytes,
        source_id,
        logical_schema,
        logical_table,
        u64::try_from(generation).unwrap_or(0),
        config_digest,
        input,
        &format!("t{index}.b0"),
        &decls,
        Some(&admission.fields),
    )
    .map_err(|fault| fault.reason())?;
    if prepared.batch.rows.len() as u64 != row_count {
        return Err("shape");
    }
    let summary = serde_json::json!({
        "logical_schema": logical_schema,
        "logical_table": logical_table,
        "physical_name": prepared.physical_name,
        "row_count": row_count,
        "batch_sha256": batch_sha,
        "batch_id": format!("t{index}.b0"),
        "copy_digest": prepared.copy_hex,
        "canonical_digest": prepared.canonical_digest,
        "columns": prepared.verified_columns.iter().map(|column| serde_json::json!({
            "logical_name": column.logical_name,
            "arrow_type": column.source_arrow,
            "oid": column.oid,
            "typmod": column.typmod,
            "provenance": column_provenance_for(column.oid, &column.source_arrow)
        })).collect::<Vec<_>>()
    });
    let verified = VerifiedTable {
        index,
        logical_schema: logical_schema.to_string(),
        logical_table: logical_table.to_string(),
        physical_name: prepared.physical_name,
        row_count,
        canonical_digest: prepared.canonical_digest,
        payload_digest: prepared.payload_digest,
        applied_pos: prepared.applied_pos,
        columns: prepared.verified_columns,
        batch: prepared.batch,
        copy_columns: prepared.copy_columns,
        positions: prepared.positions,
        };
    Ok(TablePlan {
        summary: summary.clone(),
        table: PlannedTable { summary, verified },
    })
}

fn column_decls(table: &serde_json::Value) -> Result<Vec<CopyColumnDecl>, &'static str> {
    let columns = table.get("columns").and_then(|value| value.as_array()).ok_or("schema")?;
    if columns.is_empty() {
        return Err("empty");
    }
    let mut out = Vec::with_capacity(columns.len());
    for column in columns {
        let name = column.get("name").and_then(|value| value.as_str()).ok_or("schema")?;
        let logical = column.get("logical_name").and_then(|value| value.as_str()).ok_or("schema")?;
        let arrow_type = column.get("arrow_type").and_then(|value| value.as_str()).ok_or("schema")?;
        let nullable = column.get("nullable").and_then(|value| value.as_bool()).ok_or("schema")?;
        let document = match column.get("document") {
            None => false,
            Some(value) => value.as_bool().ok_or("schema")?,
        };
        if document && (arrow_type != "utf8" || column.get("precision").is_some() || column.get("scale").is_some() || column.get("char_length").is_some()) {
            return Err("schema");
        }
        out.push(CopyColumnDecl {
            name: name.to_string(),
            logical_name: logical.to_string(),
            arrow_type: arrow_type.to_string(),
            nullable,
            precision: column.get("precision").and_then(|value| value.as_i64()).and_then(|value| i32::try_from(value).ok()),
            scale: column.get("scale").and_then(|value| value.as_i64()).and_then(|value| i32::try_from(value).ok()),
            char_length: column.get("char_length").and_then(|value| value.as_i64()).and_then(|value| i32::try_from(value).ok()),
            document,
        });
    }
    Ok(out)
}

struct BoundAdmission {
    decision_sha256: String,
    source_id: String,
    batches: Vec<String>,
    scope: &'static str,
    registry_sha256: Option<String>,
    driver_digest: Option<String>,
    objects: Vec<BoundObject>,
    fields: Vec<BoundField>,
    source_bound: bool,
    receipt_sha256: String,
    arrow_schema_sha256: String,
}

fn client_authority(document: &serde_json::Value) -> bool {
    const KEYS: &[&str] = &[
        "admitted",
        "admission_bin",
        "registry_path",
        "registry_sha256",
        "approval",
    ];
    let present = |value: &serde_json::Value| KEYS.iter().any(|key| value.get(*key).is_some());
    present(document) || document.get("admission").is_some_and(present)
}

fn bind_admission(
    document: &serde_json::Value,
    engine: &str,
    source_id: &str,
) -> Result<BoundAdmission, &'static str> {
    let admission = document.get("admission").ok_or("nonadmitted")?;
    let receipt_path = admission.get("receipt_path").and_then(|value| value.as_str()).ok_or("nonadmitted")?;
    let decision_path = admission.get("decision_path").and_then(|value| value.as_str()).ok_or("nonadmitted")?;
    let receipt = read_capped(Path::new(receipt_path), MAX_JSON).map_err(|_| "nonadmitted")?;
    if hex_bytes(&Sha256::digest(&receipt)) != RECEIPT_SHA256 {
        return Err("nonadmitted");
    }
    let receipt_json: serde_json::Value = serde_json::from_slice(&receipt).map_err(|_| "nonadmitted")?;
    if receipt_json.get("schema").and_then(|value| value.as_str()) != Some(ADMISSION_RECEIPT_SCHEMA)
        || receipt_json.get("content_commit").and_then(|value| value.as_str()) != Some(CONTENT_COMMIT)
    {
        return Err("nonadmitted");
    }
    let decision = read_capped(Path::new(decision_path), MAX_JSON).map_err(|_| "nonadmitted")?;
    let decision_sha = hex_bytes(&Sha256::digest(&decision));
    let pin = admitted_pins()
        .iter()
        .find(|pin| pin.decision_sha256 == decision_sha)
        .ok_or("nonadmitted")?;
    let decision_json: serde_json::Value = serde_json::from_slice(&decision).map_err(|_| "nonadmitted")?;
    if decision_json.get("protocol").and_then(|value| value.as_str()) != Some(ADMISSION_DECISION_PROTOCOL)
        || decision_json.get("admitted").and_then(|value| value.as_bool()) != Some(true)
        || decision_json.pointer("/identity/source_id").and_then(|value| value.as_str()) != Some(pin.source_id)
    {
        return Err("nonadmitted");
    }
    if pin.source_engine != engine || pin.source_id != source_id {
        return Err("nonadmitted");
    }
    Ok(BoundAdmission {
        decision_sha256: decision_sha,
        source_id: pin.source_id.to_string(),
        batches: pin.batches.iter().map(|item| (*item).to_string()).collect(),
        scope: "readonly_policy_replay",
        registry_sha256: None,
        driver_digest: None,
        objects: Vec::new(),
        fields: Vec::new(),
        source_bound: false,
        receipt_sha256: RECEIPT_SHA256.to_string(),
        arrow_schema_sha256: String::new(),
    })
}

#[allow(clippy::too_many_arguments)]
fn bind_registry(
    engine: &str,
    source_id: &str,
    generation: i64,
    snapshot_kind: &str,
    snapshot_value: &str,
    config_digest: &str,
    schema_digest: &str,
    type_digest: &str,
    input_digest: &str,
) -> Result<Option<BoundAdmission>, &'static str> {
    let Some(captured) = native_binding::resolve(
        engine,
        source_id,
        generation,
        snapshot_kind,
        snapshot_value,
        config_digest,
        schema_digest,
        type_digest,
        input_digest,
    )?
    else {
        return Ok(None);
    };
    Ok(Some(BoundAdmission {
        decision_sha256: captured.decision_sha256,
        source_id: captured.source_id,
        batches: captured.batches,
        scope: "recomputed_capture_binding",
        registry_sha256: Some(captured.registry_sha256),
        driver_digest: Some(captured.driver_digest),
        objects: captured.objects,
        fields: captured.fields,
        source_bound: captured.source_bound,
        receipt_sha256: captured.receipt_sha256,
        arrow_schema_sha256: captured.arrow_schema_sha256,
    }))
}

fn digest_field(identity: &serde_json::Value, name: &str) -> Result<String, &'static str> {
    let text = identity.get(name).and_then(|value| value.as_str()).unwrap_or("");
    if hex64(text) {
        Ok(text.to_string())
    } else {
        Err("digest_mismatch")
    }
}

fn recognized_engine(engine: &str) -> bool {
    matches!(
        engine,
        "mysql" | "postgresql" | "postgres" | "sqlserver" | "oracle"
            | "clickhouse"
            | "opensearch"
            | "doris"
            | "mongodb"
            | "mongo"
            | "starrocks"
            | "mariadb"
    )
}

fn decode_hex32(text: &str) -> Result<[u8; 32], ()> {
    if !hex64(text) {
        return Err(());
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(|_| ())?;
    }
    Ok(out)
}

fn result_of(
    reason: &str,
    document: &serde_json::Value,
    admission: Option<&BoundAdmission>,
    run_id: Option<&str>,
    loader_invoked: bool,
    tables: serde_json::Value,
) -> serde_json::Value {
    let state = if reason.is_empty() && run_id.is_some() {
        "awaiting_publish"
    } else {
        "not_sealed"
    };
    serde_json::json!({
        "protocol": RESULT_PROTOCOL,
        "load_state": state,
        "reason": if reason.is_empty() { serde_json::Value::Null } else { serde_json::json!(reason) },
        "loader_invoked": loader_invoked,
        "run_id": run_id,
        "source_engine": document.get("source_engine").cloned().unwrap_or(serde_json::Value::Null),
        "target_storage": document.get("target_storage").cloned().unwrap_or(serde_json::Value::Null),
        "source_id": document.get("source_id").cloned().unwrap_or(serde_json::Value::Null),
        "source_generation": document.get("source_generation").cloned().unwrap_or(serde_json::Value::Null),
        "snapshot": document.get("snapshot").cloned().unwrap_or(serde_json::Value::Null),
        "admission": {
            "content_commit": CONTENT_COMMIT,
            "seal_commit": SEAL_COMMIT,
            "receipt_sha256": echoed_receipt(reason, admission),
            "decision_sha256": admission.map(|item| item.decision_sha256.as_str()),
            "admitted_source_id": admission.map(|item| item.source_id.as_str()),
            "scope": admission.map(|item| item.scope).unwrap_or("readonly_policy_replay"),
            "batch_digests_bound": admission.map(|item| !item.batches.is_empty()).unwrap_or(false),
            "registry_sha256": admission.and_then(|item| item.registry_sha256.as_deref()),
            "driver_digest": admission.and_then(|item| item.driver_digest.as_deref())
        },
        "tables": tables
    })
}

/// Physical mapping stores a finished `pgident/v1` string. The catalog result
/// keeps the plan field name. A safe name is unchanged. Any other original,
/// including `whCode`, is `q_` plus the lowercase hex of its NFC bytes.
pub(crate) fn mapping_ident(original: &str) -> Result<String, ()> {
    let reserved = oo_connectors::PG17_RESERVED
        .iter()
        .copied()
        .map(str::to_string)
        .collect();
    oo_connectors::stable_ident(original, &reserved).map_err(|_| ())
}

fn source_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn hex64(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 64 && bytes.iter().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

enum ReadStop {
    Missing,
    TooLarge,
}

fn read_capped(path: &Path, max: usize) -> Result<Vec<u8>, ReadStop> {
    let meta = fs::metadata(path).map_err(|_| ReadStop::Missing)?;
    if !meta.is_file() {
        return Err(ReadStop::Missing);
    }
    if meta.len() > max as u64 {
        return Err(ReadStop::TooLarge);
    }
    fs::read(path).map_err(|_| ReadStop::Missing)
}

/// SHA-256 of one Arrow stream. This is the batch digest, not a catalog digest.
pub fn arrow_sha256(bytes: &[u8]) -> String {
    hex_bytes(&Sha256::digest(bytes))
}

/// SHA-256 of the Arrow schema message. This is not the stream digest.
pub fn arrow_schema_sha256(bytes: &[u8]) -> Result<String, &'static str> {
    let (frames, _) = frame_container(bytes).map_err(|fault| match fault {
        ArrowFault::Truncated => "truncated",
        _ => "schema",
    })?;
    for frame in &frames {
        if frame.eos || frame.meta.is_empty() {
            continue;
        }
        let cursor = Cursor { data: frame.meta };
        let root = root_table(&cursor).map_err(|_| "schema")?;
        if root.u8_field(1).map_err(|_| "schema")? == Some(1) {
            return Ok(arrow_sha256(frame.meta));
        }
    }
    Err("schema")
}

/// Source-bound plans must name that schema message. Historical plans do not call this.
/// The sealed catalog `schema_digest` stays the column preimage and is a different field.
pub fn require_arrow_schema_digest(bytes: &[u8], schema_digest: &str) -> Result<(), &'static str> {
    if arrow_schema_sha256(bytes)? == schema_digest {
        Ok(())
    } else {
        Err("digest_mismatch")
    }
}

fn echoed_receipt<'a>(reason: &str, admission: Option<&'a BoundAdmission>) -> &'a str {
    if (reason.is_empty() || reason == "target_absent")
        && let Some(bound) = admission
        && bound.source_bound
    {
        bound.receipt_sha256.as_str()
    } else {
        RECEIPT_SHA256
    }
}

fn file_digest(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
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

struct Framed<'a> {
    meta: &'a [u8],
    body: &'a [u8],
    eos: bool,
}

const ARROW_MAGIC: &[u8] = b"ARROW1";

fn frame_container(bytes: &[u8]) -> Result<(Vec<Framed<'_>>, bool), ArrowFault> {
    if bytes.starts_with(ARROW_MAGIC) {
        let interior = file_interior(bytes)?;
        Ok((frame_messages(interior, false)?, false))
    } else {
        Ok((frame_messages(bytes, true)?, true))
    }
}

fn file_interior(bytes: &[u8]) -> Result<&[u8], ArrowFault> {
    if bytes.len() < 16 || &bytes[bytes.len() - 6..] != ARROW_MAGIC {
        return Err(ArrowFault::Truncated);
    }
    let size_at = bytes.len() - 10;
    let footer_len = i32::from_le_bytes(bytes[size_at..size_at + 4].try_into().unwrap());
    if footer_len <= 0 {
        return Err(ArrowFault::Truncated);
    }
    let footer_len = usize::try_from(footer_len).map_err(|_| ArrowFault::Truncated)?;
    let footer_start = size_at.checked_sub(footer_len).ok_or(ArrowFault::Truncated)?;
    if footer_start < 8 {
        return Err(ArrowFault::Truncated);
    }
    let mut at = 6usize;
    if !at.is_multiple_of(8) {
        let aligned = align8(at);
        if aligned > footer_start || bytes[at..aligned].iter().any(|byte| *byte != 0) {
            return Err(ArrowFault::Truncated);
        }
        at = aligned;
    }
    while at + 8 <= footer_start {
        let word = i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        if word == -1 {
            break;
        }
        if bytes[at..at + 8].iter().any(|byte| *byte != 0) {
            return Err(ArrowFault::Truncated);
        }
        at += 8;
    }
    if at + 8 > footer_start {
        return Err(ArrowFault::Truncated);
    }
    Ok(&bytes[at..footer_start])
}

fn frame_messages(bytes: &[u8], require_eos: bool) -> Result<Vec<Framed<'_>>, ArrowFault> {
    let mut offset = 0usize;
    let mut out = Vec::new();
    while offset + 8 <= bytes.len() {
        let continuation = i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let meta_len = i32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
        if continuation != -1 || meta_len < 0 {
            return Err(ArrowFault::Truncated);
        }
        if meta_len == 0 {
            if offset + 8 != bytes.len() {
                return Err(ArrowFault::Truncated);
            }
            out.push(Framed {
                meta: &[],
                body: &[],
                eos: true,
            });
            return Ok(out);
        }
        let meta_end = offset + 8 + meta_len as usize;
        if meta_end > bytes.len() {
            return Err(ArrowFault::Truncated);
        }
        let meta = &bytes[offset + 8..meta_end];
        let cursor = Cursor { data: meta };
        let root = root_table(&cursor)?;
        let body_len = root.i64_field(3)?.unwrap_or(0);
        if body_len < 0 {
            return Err(ArrowFault::Truncated);
        }
        let body_len = usize::try_from(body_len).map_err(|_| ArrowFault::Truncated)?;
        let body_start = align8(meta_end);
        let body_end = body_start.checked_add(body_len).ok_or(ArrowFault::Truncated)?;
        if body_end > bytes.len() {
            return Err(ArrowFault::Truncated);
        }
        out.push(Framed {
            meta,
            body: &bytes[body_start..body_end],
            eos: false,
        });
        let mut next = body_end;
        if next % 8 != 0 {
            let aligned = align8(next);
            if aligned > bytes.len() || bytes[next..aligned].iter().any(|byte| *byte != 0) {
                return Err(ArrowFault::Truncated);
            }
            next = aligned;
        }
        offset = next;
    }
    if offset != bytes.len() || require_eos {
        return Err(ArrowFault::Truncated);
    }
    Ok(out)
}

fn align8(value: usize) -> usize {
    value.saturating_add(7) & !7
}

struct FieldSpec {
    name: String,
    nullable: bool,
    kind: FieldKind,
}

enum FieldKind {
    Int { bit_width: u8 },
    UInt64,
    UInt8,
    Decimal { precision: u8, scale: i8 },
    Decimal256 { precision: u8, scale: i8 },
    Timestamp { timezone: Option<String> },
    TimestampSeconds { timezone: Option<String> },
    TimestampNanos { timezone: String },
    TimestampMillis,
    TimestampMillisNaive,
    Date32,
    Date64,
    Time64 { nanos: bool },
    DurationMicros,
    Utf8,
    Binary,
    FixedBinary { width: i32 },
    Bool,
    Float64,
    DictionaryBinary { id: i64 },
    ListInt64,
    ListBinary { elements_nullable: bool },
    ListUtf8 { elements_nullable: bool },
    Struct { children: Vec<StructChild> },
    DenseUnion { children: Vec<UnionChild> },
}

struct StructChild {
    name: String,
    nullable: bool,
    kind: StructChildKind,
}

enum StructChildKind {
    Utf8,
    Int { bit_width: u8 },
}

struct UnionChild {
    type_id: i8,
    token: &'static str,
    nullable: bool,
    kind: UnionChildKind,
}

enum UnionChildKind {
    Utf8,
    Int64,
}

impl FieldSpec {
    fn decoded(self) -> DecodedField {
        let (arrow_type, precision, scale, timezone) = match &self.kind {
            FieldKind::Int { bit_width } => (format!("int{bit_width}"), Some(i32::from(*bit_width)), None, None),
            FieldKind::UInt64 => ("uint64".to_string(), Some(64), None, None),
            FieldKind::UInt8 => ("uint8".to_string(), None, None, None),
            FieldKind::TimestampMillis => ("timestamp(ms,utc)".to_string(), None, None, None),
            FieldKind::TimestampMillisNaive => ("timestamp_millis_naive".to_string(), None, None, None),
            FieldKind::Date32 => ("date32".to_string(), None, None, None),
            FieldKind::Date64 => ("date64".to_string(), None, None, None),
            FieldKind::Time64 { nanos } => (if *nanos { "time64_nanos" } else { "time64_micros" }.to_string(), None, None, None),
            FieldKind::DurationMicros => ("duration_micros".to_string(), None, None, None),
            FieldKind::TimestampSeconds { timezone } => {
                let name = if timezone.is_some() {
                    "timestamp_second_utc"
                } else {
                    "timestamp_second_naive"
                };
                (name.to_string(), None, None, timezone.clone())
            }
            FieldKind::FixedBinary { width } => (format!("fixedsizebinary({width})"), None, None, None),
            FieldKind::ListUtf8 { .. } => ("list(utf8)".to_string(), None, None, None),
            FieldKind::Struct { children } => (struct_token(children), None, None, None),
            FieldKind::DenseUnion { children } => (union_token(children), None, None, None),
            FieldKind::Decimal { precision, scale } => {
                ("decimal128".to_string(), Some(i32::from(*precision)), Some(i32::from(*scale)), None)
            }
            FieldKind::Decimal256 { precision, scale } => {
                ("decimal256".to_string(), Some(i32::from(*precision)), Some(i32::from(*scale)), None)
            }
            FieldKind::TimestampNanos { timezone } => {
                ("timestamp_nanos_utc".to_string(), None, None, Some(timezone.clone()))
            }
            FieldKind::DictionaryBinary { .. } => ("dictionary_binary".to_string(), None, None, None),
            FieldKind::ListInt64 => ("list_int64".to_string(), None, None, None),
            FieldKind::ListBinary { .. } => ("list_binary".to_string(), None, None, None),
            FieldKind::Timestamp { timezone } => {
                let name = if timezone.is_some() {
                    "timestamp_micros_utc"
                } else {
                    "timestamp_micros"
                };
                (name.to_string(), None, None, timezone.clone())
            }
            FieldKind::Utf8 => ("utf8".to_string(), None, None, None),
            FieldKind::Binary => ("binary".to_string(), None, None, None),
            FieldKind::Bool => ("bool".to_string(), None, None, None),
            FieldKind::Float64 => ("float64".to_string(), None, None, None),
        };
        DecodedField {
            name: self.name,
            nullable: self.nullable,
            arrow_type,
            precision,
            scale,
            timezone,
        }
    }
}

fn read_schema(root: &Table<'_>) -> Result<Vec<FieldSpec>, ArrowFault> {
    let header = root.table_field(2)?.ok_or(ArrowFault::Shape)?;
    if let Some(endian) = header.i16_field(0)?
        && endian != 0
    {
        return Err(ArrowFault::Shape);
    }
    if let Some((_, len)) = header.vector(3)?
        && len > 0
    {
        return Err(ArrowFault::Shape);
    }
    let (data, len) = header.vector(1)?.ok_or(ArrowFault::Shape)?;
    if len == 0 {
        return Err(ArrowFault::Empty);
    }
    let mut fields = Vec::with_capacity(len);
    for index in 0..len {
        let field = table_from_vector(header.cursor, data, index)?;
        let name = field.string(0)?.ok_or(ArrowFault::Shape)?;
        let nullable = field.bool_field(1, false)?;
        let type_id = field.u8_field(2)?.ok_or(ArrowFault::Shape)?;
        let child_count = field.vector(5)?.map(|(_, len)| len).unwrap_or(0);
        let kind = if let Some(encoding) = field.table_field(4)? {
            if type_id != 4 || child_count != 0 {
                return Err(ArrowFault::Shape);
            }
            let id = encoding.i64_field(0)?.unwrap_or(0);
            let index_type = encoding.table_field(1)?.ok_or(ArrowFault::Shape)?;
            let width = index_type.i32_field(0)?.ok_or(ArrowFault::Shape)?;
            let signed = index_type.bool_field(1, false)?;
            if width != 32 || !signed {
                return Err(ArrowFault::Shape);
            }
            FieldKind::DictionaryBinary { id }
        } else if type_id == 12 {
            if child_count != 1 {
                return Err(ArrowFault::Shape);
            }
            let (child_data, _) = field.vector(5)?.ok_or(ArrowFault::Shape)?;
            let child = table_from_vector(header.cursor, child_data, 0)?;
            if child.table_field(4)?.is_some() || child.vector(5)?.is_some_and(|(_, len)| len > 0) {
                return Err(ArrowFault::Shape);
            }
            let child_nullable = child.bool_field(1, false)?;
            let child_type = child.u8_field(2)?.ok_or(ArrowFault::Shape)?;
            let child_table = child.table_field(3)?.ok_or(ArrowFault::Shape)?;
            match child_type {
                2 => {
                    let width = child_table.i32_field(0)?.ok_or(ArrowFault::Shape)?;
                    let signed = child_table.bool_field(1, false)?;
                    if width != 64 || !signed || child_nullable {
                        return Err(ArrowFault::Shape);
                    }
                    FieldKind::ListInt64
                }
                4 => FieldKind::ListBinary {
                    elements_nullable: child_nullable,
                },
                5 => FieldKind::ListUtf8 {
                    elements_nullable: child_nullable,
                },
                _ => return Err(ArrowFault::Shape),
            }
        } else if type_id == 13 {
            read_struct_kind(&field, child_count)?
        } else if type_id == 14 {
            read_union_kind(&field, child_count)?
        } else {
            if child_count != 0 {
                return Err(ArrowFault::Shape);
            }
            let type_table = field.table_field(3)?.ok_or(ArrowFault::Shape)?;
            match type_id {
                2 => {
                    let width = type_table.i32_field(0)?.ok_or(ArrowFault::Shape)?;
                    let signed = type_table.bool_field(1, false)?;
                    if signed && matches!(width, 8 | 16 | 32 | 64) {
                        FieldKind::Int { bit_width: u8::try_from(width).unwrap_or(0) }
                    } else if !signed && width == 64 {
                        FieldKind::UInt64
                    } else if !signed && width == 8 {
                        FieldKind::UInt8
                    } else {
                        return Err(ArrowFault::Shape);
                    }
                }
                15 => {
                    let width = type_table.i32_field(0)?.ok_or(ArrowFault::Shape)?;
                    if !matches!(width, 12 | 16) {
                        return Err(ArrowFault::Shape);
                    }
                    FieldKind::FixedBinary { width }
                }
                7 => {
                    let precision = type_table.i32_field(0)?.ok_or(ArrowFault::Precision)?;
                    let scale = type_table.i32_field(1)?.unwrap_or(0);
                    let bits = type_table.i32_field(2)?.unwrap_or(128);
                    if bits == 128 && (1..=38).contains(&precision) && (-38..=38).contains(&scale) {
                        FieldKind::Decimal {
                            precision: u8::try_from(precision).map_err(|_| ArrowFault::Precision)?,
                            scale: i8::try_from(scale).map_err(|_| ArrowFault::Precision)?,
                        }
                    } else if bits == 256 && (1..=76).contains(&precision) && (-76..=76).contains(&scale) {
                        FieldKind::Decimal256 {
                            precision: u8::try_from(precision).map_err(|_| ArrowFault::Precision)?,
                            scale: i8::try_from(scale).map_err(|_| ArrowFault::Precision)?,
                        }
                    } else {
                        return Err(ArrowFault::Precision);
                    }
                }
                8 => {
                    // Arrow Date.unit defaults to DAY. An omitted slot is that default.
                    let unit = type_table.i16_field(0)?.unwrap_or(0);
                    if unit == 0 {
                        FieldKind::Date32
                    } else if unit == 1 {
                        FieldKind::Date64
                    } else {
                        return Err(ArrowFault::Shape);
                    }
                }
                9 => {
                    let unit = type_table.i16_field(0)?.unwrap_or(0);
                    let width = type_table.i32_field(1)?.unwrap_or(32);
                    if width != 64 || !matches!(unit, 2 | 3) {
                        return Err(ArrowFault::Shape);
                    }
                    FieldKind::Time64 { nanos: unit == 3 }
                }
                18 => {
                    if type_table.i16_field(0)?.unwrap_or(1) != 2 { return Err(ArrowFault::Shape); }
                    FieldKind::DurationMicros
                }
                10 => {
                    // Arrow Timestamp.unit defaults to SECOND.
                    let unit = type_table.i16_field(0)?.unwrap_or(0);
                    let timezone = type_table.string(1)?;
                    let timezone = match timezone {
                        Some(text) if !text.is_empty() => Some(text),
                        _ => None,
                    };
                    if unit == 0 && timezone.is_none() {
                        FieldKind::TimestampSeconds { timezone: None }
                    } else if unit == 0 && timezone.as_deref() == Some("UTC") {
                        FieldKind::TimestampSeconds { timezone }
                    } else if unit == 2 {
                        FieldKind::Timestamp { timezone }
                    } else if unit == 3 && timezone.as_deref() == Some("UTC") {
                        FieldKind::TimestampNanos {
                            timezone: timezone.unwrap_or_default(),
                        }
                    } else if unit == 1 && timezone.as_deref() == Some("UTC") {
                        FieldKind::TimestampMillis
                    } else if unit == 1 && timezone.is_none() {
                        FieldKind::TimestampMillisNaive
                    } else {
                        // A zone other than UTC, or nanoseconds that are not UTC,
                        // cannot be stored on the timestamp targets without loss.
                        return Err(ArrowFault::Precision);
                    }
                }
                5 => FieldKind::Utf8,
                4 => FieldKind::Binary,
                6 => FieldKind::Bool,
                3 => {
                    let precision = type_table.i16_field(0)?.ok_or(ArrowFault::Precision)?;
                    if precision != 2 {
                        return Err(ArrowFault::Precision);
                    }
                    FieldKind::Float64
                }
                _ => return Err(ArrowFault::Shape),
            }
        };
        fields.push(FieldSpec { name, nullable, kind });
    }
    Ok(fields)
}

fn struct_token(children: &[StructChild]) -> String {
    let parts: Vec<String> = children
        .iter()
        .map(|child| {
            let ty = match child.kind {
                StructChildKind::Utf8 => "utf8",
                StructChildKind::Int { bit_width: 8 } => "int8",
                StructChildKind::Int { bit_width: 16 } => "int16",
                StructChildKind::Int { bit_width: 32 } => "int32",
                StructChildKind::Int { bit_width: 64 } => "int64",
                StructChildKind::Int { .. } => "int",
            };
            format!("{}:{ty}", child.name)
        })
        .collect();
    format!("struct({})", parts.join(","))
}

fn union_token(children: &[UnionChild]) -> String {
    let mut parts: Vec<(i8, String)> = children
        .iter()
        .map(|child| (child.type_id, format!("{}:{}", child.type_id, child.token)))
        .collect();
    parts.sort_by_key(|item| item.0);
    let rendered = parts.into_iter().map(|(_, text)| text).collect::<Vec<_>>().join(",");
    format!("union(dense,{rendered})")
}

fn token_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 63 && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn child_table<'a>(field: &Table<'a>, index: usize) -> Result<Table<'a>, ArrowFault> {
    let (data, len) = field.vector(5)?.ok_or(ArrowFault::Shape)?;
    if index >= len {
        return Err(ArrowFault::Shape);
    }
    table_from_vector(field.cursor, data, index)
}

fn read_struct_kind(field: &Table<'_>, child_count: usize) -> Result<FieldKind, ArrowFault> {
    if child_count == 0 {
        return Err(ArrowFault::Shape);
    }
    let mut children = Vec::with_capacity(child_count);
    for index in 0..child_count {
        let child = child_table(field, index)?;
        if child.table_field(4)?.is_some() || child.vector(5)?.is_some_and(|(_, len)| len > 0) {
            return Err(ArrowFault::Shape);
        }
        let name = child.string(0)?.ok_or(ArrowFault::Shape)?;
        if !token_name(&name) {
            return Err(ArrowFault::Shape);
        }
        let nullable = child.bool_field(1, false)?;
        let type_id = child.u8_field(2)?.ok_or(ArrowFault::Shape)?;
        let type_table = child.table_field(3)?.ok_or(ArrowFault::Shape)?;
        let kind = match type_id {
            5 => StructChildKind::Utf8,
            2 => {
                let width = type_table.i32_field(0)?.ok_or(ArrowFault::Shape)?;
                let signed = type_table.bool_field(1, false)?;
                if !signed || !matches!(width, 8 | 16 | 32 | 64) {
                    return Err(ArrowFault::Shape);
                }
                StructChildKind::Int {
                    bit_width: u8::try_from(width).map_err(|_| ArrowFault::Shape)?,
                }
            }
            _ => return Err(ArrowFault::Shape),
        };
        children.push(StructChild { name, nullable, kind });
    }
    Ok(FieldKind::Struct { children })
}

fn read_union_kind(field: &Table<'_>, child_count: usize) -> Result<FieldKind, ArrowFault> {
    if child_count == 0 {
        return Err(ArrowFault::Shape);
    }
    let type_table = field.table_field(3)?.ok_or(ArrowFault::Shape)?;
    let mode = type_table.i16_field(0)?.unwrap_or(0);
    if mode != 1 {
        return Err(ArrowFault::Shape);
    }
    let ids = match type_table.vector(1)? {
        None => (0..i32::try_from(child_count).map_err(|_| ArrowFault::Shape)?).collect::<Vec<_>>(),
        Some((data, len)) => {
            if len != child_count {
                return Err(ArrowFault::Shape);
            }
            field.cursor.need(data, len.checked_mul(4).ok_or(ArrowFault::Shape)?)?;
            let mut out = Vec::with_capacity(len);
            for index in 0..len {
                out.push(field.cursor.i32(data + index * 4)?);
            }
            out
        }
    };
    if ids.len() != child_count {
        return Err(ArrowFault::Shape);
    }
    let mut seen = Vec::new();
    let mut children = Vec::with_capacity(child_count);
    for (index, type_id) in ids.iter().copied().enumerate() {
        if seen.contains(&type_id) {
            return Err(ArrowFault::Shape);
        }
        seen.push(type_id);
        let child = child_table(field, index)?;
        if child.table_field(4)?.is_some() || child.vector(5)?.is_some_and(|(_, len)| len > 0) {
            return Err(ArrowFault::Shape);
        }
        let type_code = child.u8_field(2)?.ok_or(ArrowFault::Shape)?;
        let nullable = child.bool_field(1, false)?;
        let (token, kind) = match type_code {
            5 => ("utf8", UnionChildKind::Utf8),
            2 => {
                let table = child.table_field(3)?.ok_or(ArrowFault::Shape)?;
                let width = table.i32_field(0)?.ok_or(ArrowFault::Shape)?;
                let signed = table.bool_field(1, false)?;
                if width != 64 || !signed {
                    return Err(ArrowFault::Shape);
                }
                ("int64", UnionChildKind::Int64)
            }
            _ => return Err(ArrowFault::Shape),
        };
        children.push(UnionChild {
            type_id: i8::try_from(type_id).map_err(|_| ArrowFault::Shape)?,
            token,
            nullable,
            kind,
        });
    }
    Ok(FieldKind::DenseUnion { children })
}

fn read_dictionary(
    root: &Table<'_>,
    body: &[u8],
    fields: &[FieldSpec],
    dicts: &mut BTreeMap<i64, Vec<NativeCell>>,
) -> Result<(), ArrowFault> {
    let header = root.table_field(2)?.ok_or(ArrowFault::Shape)?;
    let id = header.i64_field(0)?.unwrap_or(0);
    let is_delta = header.bool_field(2, false)?;
    let known = fields.iter().any(|field| matches!(field.kind, FieldKind::DictionaryBinary { id: got } if got == id));
    if !known {
        return Err(ArrowFault::Shape);
    }
    let data = header.table_field(1)?.ok_or(ArrowFault::Shape)?;
    let values = read_dictionary_values(&data, body)?;
    if is_delta {
        let slot = dicts.get_mut(&id).ok_or(ArrowFault::Shape)?;
        slot.extend(values);
    } else {
        dicts.insert(id, values);
    }
    Ok(())
}

fn read_dictionary_values(header: &Table<'_>, body: &[u8]) -> Result<Vec<NativeCell>, ArrowFault> {
    if header.table_field(3)?.is_some() {
        return Err(ArrowFault::Shape);
    }
    if let Some((_, len)) = header.vector(4)?
        && len > 0
    {
        return Err(ArrowFault::Shape);
    }
    let rows = header.i64_field(0)?.ok_or(ArrowFault::Shape)?;
    if rows < 0 {
        return Err(ArrowFault::Truncated);
    }
    let rows = usize::try_from(rows).map_err(|_| ArrowFault::Shape)?;
    let (nodes, node_count) = header.vector(1)?.ok_or(ArrowFault::Shape)?;
    let (buffers, buffer_count) = header.vector(2)?.ok_or(ArrowFault::Shape)?;
    if node_count != 1 {
        return Err(ArrowFault::Shape);
    }
    let length = header.cursor.i64(nodes)?;
    let null_count = header.cursor.i64(nodes + 8)?;
    if length != rows as i64 || null_count < -1 {
        return Err(ArrowFault::Shape);
    }
    let mut buffer_index = 0usize;
    let validity = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
    let valid = if rows == 0 {
        Vec::new()
    } else {
        validity_bits(body, &validity, rows, null_count)?
    };
    let column = read_bytes_like(header.cursor, buffers, &mut buffer_index, buffer_count, body, &valid, false)?;
    if buffer_index != buffer_count {
        return Err(ArrowFault::Shape);
    }
    Ok(column)
}

fn read_record_batch(
    root: &Table<'_>,
    body: &[u8],
    fields: &[FieldSpec],
    dicts: &BTreeMap<i64, Vec<NativeCell>>,
) -> Result<Vec<Vec<NativeCell>>, ArrowFault> {
    let header = root.table_field(2)?.ok_or(ArrowFault::Shape)?;
    if header.table_field(3)?.is_some() {
        return Err(ArrowFault::Shape);
    }
    if let Some((_, len)) = header.vector(4)?
        && len > 0
    {
        return Err(ArrowFault::Shape);
    }
    // FlatBuffers omits the default zero length of an empty record batch.
    let rows = header.i64_field(0)?.unwrap_or(0);
    if rows < 0 {
        return Err(ArrowFault::Truncated);
    }
    let rows = usize::try_from(rows).map_err(|_| ArrowFault::Shape)?;
    let (nodes, node_count) = header.vector(1)?.ok_or(ArrowFault::Shape)?;
    let (buffers, buffer_count) = header.vector(2)?.ok_or(ArrowFault::Shape)?;
    let mut node_index = 0usize;
    let mut buffer_index = 0usize;
    let mut columns = Vec::with_capacity(fields.len());
    for field in fields {
        let (length, null_count) = take_node(header.cursor, nodes, &mut node_index, node_count)?;
        if length != rows as i64 || null_count < -1 {
            return Err(ArrowFault::Shape);
        }
        let (column, valid) = if let FieldKind::DenseUnion { children } = &field.kind {
            if null_count > 0 {
                return Err(ArrowFault::Shape);
            }
            let column = read_dense_union(
                header.cursor,
                nodes,
                &mut node_index,
                node_count,
                buffers,
                &mut buffer_index,
                buffer_count,
                body,
                rows,
                children,
            )?;
            (column, vec![true; rows])
        } else {
            let validity = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
            let valid = validity_bits(body, &validity, rows, null_count)?;
            let column = match &field.kind {
            FieldKind::DenseUnion { .. } => return Err(ArrowFault::Shape),
            FieldKind::Int { bit_width } => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_ints(body, &values, &valid, *bit_width)?
            }
            FieldKind::Time64 { nanos } => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                let values = read_ints(body, &values, &valid, 64)?;
                let ceiling = if *nanos { 86_400_000_000_000 } else { 86_400_000_000 };
                if values.iter().any(|v| matches!(v, NativeCell::Int { value, .. } if *value < 0 || *value >= ceiling)) {
                    return Err(ArrowFault::Shape);
                }
                values
            }
            FieldKind::DurationMicros => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_ints(body, &values, &valid, 64)?
            }
            FieldKind::UInt8 => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_uint8(body, &values, &valid)?
            }
            FieldKind::UInt64 => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_uint64(body, &values, &valid)?
            }
            FieldKind::Decimal { precision, scale } => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_decimals(body, &values, &valid, *precision, *scale)?
            }
            FieldKind::Decimal256 { precision, scale } => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_decimal256(body, &values, &valid, *precision, *scale)?
            }
            FieldKind::Timestamp { timezone } => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_timestamps(body, &values, &valid, timezone.clone(), 1)?
            }
            FieldKind::TimestampSeconds { timezone } => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_timestamps(body, &values, &valid, timezone.clone(), 1_000_000)?
            }
            FieldKind::TimestampNanos { timezone } => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_nanos(body, &values, &valid, timezone)?
            }
            FieldKind::TimestampMillis => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_millis(body, &values, &valid, true)?
            }
            FieldKind::TimestampMillisNaive => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_millis(body, &values, &valid, false)?
            }
            FieldKind::Date32 => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_date32(body, &values, &valid)?
            }
            FieldKind::Date64 => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_date64(body, &values, &valid)?
            }
            FieldKind::Utf8 => read_bytes_like(header.cursor, buffers, &mut buffer_index, buffer_count, body, &valid, true)?,
            FieldKind::Binary => read_bytes_like(header.cursor, buffers, &mut buffer_index, buffer_count, body, &valid, false)?,
            FieldKind::FixedBinary { width } => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_fixed(body, &values, &valid, *width)?
            }
            FieldKind::Bool => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_bools(body, &values, &valid)?
            }
            FieldKind::Float64 => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_f64(body, &values, &valid)?
            }
            FieldKind::DictionaryBinary { id } => {
                let values = take_buffer(header.cursor, buffers, &mut buffer_index, buffer_count)?;
                read_dictionary_column(body, &values, &valid, *id, dicts)?
            }
            FieldKind::ListInt64 => read_list_i64(
                header.cursor,
                nodes,
                &mut node_index,
                node_count,
                buffers,
                &mut buffer_index,
                buffer_count,
                body,
                &valid,
            )?,
            FieldKind::ListBinary { elements_nullable } => read_list_binary(
                header.cursor,
                nodes,
                &mut node_index,
                node_count,
                buffers,
                &mut buffer_index,
                buffer_count,
                body,
                &valid,
                *elements_nullable,
            )?,
            FieldKind::ListUtf8 { elements_nullable } => read_list_utf8(
                header.cursor,
                nodes,
                &mut node_index,
                node_count,
                buffers,
                &mut buffer_index,
                buffer_count,
                body,
                &valid,
                *elements_nullable,
            )?,
            FieldKind::Struct { children } => read_struct(
                header.cursor,
                nodes,
                &mut node_index,
                node_count,
                buffers,
                &mut buffer_index,
                buffer_count,
                body,
                &valid,
                children,
            )?,
            };
            (column, valid)
        };
        if field.nullable {
            if valid.iter().any(|bit| !bit) && column.iter().all(|cell| !matches!(cell, NativeCell::Null)) {
                return Err(ArrowFault::Shape);
            }
        } else if valid.iter().any(|bit| !bit) || column.iter().any(|cell| matches!(cell, NativeCell::Null)) {
            return Err(ArrowFault::Shape);
        }
        columns.push(column);
    }
    if node_index != node_count || buffer_index != buffer_count {
        return Err(ArrowFault::Shape);
    }
    Ok(columns)
}

fn take_node(cursor: &Cursor<'_>, nodes: usize, index: &mut usize, count: usize) -> Result<(i64, i64), ArrowFault> {
    if *index >= count {
        return Err(ArrowFault::Shape);
    }
    let at = nodes + *index * 16;
    *index += 1;
    Ok((cursor.i64(at)?, cursor.i64(at + 8)?))
}

fn read_uint8(body: &[u8], buffer: &RawBuffer, valid: &[bool]) -> Result<Vec<NativeCell>, ArrowFault> {
    if buffer.length < valid.len() {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        out.push(NativeCell::UInt8(bytes[index]));
    }
    Ok(out)
}

fn read_fixed(body: &[u8], buffer: &RawBuffer, valid: &[bool], width: i32) -> Result<Vec<NativeCell>, ArrowFault> {
    let width = usize::try_from(width).map_err(|_| ArrowFault::Shape)?;
    let need = valid.len().checked_mul(width).ok_or(ArrowFault::Shape)?;
    if buffer.length < need {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let start = index * width;
        out.push(NativeCell::Binary(bytes[start..start + width].to_vec()));
    }
    Ok(out)
}

fn read_millis(
    body: &[u8],
    buffer: &RawBuffer,
    valid: &[bool],
    utc: bool,
) -> Result<Vec<NativeCell>, ArrowFault> {
    if buffer.length < valid.len().saturating_mul(8) {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let millis = i64::from_le_bytes(bytes[index * 8..index * 8 + 8].try_into().unwrap());
        let micros = millis.checked_mul(1_000).ok_or(ArrowFault::Precision)?;
        if !utc && civil_timestamp(micros).is_none() {
            return Err(ArrowFault::Precision);
        }
        out.push(if utc {
            NativeCell::TimestampMicrosUtc {
                value: micros,
                timezone: "UTC".to_string(),
            }
        } else {
            NativeCell::TimestampMicros(micros)
        });
    }
    Ok(out)
}

fn read_date32(body: &[u8], buffer: &RawBuffer, valid: &[bool]) -> Result<Vec<NativeCell>, ArrowFault> {
    if buffer.length < valid.len().saturating_mul(4) {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let days = i32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap());
        let (year, month, day) = civil_from_unix_days(i64::from(days)).ok_or(ArrowFault::Precision)?;
        oo_core::canonical::validate_date(year, month, day).map_err(|_| ArrowFault::Precision)?;
        out.push(NativeCell::Date32(days));
    }
    Ok(out)
}

fn read_date64(body: &[u8], buffer: &RawBuffer, valid: &[bool]) -> Result<Vec<NativeCell>, ArrowFault> {
    if buffer.length < valid.len().saturating_mul(8) {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let millis = i64::from_le_bytes(bytes[index * 8..index * 8 + 8].try_into().unwrap());
        let micros = millis.checked_mul(1_000).ok_or(ArrowFault::Precision)?;
        if civil_timestamp(micros).is_none() {
            return Err(ArrowFault::Precision);
        }
        out.push(NativeCell::TimestampMicros(micros));
    }
    Ok(out)
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{code:04x}", code = control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn json_text(value: &serde_json::Value) -> Result<String, ArrowFault> {
    serde_json::to_string(value).map_err(|_| ArrowFault::Shape)
}

#[allow(clippy::too_many_arguments)]
fn read_list_utf8(
    cursor: &Cursor<'_>,
    nodes: usize,
    node_index: &mut usize,
    node_count: usize,
    buffers: usize,
    buffer_index: &mut usize,
    buffer_count: usize,
    body: &[u8],
    parent_valid: &[bool],
    elements_nullable: bool,
) -> Result<Vec<NativeCell>, ArrowFault> {
    let offsets = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
    let starts = read_offsets(body, &offsets, parent_valid.len())?;
    let (child_len, child_nulls) = take_node(cursor, nodes, node_index, node_count)?;
    if child_len < 0 || child_nulls < -1 {
        return Err(ArrowFault::Shape);
    }
    let child_rows = usize::try_from(child_len).map_err(|_| ArrowFault::Shape)?;
    if usize::try_from(*starts.last().unwrap_or(&0)).unwrap_or(usize::MAX) != child_rows {
        return Err(ArrowFault::Shape);
    }
    let child_validity = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
    let child_valid = if child_rows == 0 {
        Vec::new()
    } else {
        validity_bits(body, &child_validity, child_rows, child_nulls)?
    };
    let flat = read_bytes_like(cursor, buffers, buffer_index, buffer_count, body, &child_valid, true)?;
    let mut out = Vec::with_capacity(parent_valid.len());
    for (row, is_valid) in parent_valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let from = usize::try_from(starts[row]).map_err(|_| ArrowFault::Shape)?;
        let to = usize::try_from(starts[row + 1]).map_err(|_| ArrowFault::Shape)?;
        let mut items = Vec::with_capacity(to - from);
        for cell in &flat[from..to] {
            match cell {
                NativeCell::Null if elements_nullable => items.push(serde_json::Value::Null),
                NativeCell::Utf8(text) => items.push(serde_json::Value::String(text.clone())),
                _ => return Err(ArrowFault::Shape),
            }
        }
        out.push(NativeCell::Json(json_text(&serde_json::Value::Array(items))?));
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn read_struct(
    cursor: &Cursor<'_>,
    nodes: usize,
    node_index: &mut usize,
    node_count: usize,
    buffers: usize,
    buffer_index: &mut usize,
    buffer_count: usize,
    body: &[u8],
    parent_valid: &[bool],
    children: &[StructChild],
) -> Result<Vec<NativeCell>, ArrowFault> {
    let mut columns = Vec::with_capacity(children.len());
    for child in children {
        let (child_len, child_nulls) = take_node(cursor, nodes, node_index, node_count)?;
        if child_len < 0 || child_nulls < -1 {
            return Err(ArrowFault::Shape);
        }
        let child_rows = usize::try_from(child_len).map_err(|_| ArrowFault::Shape)?;
        if child_rows != parent_valid.len() {
            return Err(ArrowFault::Shape);
        }
        let child_validity = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
        let child_valid = validity_bits(body, &child_validity, child_rows, child_nulls)?;
        for (row, bit) in child_valid.iter().enumerate() {
            if !bit && !child.nullable && parent_valid[row] {
                return Err(ArrowFault::Shape);
            }
        }
        let column = match child.kind {
            StructChildKind::Utf8 => read_bytes_like(cursor, buffers, buffer_index, buffer_count, body, &child_valid, true)?,
            StructChildKind::Int { bit_width } => {
                let values = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
                read_ints(body, &values, &child_valid, bit_width)?
            }
        };
        columns.push(column);
    }
    let mut out = Vec::with_capacity(parent_valid.len());
    for (row, is_valid) in parent_valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let mut parts = Vec::with_capacity(children.len());
        for (child, column) in children.iter().zip(&columns) {
            let value = match &column[row] {
                NativeCell::Null => "null".to_string(),
                NativeCell::Utf8(text) => json_string(text),
                NativeCell::Int { value, .. } => value.to_string(),
                _ => return Err(ArrowFault::Shape),
            };
            parts.push(format!("{}:{value}", json_string(&child.name)));
        }
        out.push(NativeCell::Json(format!("{{{}}}", parts.join(","))));
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn read_dense_union(
    cursor: &Cursor<'_>,
    nodes: usize,
    node_index: &mut usize,
    node_count: usize,
    buffers: usize,
    buffer_index: &mut usize,
    buffer_count: usize,
    body: &[u8],
    rows: usize,
    children: &[UnionChild],
) -> Result<Vec<NativeCell>, ArrowFault> {
    let type_buffer = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
    if type_buffer.length < rows {
        return Err(ArrowFault::Truncated);
    }
    let type_bytes = slice_body(body, &type_buffer)?;
    let offset_buffer = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
    if offset_buffer.length < rows.saturating_mul(4) {
        return Err(ArrowFault::Truncated);
    }
    let offset_bytes = slice_body(body, &offset_buffer)?;
    let mut child_columns = Vec::with_capacity(children.len());
    for child in children {
        let (child_len, child_nulls) = take_node(cursor, nodes, node_index, node_count)?;
        if child_len < 0 || child_nulls < -1 {
            return Err(ArrowFault::Shape);
        }
        let child_rows = usize::try_from(child_len).map_err(|_| ArrowFault::Shape)?;
        let child_validity = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
        let child_valid = if child_rows == 0 {
            Vec::new()
        } else {
            validity_bits(body, &child_validity, child_rows, child_nulls)?
        };
        if !child.nullable && child_valid.iter().any(|bit| !bit) {
            return Err(ArrowFault::Shape);
        }
        let column = match child.kind {
            UnionChildKind::Utf8 => read_bytes_like(cursor, buffers, buffer_index, buffer_count, body, &child_valid, true)?,
            UnionChildKind::Int64 => {
                let values = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
                read_ints(body, &values, &child_valid, 64)?
            }
        };
        child_columns.push(column);
    }
    let mut out = Vec::with_capacity(rows);
    for row in 0..rows {
        let type_id = type_bytes[row] as i8;
        let child_index = children
            .iter()
            .position(|child| child.type_id == type_id)
            .ok_or(ArrowFault::Shape)?;
        let offset = i32::from_le_bytes(offset_bytes[row * 4..row * 4 + 4].try_into().unwrap());
        if offset < 0 {
            return Err(ArrowFault::Shape);
        }
        let slot = usize::try_from(offset).map_err(|_| ArrowFault::Shape)?;
        let cell = child_columns[child_index].get(slot).ok_or(ArrowFault::Shape)?;
        let child = &children[child_index];
        let value = match cell {
            NativeCell::Null if child.nullable => "null".to_string(),
            NativeCell::Utf8(text) if matches!(child.kind, UnionChildKind::Utf8) => json_string(text),
            NativeCell::Int { bit_width: 64, value } if matches!(child.kind, UnionChildKind::Int64) => value.to_string(),
            _ => return Err(ArrowFault::Shape),
        };
        out.push(NativeCell::Json(format!(
            "{{\"type_id\":{type_id},\"type\":{},\"value\":{value}}}",
            json_string(child.token)
        )));
    }
    Ok(out)
}

fn read_uint64(body: &[u8], buffer: &RawBuffer, valid: &[bool]) -> Result<Vec<NativeCell>, ArrowFault> {
    if buffer.length < valid.len() * 8 {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let at = index * 8;
        let value = u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        out.push(NativeCell::UInt64(value));
    }
    Ok(out)
}

fn read_decimal256(
    body: &[u8],
    buffer: &RawBuffer,
    valid: &[bool],
    precision: u8,
    scale: i8,
) -> Result<Vec<NativeCell>, ArrowFault> {
    if buffer.length < valid.len() * 32 {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let at = index * 32;
        let mut coefficient = [0u8; 32];
        coefficient.copy_from_slice(&bytes[at..at + 32]);
        let (_negative, digits) = i256_digits(&coefficient);
        if digits.len() > usize::from(precision) {
            return Err(ArrowFault::Precision);
        }
        out.push(NativeCell::Decimal256 {
            precision,
            scale,
            coefficient,
        });
    }
    Ok(out)
}

fn read_nanos(
    body: &[u8],
    buffer: &RawBuffer,
    valid: &[bool],
    timezone: &str,
) -> Result<Vec<NativeCell>, ArrowFault> {
    if buffer.length < valid.len() * 8 {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let value = i64::from_le_bytes(bytes[index * 8..index * 8 + 8].try_into().unwrap());
        out.push(NativeCell::TimestampNanos {
            value,
            timezone: timezone.to_string(),
        });
    }
    Ok(out)
}

fn read_dictionary_column(
    body: &[u8],
    buffer: &RawBuffer,
    valid: &[bool],
    id: i64,
    dicts: &BTreeMap<i64, Vec<NativeCell>>,
) -> Result<Vec<NativeCell>, ArrowFault> {
    if buffer.length < valid.len() * 4 {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let dict = dicts.get(&id).ok_or(ArrowFault::Shape)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let slot = i32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap());
        if slot < 0 {
            return Err(ArrowFault::Shape);
        }
        let entry = dict.get(usize::try_from(slot).map_err(|_| ArrowFault::Shape)?).ok_or(ArrowFault::Shape)?;
        match entry {
            NativeCell::Binary(value) => out.push(NativeCell::Binary(value.clone())),
            NativeCell::Null => out.push(NativeCell::Null),
            _ => return Err(ArrowFault::Shape),
        }
    }
    Ok(out)
}

fn read_offsets(body: &[u8], buffer: &RawBuffer, rows: usize) -> Result<Vec<i32>, ArrowFault> {
    let need = rows.checked_add(1).ok_or(ArrowFault::Shape)? * 4;
    if buffer.length < need {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut previous = 0i32;
    let mut out = Vec::with_capacity(rows + 1);
    for index in 0..=rows {
        let next = i32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap());
        if index == 0 && next != 0 || next < previous {
            return Err(ArrowFault::Shape);
        }
        previous = next;
        out.push(next);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn read_list_i64(
    cursor: &Cursor<'_>,
    nodes: usize,
    node_index: &mut usize,
    node_count: usize,
    buffers: usize,
    buffer_index: &mut usize,
    buffer_count: usize,
    body: &[u8],
    parent_valid: &[bool],
) -> Result<Vec<NativeCell>, ArrowFault> {
    let offsets = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
    let starts = read_offsets(body, &offsets, parent_valid.len())?;
    let (child_len, child_nulls) = take_node(cursor, nodes, node_index, node_count)?;
    if child_len < 0 || child_nulls < -1 {
        return Err(ArrowFault::Shape);
    }
    let child_rows = usize::try_from(child_len).map_err(|_| ArrowFault::Shape)?;
    if usize::try_from(*starts.last().unwrap_or(&0)).unwrap_or(usize::MAX) != child_rows {
        return Err(ArrowFault::Shape);
    }
    let child_validity = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
    let child_valid = if child_rows == 0 {
        Vec::new()
    } else {
        validity_bits(body, &child_validity, child_rows, child_nulls)?
    };
    if child_valid.iter().any(|bit| !bit) {
        return Err(ArrowFault::Shape);
    }
    let values = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
    if values.length < child_rows * 8 {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, &values)?;
    let mut flat = Vec::with_capacity(child_rows);
    for index in 0..child_rows {
        let at = index * 8;
        flat.push(i64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()));
    }
    let mut out = Vec::with_capacity(parent_valid.len());
    for (row, is_valid) in parent_valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let from = usize::try_from(starts[row]).map_err(|_| ArrowFault::Shape)?;
        let to = usize::try_from(starts[row + 1]).map_err(|_| ArrowFault::Shape)?;
        let items: Vec<serde_json::Value> = flat[from..to].iter().copied().map(serde_json::Value::from).collect();
        let text = serde_json::to_string(&items).map_err(|_| ArrowFault::Shape)?;
        out.push(NativeCell::Json(text));
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn read_list_binary(
    cursor: &Cursor<'_>,
    nodes: usize,
    node_index: &mut usize,
    node_count: usize,
    buffers: usize,
    buffer_index: &mut usize,
    buffer_count: usize,
    body: &[u8],
    parent_valid: &[bool],
    elements_nullable: bool,
) -> Result<Vec<NativeCell>, ArrowFault> {
    let offsets = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
    let starts = read_offsets(body, &offsets, parent_valid.len())?;
    let (child_len, child_nulls) = take_node(cursor, nodes, node_index, node_count)?;
    if child_len < 0 || child_nulls < -1 {
        return Err(ArrowFault::Shape);
    }
    let child_rows = usize::try_from(child_len).map_err(|_| ArrowFault::Shape)?;
    if usize::try_from(*starts.last().unwrap_or(&0)).unwrap_or(usize::MAX) != child_rows {
        return Err(ArrowFault::Shape);
    }
    let child_validity = take_buffer(cursor, buffers, buffer_index, buffer_count)?;
    let child_valid = if child_rows == 0 {
        Vec::new()
    } else {
        validity_bits(body, &child_validity, child_rows, child_nulls)?
    };
    let flat = read_bytes_like(cursor, buffers, buffer_index, buffer_count, body, &child_valid, false)?;
    let mut out = Vec::with_capacity(parent_valid.len());
    for (row, is_valid) in parent_valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let from = usize::try_from(starts[row]).map_err(|_| ArrowFault::Shape)?;
        let to = usize::try_from(starts[row + 1]).map_err(|_| ArrowFault::Shape)?;
        let mut items = Vec::with_capacity(to - from);
        for cell in &flat[from..to] {
            match cell {
                NativeCell::Null if elements_nullable => items.push(serde_json::Value::Null),
                NativeCell::Binary(bytes) => {
                    let text = std::str::from_utf8(bytes).map_err(|_| ArrowFault::Shape)?;
                    items.push(serde_json::Value::String(text.to_string()));
                }
                _ => return Err(ArrowFault::Shape),
            }
        }
        let text = serde_json::to_string(&items).map_err(|_| ArrowFault::Shape)?;
        out.push(NativeCell::Json(text));
    }
    Ok(out)
}

struct RawBuffer {
    offset: usize,
    length: usize,
}

fn take_buffer(cursor: &Cursor<'_>, buffers: usize, index: &mut usize, count: usize) -> Result<RawBuffer, ArrowFault> {
    if *index >= count {
        return Err(ArrowFault::Shape);
    }
    let at = buffers + *index * 16;
    let offset = cursor.i64(at)?;
    let length = cursor.i64(at + 8)?;
    *index += 1;
    if offset < 0 || length < 0 {
        return Err(ArrowFault::Truncated);
    }
    Ok(RawBuffer {
        offset: usize::try_from(offset).map_err(|_| ArrowFault::Truncated)?,
        length: usize::try_from(length).map_err(|_| ArrowFault::Truncated)?,
    })
}

fn slice_body<'a>(body: &'a [u8], buffer: &RawBuffer) -> Result<&'a [u8], ArrowFault> {
    let end = buffer.offset.checked_add(buffer.length).ok_or(ArrowFault::Truncated)?;
    if end > body.len() {
        return Err(ArrowFault::Truncated);
    }
    Ok(&body[buffer.offset..end])
}

fn validity_bits(body: &[u8], buffer: &RawBuffer, rows: usize, null_count: i64) -> Result<Vec<bool>, ArrowFault> {
    let need = rows.div_ceil(8);
    if buffer.length == 0 {
        if null_count == 0 {
            return Ok(vec![true; rows]);
        }
        return Err(ArrowFault::Shape);
    }
    if buffer.length < need {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(rows);
    let mut nulls = 0i64;
    for index in 0..rows {
        let valid = bytes[index / 8] & (1 << (index % 8)) != 0;
        if !valid {
            nulls += 1;
        }
        out.push(valid);
    }
    if null_count >= 0 && nulls != null_count {
        return Err(ArrowFault::Shape);
    }
    Ok(out)
}

fn read_ints(body: &[u8], buffer: &RawBuffer, valid: &[bool], width: u8) -> Result<Vec<NativeCell>, ArrowFault> {
    let width = usize::from(width / 8);
    if buffer.length < valid.len() * width {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let at = index * width;
        let value = match width {
            1 => i64::from(bytes[at] as i8),
            2 => i64::from(i16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())),
            4 => i64::from(i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())),
            8 => i64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()),
            _ => return Err(ArrowFault::Shape),
        };
        out.push(NativeCell::Int {
            bit_width: u8::try_from(width * 8).unwrap_or(0),
            value,
        });
    }
    Ok(out)
}

fn read_decimals(
    body: &[u8],
    buffer: &RawBuffer,
    valid: &[bool],
    precision: u8,
    scale: i8,
) -> Result<Vec<NativeCell>, ArrowFault> {
    if buffer.length < valid.len() * 16 {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let at = index * 16;
        let coefficient = i128::from_le_bytes(bytes[at..at + 16].try_into().unwrap());
        let digits = coefficient.unsigned_abs().to_string();
        if digits.len() > usize::from(precision) {
            return Err(ArrowFault::Precision);
        }
        out.push(NativeCell::Decimal {
            precision,
            scale,
            coefficient,
        });
    }
    Ok(out)
}

fn read_timestamps(
    body: &[u8],
    buffer: &RawBuffer,
    valid: &[bool],
    timezone: Option<String>,
    micros_per_unit: i64,
) -> Result<Vec<NativeCell>, ArrowFault> {
    if buffer.length < valid.len() * 8 {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let raw = i64::from_le_bytes(bytes[index * 8..index * 8 + 8].try_into().unwrap());
        let value = raw.checked_mul(micros_per_unit).ok_or(ArrowFault::Precision)?;
        if civil_timestamp(value).is_none() {
            return Err(ArrowFault::Precision);
        }
        out.push(match &timezone {
            Some(zone) => NativeCell::TimestampMicrosUtc {
                value,
                timezone: zone.clone(),
            },
            None => NativeCell::TimestampMicros(value),
        });
    }
    Ok(out)
}

fn read_bytes_like(
    cursor: &Cursor<'_>,
    buffers: usize,
    index: &mut usize,
    count: usize,
    body: &[u8],
    valid: &[bool],
    utf8: bool,
) -> Result<Vec<NativeCell>, ArrowFault> {
    let offsets = take_buffer(cursor, buffers, index, count)?;
    let data = take_buffer(cursor, buffers, index, count)?;
    let need = valid.len().checked_add(1).ok_or(ArrowFault::Shape)? * 4;
    if offsets.length < need {
        return Err(ArrowFault::Truncated);
    }
    let offset_bytes = slice_body(body, &offsets)?;
    let data_bytes = slice_body(body, &data)?;
    let mut previous = i32::from_le_bytes(offset_bytes[0..4].try_into().unwrap());
    if previous < 0 {
        return Err(ArrowFault::Truncated);
    }
    let mut out = Vec::with_capacity(valid.len());
    for (row, is_valid) in valid.iter().enumerate() {
        let next = i32::from_le_bytes(offset_bytes[(row + 1) * 4..(row + 2) * 4].try_into().unwrap());
        if next < previous {
            return Err(ArrowFault::Truncated);
        }
        if !is_valid {
            out.push(NativeCell::Null);
            previous = next;
            continue;
        }
        let start = usize::try_from(previous).map_err(|_| ArrowFault::Truncated)?;
        let end = usize::try_from(next).map_err(|_| ArrowFault::Truncated)?;
        if end > data_bytes.len() {
            return Err(ArrowFault::Truncated);
        }
        let slice = data_bytes[start..end].to_vec();
        if utf8 {
            let text = String::from_utf8(slice).map_err(|_| ArrowFault::Shape)?;
            out.push(NativeCell::Utf8(text));
        } else {
            out.push(NativeCell::Binary(slice));
        }
        previous = next;
    }
    Ok(out)
}

fn read_bools(body: &[u8], buffer: &RawBuffer, valid: &[bool]) -> Result<Vec<NativeCell>, ArrowFault> {
    let need = valid.len().div_ceil(8);
    if buffer.length < need {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let bit = bytes[index / 8] & (1 << (index % 8)) != 0;
        out.push(NativeCell::Bool(bit));
    }
    Ok(out)
}

fn read_f64(body: &[u8], buffer: &RawBuffer, valid: &[bool]) -> Result<Vec<NativeCell>, ArrowFault> {
    if buffer.length < valid.len() * 8 {
        return Err(ArrowFault::Truncated);
    }
    let bytes = slice_body(body, buffer)?;
    let mut out = Vec::with_capacity(valid.len());
    for (index, is_valid) in valid.iter().enumerate() {
        if !is_valid {
            out.push(NativeCell::Null);
            continue;
        }
        let bits = u64::from_le_bytes(bytes[index * 8..index * 8 + 8].try_into().unwrap());
        out.push(NativeCell::Float64(bits));
    }
    Ok(out)
}

struct Cursor<'a> {
    data: &'a [u8],
}

impl Cursor<'_> {
    fn need(&self, at: usize, len: usize) -> Result<(), ArrowFault> {
        if at.checked_add(len).is_some_and(|end| end <= self.data.len()) {
            Ok(())
        } else {
            Err(ArrowFault::Truncated)
        }
    }

    fn u8(&self, at: usize) -> Result<u8, ArrowFault> {
        self.need(at, 1)?;
        Ok(self.data[at])
    }

    fn u16(&self, at: usize) -> Result<u16, ArrowFault> {
        self.need(at, 2)?;
        Ok(u16::from_le_bytes([self.data[at], self.data[at + 1]]))
    }

    fn i16(&self, at: usize) -> Result<i16, ArrowFault> {
        Ok(self.u16(at)? as i16)
    }

    fn u32(&self, at: usize) -> Result<u32, ArrowFault> {
        self.need(at, 4)?;
        Ok(u32::from_le_bytes(self.data[at..at + 4].try_into().unwrap()))
    }

    fn i32(&self, at: usize) -> Result<i32, ArrowFault> {
        Ok(self.u32(at)? as i32)
    }

    fn i64(&self, at: usize) -> Result<i64, ArrowFault> {
        self.need(at, 8)?;
        Ok(i64::from_le_bytes(self.data[at..at + 8].try_into().unwrap()))
    }
}

struct Table<'a> {
    cursor: &'a Cursor<'a>,
    start: usize,
    vtable: usize,
    vsize: usize,
}

impl<'a> Table<'a> {
    fn open(cursor: &'a Cursor<'a>, start: usize) -> Result<Self, ArrowFault> {
        // FlatBuffers stores a signed distance. The vtable is `start - soff`.
        let soff = cursor.i32(start)?;
        let back = isize::try_from(soff).map_err(|_| ArrowFault::Shape)?;
        let vtable = start
            .checked_add_signed(back.checked_neg().ok_or(ArrowFault::Shape)?)
            .ok_or(ArrowFault::Shape)?;
        let vsize = usize::from(cursor.u16(vtable)?);
        if vsize < 4 || vsize % 2 != 0 {
            return Err(ArrowFault::Shape);
        }
        cursor.need(vtable, vsize)?;
        Ok(Self {
            cursor,
            start,
            vtable,
            vsize,
        })
    }

    fn field(&self, index: usize) -> Result<Option<usize>, ArrowFault> {
        let slot = 4 + index * 2;
        if slot + 2 > self.vsize {
            return Ok(None);
        }
        let rel = usize::from(self.cursor.u16(self.vtable + slot)?);
        if rel == 0 {
            Ok(None)
        } else {
            Ok(Some(self.start + rel))
        }
    }

    fn u8_field(&self, index: usize) -> Result<Option<u8>, ArrowFault> {
        match self.field(index)? {
            None => Ok(None),
            Some(at) => Ok(Some(self.cursor.u8(at)?)),
        }
    }

    fn bool_field(&self, index: usize, default: bool) -> Result<bool, ArrowFault> {
        Ok(self.u8_field(index)?.map(|value| value != 0).unwrap_or(default))
    }

    fn i16_field(&self, index: usize) -> Result<Option<i16>, ArrowFault> {
        match self.field(index)? {
            None => Ok(None),
            Some(at) => Ok(Some(self.cursor.i16(at)?)),
        }
    }

    fn i32_field(&self, index: usize) -> Result<Option<i32>, ArrowFault> {
        match self.field(index)? {
            None => Ok(None),
            Some(at) => Ok(Some(self.cursor.i32(at)?)),
        }
    }

    fn i64_field(&self, index: usize) -> Result<Option<i64>, ArrowFault> {
        match self.field(index)? {
            None => Ok(None),
            Some(at) => Ok(Some(self.cursor.i64(at)?)),
        }
    }

    fn vector(&self, index: usize) -> Result<Option<(usize, usize)>, ArrowFault> {
        let Some(at) = self.field(index)? else {
            return Ok(None);
        };
        let rel = self.cursor.u32(at)? as usize;
        let pos = at.checked_add(rel).ok_or(ArrowFault::Truncated)?;
        let len = self.cursor.u32(pos)? as usize;
        Ok(Some((pos + 4, len)))
    }

    fn string(&self, index: usize) -> Result<Option<String>, ArrowFault> {
        let Some((data, len)) = self.vector(index)? else {
            return Ok(None);
        };
        self.cursor.need(data, len)?;
        let text = std::str::from_utf8(&self.cursor.data[data..data + len]).map_err(|_| ArrowFault::Shape)?;
        Ok(Some(text.to_string()))
    }

    fn table_field(&self, index: usize) -> Result<Option<Table<'a>>, ArrowFault> {
        let Some(at) = self.field(index)? else {
            return Ok(None);
        };
        let rel = self.cursor.u32(at)? as usize;
        let pos = at.checked_add(rel).ok_or(ArrowFault::Truncated)?;
        Ok(Some(Table::open(self.cursor, pos)?))
    }
}

fn root_table<'a>(cursor: &'a Cursor<'a>) -> Result<Table<'a>, ArrowFault> {
    let root = cursor.u32(0)? as usize;
    Table::open(cursor, root)
}

fn table_from_vector<'a>(cursor: &'a Cursor<'a>, data: usize, index: usize) -> Result<Table<'a>, ArrowFault> {
    let slot = data + index * 4;
    let rel = cursor.u32(slot)? as usize;
    Table::open(cursor, slot + rel)
}

trait ImportWriter {
    async fn register(&mut self, plan: &VerifiedImport, shard: &str, owner: &str) -> Result<(), LoadFault>;
    async fn adopt(&mut self, shard: &str, owner: &str) -> Result<i64, LoadFault>;
    async fn bind(&mut self, shard: &str, owner: &str, generation: i64) -> Result<(), LoadFault>;
    async fn open_run(
        &mut self,
        plan: &VerifiedImport,
        shard: &str,
        owner: &str,
        generation: i64,
    ) -> Result<String, LoadFault>;
    async fn prepare(
        &mut self,
        plan: &VerifiedImport,
        shard: &str,
        owner: &str,
        generation: i64,
        run_id: &str,
    ) -> Result<(), LoadFault>;
    async fn copy_and_write(
        &mut self,
        plan: &VerifiedImport,
        shard: &str,
        owner: &str,
        generation: i64,
        run_id: &str,
    ) -> Result<(), LoadFault>;
    async fn readback(&mut self, plan: &VerifiedImport) -> Result<(), LoadFault>;
    async fn note(
        &mut self,
        plan: &VerifiedImport,
        shard: &str,
        owner: &str,
        generation: i64,
        run_id: &str,
    ) -> Result<(), LoadFault>;
    async fn seal(
        &mut self,
        plan: &VerifiedImport,
        shard: &str,
        owner: &str,
        generation: i64,
        run_id: &str,
    ) -> Result<(), LoadFault>;
}

async fn apply<W: ImportWriter>(
    writer: &mut W,
    plan: &VerifiedImport,
    shard: &str,
    owner: &str,
) -> Result<String, LoadFault> {
    writer.register(plan, shard, owner).await?;
    let generation = writer.adopt(shard, owner).await?;
    writer.bind(shard, owner, generation).await?;
    let run_id = writer.open_run(plan, shard, owner, generation).await?;
    writer.prepare(plan, shard, owner, generation, &run_id).await?;
    writer.copy_and_write(plan, shard, owner, generation, &run_id).await?;
    writer.readback(plan).await?;
    writer.note(plan, shard, owner, generation, &run_id).await?;
    writer.seal(plan, shard, owner, generation, &run_id).await?;
    Ok(run_id)
}

struct PgWriter<'a, 'c> {
    tx: &'a mut sqlx::Transaction<'c, sqlx::Postgres>,
}

impl ImportWriter for PgWriter<'_, '_> {
    async fn register(&mut self, plan: &VerifiedImport, _shard: &str, _owner: &str) -> Result<(), LoadFault> {
        for table in &plan.tables {
            register_mapping(
                self.tx,
                &plan.source_id,
                &plan.config_digest,
                plan.source_generation,
                "namespace",
                &table.logical_schema,
                "",
                "",
            )
            .await?;
            register_mapping(
                self.tx,
                &plan.source_id,
                &plan.config_digest,
                plan.source_generation,
                "table",
                &table.logical_schema,
                &table.logical_table,
                "",
            )
            .await?;
            for column in &table.columns {
                register_mapping(
                    self.tx,
                    &plan.source_id,
                    &plan.config_digest,
                    plan.source_generation,
                    "column",
                    &table.logical_schema,
                    &table.logical_table,
                    &mapping_ident(&column.logical_name).map_err(|_| LoadFault::Refused("physical"))?,
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn adopt(&mut self, shard: &str, owner: &str) -> Result<i64, LoadFault> {
        sqlx::query_scalar("SELECT _oo.adopt_target_generation($1, $2::uuid)")
            .bind(shard)
            .bind(owner)
            .fetch_one(&mut **self.tx)
            .await
            .map_err(db_fault)
    }

    async fn bind(&mut self, shard: &str, owner: &str, generation: i64) -> Result<(), LoadFault> {
        sqlx::query("SELECT _oo.full_import_bind_session($1, $2::uuid, $3)")
            .bind(shard)
            .bind(owner)
            .bind(generation)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(db_fault)
    }

    async fn open_run(
        &mut self,
        plan: &VerifiedImport,
        shard: &str,
        owner: &str,
        generation: i64,
    ) -> Result<String, LoadFault> {
        let schemas: Vec<&str> = plan.tables.iter().map(|table| table.logical_schema.as_str()).collect();
        let names: Vec<&str> = plan.tables.iter().map(|table| table.logical_table.as_str()).collect();
        let physical: Vec<&str> = plan.tables.iter().map(|table| table.physical_name.as_str()).collect();
        sqlx::query_scalar(
            "SELECT _oo.full_import_open_run($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
        )
        .bind(shard)
        .bind(owner)
        .bind(generation)
        .bind(&plan.source_id)
        .bind(plan.source_generation)
        .bind(&plan.config_digest)
        .bind(&plan.type_digest)
        .bind(&plan.input_digest)
        .bind(&plan.schema_digest)
        .bind(&schemas)
        .bind(&names)
        .bind(&physical)
        .fetch_one(&mut **self.tx)
        .await
        .map_err(db_fault)
    }

    async fn prepare(
        &mut self,
        plan: &VerifiedImport,
        shard: &str,
        owner: &str,
        generation: i64,
        run_id: &str,
    ) -> Result<(), LoadFault> {
        for table in &plan.tables {
            let idents: Vec<String> = table
                .columns
                .iter()
                .map(|column| mapping_ident(&column.logical_name))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| LoadFault::Refused("physical"))?;
            let ident_refs: Vec<&str> = idents.iter().map(String::as_str).collect();
            let oids: Vec<i64> = table.columns.iter().map(|column| column.oid).collect();
            let typmods: Vec<i32> = table.columns.iter().map(|column| column.typmod).collect();
            let required: Vec<bool> = table.columns.iter().map(|column| column.not_null).collect();
            let physical: String = sqlx::query_scalar(
                "SELECT _oo.full_import_prepare_shadow($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            )
            .bind(shard)
            .bind(owner)
            .bind(generation)
            .bind(run_id)
            .bind(i32::try_from(table.index).unwrap_or(i32::MAX))
            .bind(&plan.source_id)
            .bind(&plan.config_digest)
            .bind(plan.source_generation)
            .bind(&table.logical_schema)
            .bind(&table.logical_table)
            .bind(&ident_refs)
            .bind(&oids)
            .bind(&typmods)
            .bind(&required)
            .fetch_one(&mut **self.tx)
            .await
            .map_err(db_fault)?;
            if physical != table.physical_name {
                return Err(LoadFault::Refused("shadow"));
            }
        }
        Ok(())
    }

    async fn copy_and_write(
        &mut self,
        plan: &VerifiedImport,
        shard: &str,
        owner: &str,
        generation: i64,
        run_id: &str,
    ) -> Result<(), LoadFault> {
        let budgets = CopyBudgets {
            max_value_bytes: 1_048_576,
            max_row_bytes: 1_048_576,
            max_chunk_bytes: 1_048_576,
            max_rows_per_chunk: 1024,
        };
        let hook = hook_digest();
        for table in &plan.tables {
            let quoted = quote_physical_name(&table.physical_name).map_err(|_| LoadFault::Refused("physical"))?;
            let mut list = String::from("\"_oo_pos\"");
            for column in &table.columns {
                list.push_str(", ");
                list.push_str(&quote_physical_name(&column.physical_name).map_err(|_| LoadFault::Refused("physical"))?);
            }
            let statement = format!("COPY _oo.{quoted} ({list}) FROM STDIN WITH (FORMAT binary)");
            let sent = copy_sink::send_batch(
                &mut *self.tx,
                &statement,
                &table.batch,
                &table.copy_columns,
                &table.positions,
                budgets,
            )
            .await
            .map_err(sink_fault)?;
            if sent.rows != table.row_count {
                return Err(LoadFault::Refused("rows"));
            }
            let wrote: String = sqlx::query_scalar(
                "SELECT _oo.full_import_write_batch($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15::numeric, $16, $17, $18::text[], $19::bigint[], $20::integer[], $21::bigint[], $22::text[])",
            )
            .bind(shard)
            .bind(owner)
            .bind(generation)
            .bind(run_id)
            .bind(i32::try_from(table.index).unwrap_or(i32::MAX))
            .bind(&plan.source_id)
            .bind(&table.physical_name)
            .bind(format!("t{}.b0", table.index))
            .bind(&table.payload_digest)
            .bind(&plan.config_digest)
            .bind(&plan.type_digest)
            .bind(&hook)
            .bind(&plan.schema_digest)
            .bind(&plan.input_digest)
            .bind(&table.applied_pos)
            .bind(i64::try_from(sent.rows).unwrap_or(i64::MAX))
            .bind(0_i64)
            .bind(Vec::<String>::new())
            .bind(Vec::<i64>::new())
            .bind(Vec::<i32>::new())
            .bind(Vec::<i64>::new())
            .bind(Vec::<String>::new())
            .fetch_one(&mut **self.tx)
            .await
            .map_err(db_fault)?;
            if wrote != "inserted" && wrote != "replay" {
                return Err(LoadFault::Refused("write"));
            }
        }
        Ok(())
    }

    async fn readback(&mut self, plan: &VerifiedImport) -> Result<(), LoadFault> {
        for table in &plan.tables {
            let quoted = quote_physical_name(&table.physical_name).map_err(|_| LoadFault::Refused("physical"))?;
            let sql = format!("SELECT count(*)::int8 FROM _oo.{quoted}");
            let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
                .fetch_one(&mut **self.tx)
                .await
                .map_err(db_fault)?;
            if u64::try_from(count).unwrap_or(0) != table.row_count {
                return Err(LoadFault::Refused("readback"));
            }
        }
        Ok(())
    }

    async fn note(
        &mut self,
        plan: &VerifiedImport,
        shard: &str,
        owner: &str,
        generation: i64,
        run_id: &str,
    ) -> Result<(), LoadFault> {
        for table in &plan.tables {
            sqlx::query("SELECT _oo.full_import_note_table($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8)")
                .bind(shard)
                .bind(owner)
                .bind(generation)
                .bind(run_id)
                .bind(i32::try_from(table.index).unwrap_or(i32::MAX))
                .bind(i64::try_from(table.row_count).unwrap_or(i64::MAX))
                .bind(&table.canonical_digest)
                .bind(&plan.source_id)
                .execute(&mut **self.tx)
                .await
                .map_err(db_fault)?;
        }
        Ok(())
    }

    async fn seal(
        &mut self,
        plan: &VerifiedImport,
        shard: &str,
        owner: &str,
        generation: i64,
        run_id: &str,
    ) -> Result<(), LoadFault> {
        // The shadow's deferred insert trigger requires state `open`. Fire it
        // before seal moves the run to `awaiting_publish`.
        sqlx::raw_sql(sqlx::AssertSqlSafe("SET CONSTRAINTS ALL IMMEDIATE"))
            .execute(&mut **self.tx)
            .await
            .map_err(db_fault)?;
        let schemas: Vec<&str> = plan.tables.iter().map(|table| table.logical_schema.as_str()).collect();
        let names: Vec<&str> = plan.tables.iter().map(|table| table.logical_table.as_str()).collect();
        let sealed: String = sqlx::query_scalar(
            "SELECT _oo.full_import_seal_run($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::text[])",
        )
        .bind(shard)
        .bind(owner)
        .bind(generation)
        .bind(run_id)
        .bind(&plan.source_id)
        .bind(plan.source_generation)
        .bind(&plan.config_digest)
        .bind(&plan.type_digest)
        .bind(&plan.input_digest)
        .bind(&plan.schema_digest)
        .bind(&schemas)
        .bind(&names)
        .fetch_one(&mut **self.tx)
        .await
        .map_err(db_fault)?;
        if sealed == "awaiting_publish" {
            Ok(())
        } else {
            Err(LoadFault::Refused("seal"))
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn register_mapping(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source: &str,
    config_digest: &str,
    generation: i64,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
) -> Result<(), LoadFault> {
    let mapped = plan_physical_mapping(&MappingRequest {
        source: source.to_string(),
        config_digest: config_digest.to_string(),
        run_generation: generation,
        level: match level {
            "namespace" => Level::Namespace,
            "table" => Level::Table,
            _ => Level::Column,
        },
        logical_namespace: namespace.to_string(),
        logical_table: table.to_string(),
        logical_column: column.to_string(),
    })
    .map_err(|_| LoadFault::Refused("physical"))?;
    sqlx::query("SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(source)
        .bind(config_digest)
        .bind(generation)
        .bind(level)
        .bind(namespace)
        .bind(table)
        .bind(column)
        .bind(mapped.identity_digest.as_slice())
        .execute(&mut **tx)
        .await
        .map_err(db_fault)?;
    let stored: String = sqlx::query_scalar("SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)")
        .bind(source)
        .bind(config_digest)
        .bind(generation)
        .bind(level)
        .bind(namespace)
        .bind(table)
        .bind(column)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_fault)?;
    if stored == mapped.physical_name {
        Ok(())
    } else {
        Err(LoadFault::Refused("physical"))
    }
}

fn db_fault(err: sqlx::Error) -> LoadFault {
    let sqlstate = err
        .as_database_error()
        .and_then(|item| item.code().map(|code| code.to_string()))
        .unwrap_or_default();
    LoadFault::Database(sqlstate)
}

fn sink_fault(err: SinkError) -> LoadFault {
    match err {
        SinkError::Encode(_) => LoadFault::Refused("encode"),
        SinkError::Driver(_) => LoadFault::Refused("copy"),
    }
}

/// Read one native import document from stdin and write the result to stdout.
pub async fn import_native() -> Result<(), u8> {
    let mut stdin = Vec::new();
    use std::io::Read;
    std::io::stdin().read_to_end(&mut stdin).map_err(|_| 2u8)?;
    if stdin.is_empty() || stdin.len() > MAX_JSON {
        eprintln!("oo-pipeline: usage");
        return Err(2);
    }
    let document: serde_json::Value = serde_json::from_slice(&stdin).map_err(|_| {
        eprintln!("oo-pipeline: usage");
        2u8
    })?;
    let mut plan = plan_import(&document);
    if plan.loader_ready()
        && let Some(target) = document.get("target")
    {
        match load_target(&mut plan, target).await {
            Ok(run_id) => {
                plan.result["load_state"] = serde_json::json!("awaiting_publish");
                plan.result["reason"] = serde_json::Value::Null;
                plan.result["loader_invoked"] = serde_json::json!(true);
                plan.result["run_id"] = serde_json::json!(run_id);
            }
            Err(LoadFault::NotReady) => {
                plan.result["reason"] = serde_json::json!("not_ready");
                plan.result["loader_invoked"] = serde_json::json!(false);
            }
            Err(_) => {
                plan.result["reason"] = serde_json::json!("load_failed");
                plan.result["loader_invoked"] = serde_json::json!(true);
                plan.result["run_id"] = serde_json::Value::Null;
                plan.result["load_state"] = serde_json::json!("not_sealed");
            }
        }
    }
    let mut bytes = serde_json::to_vec(&plan.result).map_err(|_| 1u8)?;
    bytes.push(b'\n');
    use std::io::Write;
    std::io::stdout().write_all(&bytes).map_err(|_| 1u8)?;
    if plan.result.get("load_state").and_then(|value| value.as_str()) == Some("awaiting_publish") {
        Ok(())
    } else {
        Err(1)
    }
}

async fn load_target(plan: &mut Plan, target: &serde_json::Value) -> Result<String, LoadFault> {
    let host = text_field(target, "host").ok_or(LoadFault::NotReady)?;
    let port = target.get("port").and_then(|value| value.as_u64()).and_then(|value| u16::try_from(value).ok()).ok_or(LoadFault::NotReady)?;
    let database = text_field(target, "database").ok_or(LoadFault::NotReady)?;
    let username = text_field(target, "runtime_principal").ok_or(LoadFault::NotReady)?;
    let password_file = text_field(target, "password_file").ok_or(LoadFault::NotReady)?;
    let ca_file = text_field(target, "ca_file").ok_or(LoadFault::NotReady)?;
    let shard = text_field(target, "shard").ok_or(LoadFault::NotReady)?;
    let owner = text_field(target, "owner").ok_or(LoadFault::NotReady)?;
    if !source_token(shard) {
        return Err(LoadFault::NotReady);
    }
    let password = fs::read_to_string(password_file).map_err(|_| LoadFault::NotReady)?;
    let ca = fs::read(ca_file).map_err(|_| LoadFault::NotReady)?;
    let options = sqlx::postgres::PgConnectOptions::from_explicit()
        .host(host)
        .port(port)
        .username(username)
        .password(password.trim())
        .database(database)
        .ssl_mode(sqlx::postgres::PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca);
    if options.get_options().is_some() {
        return Err(LoadFault::NotReady);
    }
    let options = options.disable_statement_logging();
    let mut conn = tokio::time::timeout(std::time::Duration::from_secs(30), options.connect())
        .await
        .map_err(|_| LoadFault::NotReady)?
        .map_err(|_| LoadFault::NotReady)?;
    load_verified(&mut conn, plan, shard, owner).await
}

fn text_field<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(|item| item.as_str()).filter(|item| !item.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Script {
        fail_at: &'static str,
        log: Vec<&'static str>,
    }

    impl Script {
        fn step(&mut self, name: &'static str) -> Result<(), LoadFault> {
            self.log.push(name);
            if self.fail_at == name {
                Err(LoadFault::Refused(name))
            } else {
                Ok(())
            }
        }
    }

    impl ImportWriter for Script {
        async fn register(&mut self, _: &VerifiedImport, _: &str, _: &str) -> Result<(), LoadFault> {
            self.step("register")
        }
        async fn adopt(&mut self, _: &str, _: &str) -> Result<i64, LoadFault> {
            self.step("adopt")?;
            Ok(1)
        }
        async fn bind(&mut self, _: &str, _: &str, _: i64) -> Result<(), LoadFault> {
            self.step("bind")
        }
        async fn open_run(&mut self, _: &VerifiedImport, _: &str, _: &str, _: i64) -> Result<String, LoadFault> {
            self.step("open_run")?;
            Ok("run".to_string())
        }
        async fn prepare(&mut self, _: &VerifiedImport, _: &str, _: &str, _: i64, _: &str) -> Result<(), LoadFault> {
            self.step("prepare")
        }
        async fn copy_and_write(&mut self, _: &VerifiedImport, _: &str, _: &str, _: i64, _: &str) -> Result<(), LoadFault> {
            self.step("copy_and_write")
        }
        async fn readback(&mut self, _: &VerifiedImport) -> Result<(), LoadFault> {
            self.step("readback")
        }
        async fn note(&mut self, _: &VerifiedImport, _: &str, _: &str, _: i64, _: &str) -> Result<(), LoadFault> {
            self.step("note")
        }
        async fn seal(&mut self, _: &VerifiedImport, _: &str, _: &str, _: i64, _: &str) -> Result<(), LoadFault> {
            self.step("seal")
        }
    }

    fn blank() -> VerifiedImport {
        VerifiedImport {
            source_id: "clickhouse_268".to_string(),
            source_generation: 1,
            config_digest: "a".repeat(64),
            schema_digest: "b".repeat(64),
            type_digest: "c".repeat(64),
            input_digest: "d".repeat(64),
            tables: Vec::new(),
        }
    }

    async fn trace(fail_at: &'static str) -> Vec<&'static str> {
        let mut script = Script {
            fail_at,
            log: vec!["begin"],
        };
        let plan = blank();
        if fail_at == "begin" {
            script.log.push("rollback");
            return script.log;
        }
        match apply(&mut script, &plan, "shard", "00000000-0000-0000-0000-000000000001").await {
            Ok(_) => script.log.push("commit"),
            Err(_) => script.log.push("rollback"),
        }
        script.log
    }

    #[tokio::test]
    async fn seal_failure_rolls_back_without_commit() {
        let phases = [
            "register",
            "adopt",
            "bind",
            "open_run",
            "prepare",
            "copy_and_write",
            "readback",
            "note",
            "seal",
        ];
        for phase in phases {
            let log = trace(phase).await;
            assert!(!log.contains(&"commit"), "{phase} {log:?}");
            assert_eq!(log.last().copied(), Some("rollback"), "{phase}");
            assert!(log.contains(&"begin"));
        }
        let done = trace("none").await;
        assert_eq!(done.last().copied(), Some("commit"));
        assert!(!done.contains(&"rollback"));
    }

    #[test]
    fn second_scale_refuses_overflow() {
        let raw = i64::MAX.to_le_bytes();
        let buffer = RawBuffer { offset: 0, length: 8 };
        let err = read_timestamps(&raw, &buffer, &[true], None, 1_000_000).unwrap_err();
        assert_eq!(err, ArrowFault::Precision);
    }

    #[test]
    fn date64_keeps_milliseconds_as_timestamp() {
        // 2026-05-01 00:00:00.001 UTC is not a whole day. It stays a timestamp.
        let millis: i64 = 20_574i64 * 86_400_000 + 1;
        let buffer = RawBuffer { offset: 0, length: 8 };
        let cells = read_date64(&millis.to_le_bytes(), &buffer, &[true]).unwrap();
        assert_eq!(cells, vec![NativeCell::TimestampMicros(millis * 1_000)]);
    }

    fn decl(arrow_type: &str, document: bool) -> CopyColumnDecl {
        CopyColumnDecl {
            name: "doc".to_string(),
            logical_name: "doc".to_string(),
            arrow_type: arrow_type.to_string(),
            nullable: true,
            precision: None,
            scale: None,
            char_length: None,
            document,
        }
    }

    fn utf8_field() -> DecodedField {
        DecodedField {
            name: "doc".to_string(),
            nullable: true,
            arrow_type: "utf8".to_string(),
            precision: None,
            scale: None,
            timezone: None,
        }
    }

    #[test]
    fn utf8_document_is_jsonb_and_plain_utf8_stays_varchar() {
        let target = target_of(&decl("utf8", true), &utf8_field()).expect("jsonb");
        assert_eq!((target.oid, target.typmod, target.type_name), (3802, -1, "jsonb"));
        assert_eq!(
            column_provenance_for(target.oid, "utf8"),
            "full document stored as jsonb oid 3802 typmod -1; missing keys stay absent; numbers stay exact; not json oid 114"
        );
        let plain = target_of(&decl("utf8", false), &utf8_field()).expect("varchar");
        assert_eq!((plain.oid, plain.typmod, plain.type_name), (1043, -1, "varchar"));
        assert!(target_of(&decl("binary", true), &utf8_field()).is_err());
        let text = r#"{"k": 0, "n": 1.50}"#;
        let cell = cell_of(&NativeCell::Utf8(text.to_string()), &target).expect("cell");
        match cell.value {
            Some(LogicalValue::Json(_)) => {}
            other => panic!("json node missing: {other:?}"),
        }
        assert_eq!(cell.arrow, Some(ArrowScalar::Utf8(text.to_string())));
        let refused = cell_of(&NativeCell::Utf8(r#"{"n":1e2}"#.to_string()), &target);
        assert!(refused.is_err());
    }
}

#[cfg(test)]
mod source_bound_schema {
    use super::*;

    fn fixture() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native/oracle-p2-scn2175138.arrow")
    }

    #[test]
    fn observed_pg_uuid_and_text_keep_original_storage() {
        let mut field = admission("batch", "schema", true).fields.remove(0);
        field.source_oid = Some(2950);
        field.source_typmod = Some(-1);
        field.source_format = Some("uuid".to_string());
        let mut uuid = target_type(TargetKind::FixedBytes { width: 16 }).unwrap();
        assert_eq!(uuid.oid, 17);
        preserve_pg_storage(&mut uuid, &field).unwrap();
        assert_eq!((uuid.oid, uuid.facts.typlen), (2950, 16));
        field.source_oid = Some(25);
        field.source_format = Some("text".to_string());
        let mut text = target_type(TargetKind::Text { char_length: None }).unwrap();
        preserve_pg_storage(&mut text, &field).unwrap();
        assert_eq!((text.oid, text.typmod), (25, -1));
        assert!(preserve_pg_storage(&mut uuid, &field).is_err());
    }

    #[test]
    fn admitted_bson_tags_select_objectid_text_uuid_and_binary() {
        use crate::native_binding::BsonType;
        let mut field = BoundField {
            name: "_id".to_string(), arrow_type: "fixedsizebinary(12)".to_string(),
            nullable: false, precision: None, scale: None, timezone: None,
            char_length: None, key: true, document: false, source_oid: None,
            source_typmod: None, source_format: None,
            bson_type: Some(BsonType { variant: "07".to_string(), subtype: None }),
        };
        let mut object_id = target_type(TargetKind::FixedBytes { width: 12 }).unwrap();
        preserve_bson_storage(&mut object_id, &field).unwrap();
        assert_eq!(object_id.oid, 25);
        let cell = cell_of(&NativeCell::Binary(vec![0xab; 12]), &object_id).unwrap();
        assert_eq!(cell.value, Some(LogicalValue::Text("ab".repeat(12))));
        field.arrow_type = "fixedsizebinary(16)".to_string();
        field.bson_type = Some(BsonType { variant: "05".to_string(), subtype: Some("04".to_string()) });
        let mut uuid = target_type(TargetKind::FixedBytes { width: 16 }).unwrap();
        preserve_bson_storage(&mut uuid, &field).unwrap();
        assert_eq!(uuid.oid, 2950);
        field.bson_type = Some(BsonType { variant: "05".to_string(), subtype: Some("00".to_string()) });
        let mut blob = target_type(TargetKind::FixedBytes { width: 16 }).unwrap();
        preserve_bson_storage(&mut blob, &field).unwrap();
        assert_eq!(blob.oid, 17);
    }

    fn admission(batch: &str, arrow_schema: &str, source_bound: bool) -> BoundAdmission {
        BoundAdmission {
            decision_sha256: "a".repeat(64),
            source_id: "oo04_t_batch".to_string(),
            batches: vec![batch.to_string()],
            scope: "recomputed_capture_binding",
            registry_sha256: None,
            driver_digest: None,
            objects: vec![BoundObject {
                logical_schema: "public".to_string(),
                logical_table: "codes".to_string(),
                row_count: 5,
                arrow_sha256: batch.to_string(),
            }],
            fields: vec![BoundField {
                name: "ID".to_string(),
                arrow_type: "int16".to_string(),
                nullable: false,
                precision: None,
                scale: None,
                timezone: None,
                char_length: None,
                key: true,
                document: false,
                source_oid: None,
                source_typmod: None,
                source_format: None,
                bson_type: None,
            }],
            source_bound,
            receipt_sha256: "b".repeat(64),
            arrow_schema_sha256: arrow_schema.to_string(),
        }
    }

    fn table(path: &std::path::Path, batch: &str) -> serde_json::Value {
        serde_json::json!({
            "logical_schema": "public",
            "logical_table": "codes",
            "row_count": 5,
            "batch_sha256": batch,
            "arrow_path": path,
            "columns": [{
                "name": "ID",
                "logical_name": "id",
                "arrow_type": "int16",
                "nullable": false
            }]
        })
    }

    #[test]
    fn source_bound_schema_digest_is_the_arrow_schema_message() {
        let path = fixture();
        let bytes = std::fs::read(&path).expect("oracle arrow");
        let batch = arrow_sha256(&bytes);
        assert_eq!(batch, ORACLE_P2_BATCH_SHA256);
        let schema = arrow_schema_sha256(&bytes).expect("schema message");
        assert_ne!(schema, batch);
        assert_ne!(schema, "ab".repeat(32));
        assert!(require_arrow_schema_digest(&bytes, &schema).is_ok());
        assert_eq!(require_arrow_schema_digest(&bytes, &batch), Err("digest_mismatch"));
        let document = table(&path, &batch);
        let wrong = "ab".repeat(32);
        assert_eq!(
            plan_table(&document, 0, "oo04_t_batch", 1, &wrong, &wrong, &admission(&batch, &wrong, true)).err(),
            Some("digest_mismatch")
        );
        assert_ne!(
            plan_table(&document, 0, "oo04_t_batch", 1, &wrong, &wrong, &admission(&batch, &schema, true)).err(),
            Some("digest_mismatch")
        );
        assert_ne!(
            plan_table(&document, 0, "oo04_t_batch", 1, &wrong, &wrong, &admission(&batch, &wrong, false)).err(),
            Some("digest_mismatch")
        );
    }
}
