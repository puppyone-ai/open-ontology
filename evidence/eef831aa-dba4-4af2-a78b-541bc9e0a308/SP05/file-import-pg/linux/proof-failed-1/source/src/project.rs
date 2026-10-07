//! Project one verified worker batch into a canonical `TypedBatch`.
//!
//! JSON oid 114 keeps logical type Text. Presence `missing` is not SQL NULL.
//! Presence `empty` on utf8 is an empty string. A quoted field stays a value.

use arrow_array::{Array, BooleanArray, Decimal128Array, Int64Array, StringArray};
use arrow_schema::DataType;
use oo_connectors::types::{
    ArrowPhysical, ArrowScalar, CellPresence, ColumnMapping, ColumnPlan, DecodedCell, MapDecision,
    NUMERIC_TYPMOD_VERSION,
};
use oo_connectors::{RunBinding, SourceError, TYPE_CONVERSION_PROTOCOL, VerifiedBatch};
use oo_core::canonical::LogicalType;
use oo_core::{parse_json_value, ExactDecimal, ExactInt, TYPE_POLICY_V1};
use oo_pipeline::copy_encoder::{assemble_batch, CopyBudgets, CopyColumn, LogicalValue, TypedBatch};
use oo_pipeline::copy_sink::{self, SinkError};

use crate::request::ColumnSpec;
use crate::{hex_encode, ImportError, REQUEST_PROTOCOL};

/// Fixed copy-encoder caps. They are not part of the config digest.
pub const COPY_BUDGETS: CopyBudgets = CopyBudgets {
    max_value_bytes: 1_048_576,
    max_row_bytes: 4_194_304,
    max_chunk_bytes: 8_388_608,
    max_rows_per_chunk: 1024,
};

/// Project one verified batch. A projector error has `child_started` false.
pub fn project_verified(
    columns: &[ColumnSpec],
    batch: &VerifiedBatch,
    source: &str,
    namespace: &str,
    table: &str,
    object_sha256: [u8; 32],
) -> Result<TypedBatch, ImportError> {
    if batch.record.num_columns() != columns.len() || batch.rows.iter().any(|row| row.fields.len() != columns.len()) {
        return Err(ImportError::new("malformed", "row_width"));
    }
    if batch.record.num_rows() != batch.rows.len() {
        return Err(ImportError::new("malformed", "row_count"));
    }
    let plans = column_plans(columns)?;
    let mut rows = Vec::with_capacity(batch.rows.len());
    for (index, verified) in batch.rows.iter().enumerate() {
        let mut cells = Vec::with_capacity(columns.len());
        for (slot, column) in columns.iter().enumerate() {
            let field = &verified.fields[slot];
            if field.name != column.name() {
                return Err(ImportError::new("malformed", "column"));
            }
            let arrow = batch.record.column(slot);
            cells.push(cell_of(column, arrow.as_ref(), index, field.presence)?);
        }
        rows.push(cells);
    }
    let binding = RunBinding {
        source_id: source.to_string(),
        schema: namespace.to_string(),
        table: table.to_string(),
        run_generation: 1,
        input_digest: object_sha256,
    };
    let keys: Vec<i32> = plans.iter().filter(|plan| plan.key).map(|plan| plan.attnum).collect();
    assemble_batch(&binding, plans, &keys, rows).map_err(assemble_error)
}

/// Empty typed batch used when the worker completed with zero rows.
pub(crate) fn empty_batch(
    columns: &[ColumnSpec],
    source: &str,
    namespace: &str,
    table: &str,
    object_sha256: [u8; 32],
) -> Result<TypedBatch, ImportError> {
    let plans = column_plans(columns)?;
    let binding = RunBinding {
        source_id: source.to_string(),
        schema: namespace.to_string(),
        table: table.to_string(),
        run_generation: 1,
        input_digest: object_sha256,
    };
    let keys: Vec<i32> = plans.iter().filter(|plan| plan.key).map(|plan| plan.attnum).collect();
    assemble_batch(&binding, plans, &keys, Vec::new()).map_err(assemble_error)
}

/// SHA-256 hex of the binary COPY header, row chunks, and trailer.
pub fn hash_typed_batch(batch: &TypedBatch) -> Result<String, ImportError> {
    let columns = copy_columns(batch);
    let positions = positions_for(0, batch.rows.len())?;
    let hashed = copy_sink::hash_batch(batch, &columns, &positions, COPY_BUDGETS).map_err(sink_error)?;
    Ok(hex_encode(hashed.hasher.finalize().as_slice()))
}

pub(crate) fn copy_columns(batch: &TypedBatch) -> Vec<CopyColumn> {
    let mut columns = Vec::with_capacity(batch.plans.len() + 1);
    columns.push(CopyColumn::SystemPosition);
    for plan in &batch.plans {
        columns.push(CopyColumn::User { attnum: plan.attnum });
    }
    columns
}

pub(crate) fn positions_for(start: u64, count: usize) -> Result<Vec<ExactDecimal>, ImportError> {
    let mut positions = Vec::with_capacity(count);
    for offset in 0..count {
        let index = start.saturating_add(offset as u64);
        positions.push(
            ExactDecimal::new(false, index.to_string(), 0).map_err(|_| ImportError::new("malformed", "position"))?,
        );
    }
    Ok(positions)
}

pub(crate) fn applied_pos(positions: &[ExactDecimal]) -> String {
    match positions.last() {
        Some(position) => position.to_plain_string(),
        None => "0".to_string(),
    }
}

pub(crate) fn sink_error(error: SinkError) -> ImportError {
    match error {
        SinkError::Encode(error) => ImportError::new("malformed", error.code),
        SinkError::Driver(detail) => ImportError::new("database", detail),
    }
}

fn assemble_error(error: SourceError) -> ImportError {
    ImportError::new("malformed", error.fact().unwrap_or("type_plan"))
}

fn column_plans(columns: &[ColumnSpec]) -> Result<Vec<ColumnPlan>, ImportError> {
    let mut plans = Vec::with_capacity(columns.len());
    for (index, column) in columns.iter().enumerate() {
        let attnum = i32::try_from(index + 1).map_err(|_| ImportError::new("malformed", "column"))?;
        let arrow = arrow_physical(column)?;
        let arrow_type = arrow.series60().map_err(|_| ImportError::new("malformed", "arrow_type"))?;
        let logical = logical_type(column);
        let numeric = column.pg_name() == "numeric";
        plans.push(ColumnPlan {
            mapping: ColumnMapping {
                protocol: TYPE_CONVERSION_PROTOCOL,
                source_type: column.parse_name().to_string(),
                arrow_type,
                pg_type: column.pg_name().to_string(),
                precision: column.precision().map(i32::from),
                scale: column.scale().map(i32::from),
                catalog_v1_numeric_scale: column.scale().map(i32::from),
                numeric_typmod_version: numeric.then_some(NUMERIC_TYPMOD_VERSION),
                timezone: None,
                nullable: column.nullable(),
                type_policy_version: TYPE_POLICY_V1,
                decision: MapDecision::Lossless,
                metadata: vec![("oo.conversion".to_string(), TYPE_CONVERSION_PROTOCOL.to_string())],
            },
            attnum,
            name: column.name().to_string(),
            oid: column.oid(),
            decode_oid: column.oid(),
            logical: Some(logical),
            arrow: Some(arrow),
            element: None,
            enum_labels: Vec::new(),
            key: column.primary_key(),
        });
    }
    Ok(plans)
}

fn logical_type(column: &ColumnSpec) -> LogicalType {
    match column.pg_name() {
        "bool" => LogicalType::Bool,
        "int8" => LogicalType::Int,
        "numeric" => LogicalType::Decimal {
            precision: u16::from(column.precision().unwrap_or(0)),
            scale: i16::from(column.scale().unwrap_or(0)),
        },
        _ => LogicalType::Text,
    }
}

fn arrow_physical(column: &ColumnSpec) -> Result<ArrowPhysical, ImportError> {
    Ok(match column.pg_name() {
        "bool" => ArrowPhysical::Boolean,
        "int8" => ArrowPhysical::Int64,
        "numeric" => ArrowPhysical::Decimal128 {
            precision: column.precision().ok_or(ImportError::new("closed_input", "precision"))?,
            scale: column.scale().ok_or(ImportError::new("closed_input", "scale"))?,
        },
        _ => ArrowPhysical::Utf8,
    })
}

fn cell_of(column: &ColumnSpec, arrow: &dyn Array, row: usize, presence: &str) -> Result<DecodedCell, ImportError> {
    expect_type(column, arrow.data_type())?;
    let is_null = arrow.is_null(row);
    match presence {
        "missing" => {
            if !is_null {
                return Err(ImportError::new("malformed", "presence"));
            }
            Err(ImportError::new("missing_field", "missing_field"))
        }
        "null" => {
            if !is_null {
                return Err(ImportError::new("malformed", "presence"));
            }
            Ok(DecodedCell::sql_null())
        }
        "empty" => empty_cell(column, arrow, row),
        "value" => {
            if is_null {
                return Err(ImportError::new("malformed", "presence"));
            }
            value_cell(column, arrow, row)
        }
        _ => Err(ImportError::new("malformed", "presence")),
    }
}

fn empty_cell(column: &ColumnSpec, arrow: &dyn Array, row: usize) -> Result<DecodedCell, ImportError> {
    if !column.is_utf8() || column.is_json() {
        return Err(ImportError::new("malformed", if column.is_json() { "json" } else { "presence" }));
    }
    let text = string_value(arrow, row)?;
    if !text.is_empty() {
        return Err(ImportError::new("malformed", "presence"));
    }
    text_cell(text)
}

fn value_cell(column: &ColumnSpec, arrow: &dyn Array, row: usize) -> Result<DecodedCell, ImportError> {
    if column.parse_name() == "int64" {
        let array = arrow
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or(ImportError::new("malformed", "arrow_type"))?;
        let number = array.value(row);
        let exact = ExactInt::parse(&number.to_string()).map_err(|_| ImportError::new("malformed", "type_plan"))?;
        return Ok(present(LogicalValue::Int(exact), ArrowScalar::Int64(number), None, None));
    }
    if column.parse_name() == "bool" {
        let array = arrow
            .as_any()
            .downcast_ref::<BooleanArray>()
            .ok_or(ImportError::new("malformed", "arrow_type"))?;
        let flag = array.value(row);
        return Ok(present(LogicalValue::Bool(flag), ArrowScalar::Bool(flag), None, None));
    }
    if column.parse_kind_decimal() {
        let array = arrow
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .ok_or(ImportError::new("malformed", "arrow_type"))?;
        let unscaled = array.value(row);
        let scale = i32::from(column.scale().ok_or(ImportError::new("closed_input", "scale"))?);
        let exact = decimal_from_unscaled(unscaled, scale)?;
        let storage = i32::try_from(exact.storage_precision()).map_err(|_| ImportError::new("malformed", "numeric"))?;
        return Ok(present(
            LogicalValue::Decimal(exact),
            ArrowScalar::Decimal128(unscaled),
            Some(storage),
            Some(scale),
        ));
    }
    let text = string_value(arrow, row)?;
    if text.as_bytes().contains(&0) {
        return Err(ImportError::new("malformed", "text_nul"));
    }
    if column.is_json() {
        parse_json_value(&text, REQUEST_PROTOCOL).map_err(|_| ImportError::new("malformed", "json"))?;
    }
    text_cell(text)
}

fn text_cell(text: String) -> Result<DecodedCell, ImportError> {
    if text.as_bytes().contains(&0) {
        return Err(ImportError::new("malformed", "text_nul"));
    }
    Ok(present(LogicalValue::Text(text.clone()), ArrowScalar::Utf8(text), None, None))
}

fn string_value(arrow: &dyn Array, row: usize) -> Result<String, ImportError> {
    let array = arrow
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or(ImportError::new("malformed", "arrow_type"))?;
    if array.is_null(row) {
        return Err(ImportError::new("malformed", "presence"));
    }
    Ok(array.value(row).to_string())
}

fn present(
    value: LogicalValue,
    arrow: ArrowScalar,
    actual_precision: Option<i32>,
    actual_scale: Option<i32>,
) -> DecodedCell {
    DecodedCell {
        presence: CellPresence::Value,
        value: Some(value),
        arrow: Some(arrow),
        reject: None,
        actual_precision,
        actual_scale,
        float_bits: None,
    }
}

fn decimal_from_unscaled(unscaled: i128, scale: i32) -> Result<ExactDecimal, ImportError> {
    let negative = unscaled < 0;
    let absolute = unscaled.checked_abs().ok_or(ImportError::new("malformed", "numeric"))?;
    ExactDecimal::new(negative, absolute.to_string(), scale).map_err(|_| ImportError::new("malformed", "numeric"))
}

fn expect_type(column: &ColumnSpec, data_type: &DataType) -> Result<(), ImportError> {
    let ok = match column.pg_name() {
        "int8" => matches!(data_type, DataType::Int64),
        "bool" => matches!(data_type, DataType::Boolean),
        "numeric" => matches!(
            data_type,
            DataType::Decimal128(precision, scale)
                if *precision == column.precision().unwrap_or(0) && *scale == column.scale().unwrap_or(0)
        ),
        "text" | "json" => matches!(data_type, DataType::Utf8),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(ImportError::new("malformed", "arrow_type"))
    }
}

