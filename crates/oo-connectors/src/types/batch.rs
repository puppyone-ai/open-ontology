//! Canonical rows and Arrow-shaped columns from decoded cells.
//!
//! The canonical hash is the type-policy frame. It is not a hash of Arrow
//! buffers. SQL NULL and an unavailable cell are different presences.

use oo_core::canonical::{
    encode_pk, encode_value, hash_bytes, LogicalField, LogicalValue, Presence,
};
use oo_core::TYPE_POLICY_V1;

use super::binary::{field_value, logical_field};
use super::mapping::{ArrowPhysical, ColumnPlan, MapDecision};
use super::{digest_hex, RunBinding, TYPE_CONVERSION_PROTOCOL};
use crate::error::SourceError;

/// Why a cell has no value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellReject {
    pub reason: &'static str,
    pub value_len: u64,
}

/// SQL NULL is [`CellPresence::Null`]. A failed or omitted value is
/// [`CellPresence::Unavailable`] and is also an Arrow null.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellPresence {
    Null,
    Value,
    Unavailable,
}

/// One physical Arrow value. Decimal bytes are the unscaled coefficient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArrowScalar {
    Bool(bool),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    /// Raw IEEE-754 bits. Signed zero stays distinct here.
    Float64(u64),
    Utf8(String),
    Binary(Vec<u8>),
    Date32(i32),
    Time64(i64),
    TimestampNaive(i64),
    TimestampUtc(i64),
    Decimal128(i128),
    /// Little-endian two's complement.
    Decimal256([u8; 32]),
    List(Vec<ArrowScalar>),
    Null,
}

/// One decoded cell. `reject` is set only for [`CellPresence::Unavailable`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedCell {
    pub presence: CellPresence,
    pub value: Option<LogicalValue>,
    pub arrow: Option<ArrowScalar>,
    pub reject: Option<CellReject>,
    pub actual_precision: Option<i32>,
    pub actual_scale: Option<i32>,
    pub float_bits: Option<u64>,
}

impl DecodedCell {
    pub fn sql_null() -> Self {
        Self {
            presence: CellPresence::Null,
            value: None,
            arrow: Some(ArrowScalar::Null),
            reject: None,
            actual_precision: None,
            actual_scale: None,
            float_bits: None,
        }
    }
}

/// Arrow column parallel to one canonical field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArrowColumn {
    pub name: String,
    pub nullable: bool,
    pub arrow_type: String,
    pub physical: ArrowPhysical,
    pub metadata: Vec<(String, String)>,
    pub values: Vec<ArrowScalar>,
}

/// Masked reject. The value is not copied. `value_len` is the wire length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaskedReject {
    pub source_id: String,
    pub schema: String,
    pub table: String,
    pub column: String,
    pub attnum: i32,
    pub path: String,
    pub row_index: u64,
    pub key_digest: Option<String>,
    pub reason: &'static str,
    pub value_len: u64,
    pub run_generation: u64,
    pub input_digest: String,
    pub type_policy_version: &'static str,
    pub protocol: &'static str,
}

/// One preserved source row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedRow {
    pub cells: Vec<DecodedCell>,
    pub canonical_hash: [u8; 32],
    pub key_digest: Option<[u8; 32]>,
}

/// Typed read result. `plans` lists every source column. `fields` and `arrow`
/// list only the columns that have a lossless logical type, in the same order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedBatch {
    pub protocol: &'static str,
    pub type_policy_version: &'static str,
    pub source_id: String,
    pub schema: String,
    pub table: String,
    pub run_generation: u64,
    pub input_digest: [u8; 32],
    pub plans: Vec<ColumnPlan>,
    pub fields: Vec<LogicalField>,
    pub rows: Vec<TypedRow>,
    pub arrow: Vec<ArrowColumn>,
    pub rejects: Vec<MaskedReject>,
}

impl TypedBatch {
    /// Series 60 `RecordBatch` for the later reader and Loader.
    /// This does not hash Arrow buffers and does not change the canonical frame.
    /// `public-dependency` is not stable on Rust 1.94, so the arrow crates stay
    /// private dependencies and this return type is explicitly allowed.
    #[allow(exported_private_dependencies)]
    pub fn record_batch(&self) -> Result<arrow_array::RecordBatch, SourceError> {
        if self.arrow.is_empty() {
            let options =
                arrow_array::RecordBatchOptions::new().with_row_count(Some(self.rows.len()));
            return arrow_array::RecordBatch::try_new_with_options(
                std::sync::Arc::new(arrow_schema::Schema::empty()),
                Vec::new(),
                &options,
            )
            .map_err(|_| SourceError::protocol("arrow_batch_schema"));
        }
        super::arrow_batch::record_batch(&self.arrow)
    }
}

/// Build the batch. `rows` are parallel to the lossless plans. A key that is
/// null or unavailable fails the read. A null in a non-nullable column fails
/// the read. Other unavailable cells keep the row.
pub fn assemble_batch(
    binding: &RunBinding,
    plans: Vec<ColumnPlan>,
    key_attnums: &[i32],
    rows: Vec<Vec<DecodedCell>>,
) -> Result<TypedBatch, SourceError> {
    let framed: Vec<usize> = plans
        .iter()
        .enumerate()
        .filter(|(_, plan)| plan.mapping.decision == MapDecision::Lossless)
        .map(|(index, _)| index)
        .collect();
    let mut fields = Vec::with_capacity(framed.len());
    for index in &framed {
        fields.push(
            logical_field(&plans[*index]).ok_or_else(|| SourceError::protocol("type_plan"))?,
        );
    }
    let mut arrow = Vec::with_capacity(framed.len());
    for index in &framed {
        let plan = &plans[*index];
        let Some(physical) = plan.arrow.clone() else {
            return Err(SourceError::protocol("type_plan"));
        };
        arrow.push(ArrowColumn {
            name: plan.name.clone(),
            nullable: plan.mapping.nullable,
            arrow_type: plan.mapping.arrow_type.clone(),
            physical,
            metadata: plan.mapping.metadata.clone(),
            values: Vec::with_capacity(rows.len()),
        });
    }
    let mut out_rows = Vec::with_capacity(rows.len());
    let mut rejects = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        if row.len() != framed.len() {
            return Err(SourceError::protocol("row_width"));
        }
        for (slot, cell) in row.iter().enumerate() {
            let plan = &plans[framed[slot]];
            if cell.presence == CellPresence::Null && (!plan.mapping.nullable || plan.key) {
                return Err(SourceError::protocol("null_not_allowed"));
            }
            if plan.key && cell.presence != CellPresence::Value {
                return Err(SourceError::protocol("key_unavailable"));
            }
        }
        let key_digest = digest_key(&plans, &framed, key_attnums, row)?;
        let key_hex = key_digest.as_ref().map(digest_hex);
        let mut values = Vec::with_capacity(framed.len());
        for (slot, cell) in row.iter().enumerate() {
            let plan = &plans[framed[slot]];
            let field = field_value(cell).map_err(|_| SourceError::protocol("canonical_value"))?;
            if let Some(reject) = cell.reject {
                rejects.push(masked(binding, plan, row_index, key_hex.clone(), reject));
            }
            arrow[slot].values.push(arrow_of(cell));
            values.push(field);
        }
        let frame = encode_value(TYPE_POLICY_V1, &fields, &values)
            .map_err(|_| SourceError::protocol("canonical_value"))?;
        for plan in &plans {
            let MapDecision::Unsupported(reason) = plan.mapping.decision else {
                continue;
            };
            rejects.push(masked(
                binding,
                plan,
                row_index,
                key_hex.clone(),
                CellReject {
                    reason,
                    value_len: 0,
                },
            ));
        }
        out_rows.push(TypedRow {
            cells: row.clone(),
            canonical_hash: hash_bytes(&frame),
            key_digest,
        });
    }
    Ok(TypedBatch {
        protocol: TYPE_CONVERSION_PROTOCOL,
        type_policy_version: TYPE_POLICY_V1,
        source_id: binding.source_id.clone(),
        schema: binding.schema.clone(),
        table: binding.table.clone(),
        run_generation: binding.run_generation,
        input_digest: binding.input_digest,
        plans,
        fields,
        rows: out_rows,
        arrow,
        rejects,
    })
}

fn digest_key(
    plans: &[ColumnPlan],
    framed: &[usize],
    key_attnums: &[i32],
    row: &[DecodedCell],
) -> Result<Option<[u8; 32]>, SourceError> {
    if key_attnums.is_empty() {
        return Ok(None);
    }
    let mut fields = Vec::new();
    let mut values = Vec::new();
    for attnum in key_attnums {
        let Some(slot) = framed
            .iter()
            .position(|index| plans[*index].attnum == *attnum)
        else {
            return Err(SourceError::protocol("unsupported_key"));
        };
        let field = logical_field(&plans[framed[slot]])
            .ok_or_else(|| SourceError::protocol("type_plan"))?;
        let value = field_value(&row[slot]).map_err(|_| SourceError::protocol("canonical_value"))?;
        if value.presence != Presence::Value {
            return Err(SourceError::protocol("key_unavailable"));
        }
        fields.push(field);
        values.push(value);
    }
    let frame = encode_pk(TYPE_POLICY_V1, &fields, &values)
        .map_err(|_| SourceError::protocol("canonical_value"))?;
    Ok(Some(hash_bytes(&frame)))
}

fn arrow_of(cell: &DecodedCell) -> ArrowScalar {
    if cell.presence != CellPresence::Value {
        return ArrowScalar::Null;
    }
    cell.arrow.clone().unwrap_or(ArrowScalar::Null)
}

fn masked(
    binding: &RunBinding,
    plan: &ColumnPlan,
    row_index: usize,
    key_digest: Option<String>,
    reject: CellReject,
) -> MaskedReject {
    MaskedReject {
        source_id: binding.source_id.clone(),
        schema: binding.schema.clone(),
        table: binding.table.clone(),
        column: plan.name.clone(),
        attnum: plan.attnum,
        path: plan.name.clone(),
        row_index: row_index as u64,
        key_digest,
        reason: reject.reason,
        value_len: reject.value_len,
        run_generation: binding.run_generation,
        input_digest: digest_hex(&binding.input_digest),
        type_policy_version: TYPE_POLICY_V1,
        protocol: TYPE_CONVERSION_PROTOCOL,
    }
}

/// Bytes of the Arrow values only. This is not the canonical frame.
pub fn arrow_buffer_preimage(columns: &[ArrowColumn]) -> Vec<u8> {
    let mut out = Vec::new();
    for column in columns {
        out.extend(column.name.as_bytes());
        out.push(0);
        for value in &column.values {
            write_scalar(&mut out, value);
        }
    }
    out
}

fn write_scalar(out: &mut Vec<u8>, value: &ArrowScalar) {
    match value {
        ArrowScalar::Null => out.push(0),
        ArrowScalar::Bool(item) => {
            out.push(1);
            out.push(u8::from(*item));
        }
        ArrowScalar::Int16(item) => {
            out.push(2);
            out.extend(item.to_le_bytes());
        }
        ArrowScalar::Int32(item) => {
            out.push(3);
            out.extend(item.to_le_bytes());
        }
        ArrowScalar::Int64(item) => {
            out.push(4);
            out.extend(item.to_le_bytes());
        }
        ArrowScalar::Float64(item) => {
            out.push(5);
            out.extend(item.to_le_bytes());
        }
        ArrowScalar::Utf8(item) => {
            out.push(6);
            out.extend(item.as_bytes());
            out.push(0);
        }
        ArrowScalar::Binary(item) => {
            out.push(7);
            out.extend(item);
            out.push(0);
        }
        ArrowScalar::Date32(item) => {
            out.push(8);
            out.extend(item.to_le_bytes());
        }
        ArrowScalar::Time64(item) => {
            out.push(9);
            out.extend(item.to_le_bytes());
        }
        ArrowScalar::TimestampNaive(item) | ArrowScalar::TimestampUtc(item) => {
            out.push(10);
            out.extend(item.to_le_bytes());
        }
        ArrowScalar::Decimal128(item) => {
            out.push(11);
            out.extend(item.to_le_bytes());
        }
        ArrowScalar::Decimal256(item) => {
            out.push(12);
            out.extend(item);
        }
        ArrowScalar::List(items) => {
            out.push(13);
            for item in items {
                write_scalar(out, item);
            }
            out.push(0);
        }
    }
}
