//! Bounded PostgreSQL binary COPY rows.
//!
//! The caller pulls a header, row chunks, and a trailer. Nothing here opens a
//! connection, writes a target, or admits the reader. See `copy-encoder/v1`.
//! `json` oid 114 is the cell text. `jsonb` oid 3802 is one version byte `1`
//! plus that same text. The text is not rendered again.

use std::fmt;

use oo_core::canonical::{ARBITRARY_DECIMAL_SCALE, LogicalType, classify_f64, validate_date};
pub use oo_core::canonical::{FloatClass, LogicalValue, parse_json};
pub use oo_core::{ExactDecimal, ExactInt};

pub use oo_connectors::types::{
    ArrowPhysical, ArrowScalar, CellPresence, CellReject, ColumnPlan, DecodedCell, MapDecision,
    RunBinding, TypeFacts, TypeGraph, TypedBatch, assemble_batch, map_column,
};
pub use oo_connectors::{
    ColumnPrivileges, SourceColumn, TypeSupport, TypeUnsupportedReason, numeric_parts,
};

/// Protocol name for this encoder.
pub const PROTOCOL: &str = "copy-encoder/v1";

/// Accept one BSON document envelope and return the PostgreSQL jsonb text.
///
/// The text is the caller's `pg_jsonb_text` after [`oo_core::jsonb_doc::seal_bson_jsonb_document`]
/// accepts the raw BSON, the version-byte file, and the exact number lexemes.
/// This does not render the value through a float.
pub fn accept_jsonb_document(envelope: &str) -> Result<String, CopyError> {
    oo_core::jsonb_doc::seal_bson_jsonb_document(envelope)
        .map_err(|_| CopyError::new("type_not_encoded", "jsonb"))?;
    let value: serde_json::Value =
        serde_json::from_str(envelope).map_err(|_| CopyError::new("type_not_encoded", "jsonb"))?;
    let text = value
        .get("pg_jsonb_text")
        .and_then(|item| item.as_str())
        .ok_or_else(|| CopyError::new("type_not_encoded", "jsonb"))?;
    if text.is_empty() || text.as_bytes().contains(&0) {
        return Err(CopyError::new("type_not_encoded", "jsonb"));
    }
    Ok(text.to_string())
}

/// Binary COPY signature, flags, and header-extension length.
pub const HEADER_LEN: usize = 19;

const HEADER: [u8; HEADER_LEN] = *b"PGCOPY\n\xff\r\n\0\0\0\0\0\0\0\0\0";
const TRAILER: [u8; 2] = [0xff, 0xff];
const PG_UNIX_DAYS: i64 = 10_957;
const PG_UNIX_MICROS: i128 = 946_684_800_000_000;
const DAY_MICROS: i128 = 86_400_000_000;

const OID_BOOL: i64 = 16;
const OID_BYTEA: i64 = 17;
const OID_INT8: i64 = 20;
const OID_INT2: i64 = 21;
const OID_INT4: i64 = 23;
const OID_TEXT: i64 = 25;
const OID_JSON: i64 = 114;
const OID_FLOAT4: i64 = 700;
const OID_FLOAT8: i64 = 701;
const OID_BPCHAR: i64 = 1042;
const OID_VARCHAR: i64 = 1043;
const OID_DATE: i64 = 1082;
const OID_TIME: i64 = 1083;
const OID_TIMESTAMP: i64 = 1114;
const OID_TIMESTAMPTZ: i64 = 1184;
const OID_INTERVAL: i64 = 1186;
const OID_NUMERIC: i64 = 1700;
const OID_UUID: i64 = 2950;
const OID_JSONB: i64 = 3802;

/// Caps checked before the chunk buffer is allocated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyBudgets {
    /// Largest payload of one value, excluding its length word.
    pub max_value_bytes: u32,
    /// Largest encoded tuple, including the field count and length words.
    pub max_row_bytes: u32,
    /// Largest header, row chunk, or trailer returned by one pull.
    pub max_chunk_bytes: u32,
    /// Most tuples in one row chunk.
    pub max_rows_per_chunk: u32,
}

/// COPY column. Index 0 is the system position, not a batch field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyColumn {
    /// Caller-supplied `numeric(38,0)` position. One per row.
    SystemPosition,
    /// Lossless source column, in this list's order.
    User { attnum: i32 },
}

/// Unavailable and masked cells are refused. They are not written as NULL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnavailablePolicy {
    /// Fail the pull. The detail is the reject reason.
    Refuse,
}

/// Typed encode failure. `detail` is a static token, never a cell value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyError {
    /// Stable code.
    pub code: &'static str,
    /// Static qualifier. Empty when the code stands alone.
    pub detail: &'static str,
    /// Source row when the failure is inside a tuple.
    pub row: Option<u32>,
    /// User `attnum` when the failure is one column. Absent for the system position.
    pub attnum: Option<i32>,
}

impl CopyError {
    fn new(code: &'static str, detail: &'static str) -> Self {
        Self {
            code,
            detail,
            row: None,
            attnum: None,
        }
    }

    fn at(mut self, row: u32, attnum: Option<i32>) -> Self {
        self.row = Some(row);
        self.attnum = attnum;
        self
    }
}

impl fmt::Display for CopyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code;
        if self.detail.is_empty() {
            write!(formatter, "copy encoder {code}")
        } else {
            let detail = self.detail;
            write!(formatter, "copy encoder {code} {detail}")
        }
    }
}

impl std::error::Error for CopyError {}

/// One pulled piece. The `Vec` is at most `max_chunk_bytes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyPiece {
    /// 19-byte binary header.
    Header { bytes: Vec<u8> },
    /// One or more tuples.
    Rows { bytes: Vec<u8>, row_count: u32 },
    /// `int16` `-1` file trailer.
    Trailer { bytes: Vec<u8> },
    /// No further bytes.
    Done,
}

/// Series-60 row and column counts from `TypedBatch::record_batch`.
pub fn arrow_dimensions(batch: &TypedBatch) -> Result<(usize, usize), CopyError> {
    let record = batch
        .record_batch()
        .map_err(|_| CopyError::new("arrow_batch", ""))?;
    Ok((record.num_rows(), record.num_columns()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Header,
    Rows,
    Trailer,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Position,
    Bool,
    Int2,
    Int4,
    Int8,
    Float4,
    Float8,
    Text { limit: Option<usize> },
    Json,
    Jsonb,
    Bytes,
    Uuid,
    Date,
    Time,
    Interval,
    Timestamp,
    Timestamptz,
    Numeric,
}

#[derive(Debug)]
struct Resolved {
    kind: Kind,
    slot: u32,
    nullable: bool,
    attnum: Option<i32>,
}

/// Pull encoder for one batch. A value error sticks until the caller drops it.
#[derive(Debug)]
pub struct CopyEncoder<'a> {
    batch: &'a TypedBatch,
    positions: &'a [ExactDecimal],
    columns: Vec<Resolved>,
    budgets: CopyBudgets,
    cursor: u32,
    rows: u32,
    lossless: usize,
    phase: Phase,
    error: Option<CopyError>,
}

impl<'a> CopyEncoder<'a> {
    /// Check budgets, column order, and positions. No chunk buffer is built here.
    pub fn open(
        batch: &'a TypedBatch,
        columns: &[CopyColumn],
        positions: &'a [ExactDecimal],
        budgets: CopyBudgets,
        unavailable: UnavailablePolicy,
    ) -> Result<Self, CopyError> {
        let UnavailablePolicy::Refuse = unavailable;
        if budgets.max_value_bytes == 0
            || budgets.max_row_bytes == 0
            || budgets.max_rows_per_chunk == 0
            || budgets.max_chunk_bytes < u32::try_from(HEADER_LEN).unwrap_or(u32::MAX)
        {
            return Err(CopyError::new("closed_budget", ""));
        }
        if columns.len() > usize::try_from(i16::MAX).unwrap_or(0) {
            return Err(CopyError::new("column_count", ""));
        }
        if !matches!(columns.first(), Some(CopyColumn::SystemPosition)) {
            return Err(CopyError::new("system_position_order", ""));
        }
        let systems = columns
            .iter()
            .filter(|column| matches!(column, CopyColumn::SystemPosition))
            .count();
        if systems != 1 {
            return Err(CopyError::new("duplicate_system_position", ""));
        }
        if columns.len() < 2 {
            return Err(CopyError::new("zero_user_columns", ""));
        }
        let rows = u32::try_from(batch.rows.len()).map_err(|_| CopyError::new("row_width", ""))?;
        if positions.len() != batch.rows.len() {
            return Err(CopyError::new("position_count", ""));
        }
        for (index, position) in positions.iter().enumerate() {
            if !position.fits_position() {
                let row = u32::try_from(index).unwrap_or(u32::MAX);
                return Err(CopyError::new("position_range", "").at(row, None));
            }
        }
        let lossless = batch
            .plans
            .iter()
            .filter(|plan| plan.mapping.decision == MapDecision::Lossless)
            .count();
        if u32::try_from(lossless).is_err() {
            return Err(CopyError::new("row_width", ""));
        }
        let mut resolved = Vec::with_capacity(columns.len());
        resolved.push(Resolved {
            kind: Kind::Position,
            slot: 0,
            nullable: false,
            attnum: None,
        });
        for (index, column) in columns.iter().enumerate().skip(1) {
            let CopyColumn::User { attnum } = column else {
                return Err(CopyError::new("duplicate_system_position", ""));
            };
            if columns[..index].iter().any(|earlier| {
                matches!(earlier, CopyColumn::User { attnum: previous } if previous == attnum)
            }) {
                return Err(CopyError::new("duplicate_user_column", "").at(0, Some(*attnum)));
            }
            resolved.push(resolve_user(batch, *attnum)?);
        }
        Ok(Self {
            batch,
            positions,
            columns: resolved,
            budgets,
            cursor: 0,
            rows,
            lossless,
            phase: Phase::Header,
            error: None,
        })
    }

    /// Next bounded piece. The first call is the header.
    pub fn pull(&mut self) -> Result<CopyPiece, CopyError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        match self.phase {
            Phase::Failed => Err(self.error.unwrap_or(CopyError::new("stopped", ""))),
            Phase::Header => self.pull_header(),
            Phase::Rows => self.pull_rows(),
            Phase::Trailer => self.pull_trailer(),
            Phase::Done => Ok(CopyPiece::Done),
        }
    }

    fn pull_header(&mut self) -> Result<CopyPiece, CopyError> {
        self.phase = if self.rows == 0 {
            Phase::Trailer
        } else {
            Phase::Rows
        };
        Ok(CopyPiece::Header {
            bytes: HEADER.to_vec(),
        })
    }

    fn pull_trailer(&mut self) -> Result<CopyPiece, CopyError> {
        self.phase = Phase::Done;
        Ok(CopyPiece::Trailer {
            bytes: TRAILER.to_vec(),
        })
    }

    fn pull_rows(&mut self) -> Result<CopyPiece, CopyError> {
        let chunk_cap = usize::try_from(self.budgets.max_chunk_bytes).unwrap_or(usize::MAX);
        let mut total = 0usize;
        let mut count = 0u32;
        let mut index = self.cursor;
        while index < self.rows && count < self.budgets.max_rows_per_chunk {
            let row_len = match self.row_len(index) {
                Ok(len) => len,
                Err(error) => return self.poison(error),
            };
            if row_len > chunk_cap
                || total
                    .checked_add(row_len)
                    .is_none_or(|next| next > chunk_cap)
            {
                if count == 0 {
                    return self.poison(CopyError::new("chunk_budget", "").at(index, None));
                }
                break;
            }
            total += row_len;
            count += 1;
            index += 1;
        }
        if count == 0 {
            self.phase = Phase::Trailer;
            return self.pull_trailer();
        }
        let mut bytes = Vec::with_capacity(total);
        for row in self.cursor..index {
            if let Err(error) = self.write_row(row, &mut bytes) {
                return self.poison(error);
            }
        }
        if bytes.len() != total {
            return self.poison(CopyError::new("encode_length", ""));
        }
        self.cursor = index;
        if self.cursor == self.rows {
            self.phase = Phase::Trailer;
        }
        Ok(CopyPiece::Rows {
            bytes,
            row_count: count,
        })
    }

    fn poison(&mut self, error: CopyError) -> Result<CopyPiece, CopyError> {
        self.phase = Phase::Failed;
        self.error = Some(error);
        Err(error)
    }

    fn row_len(&self, row: u32) -> Result<usize, CopyError> {
        let mut total = 2usize;
        for column in &self.columns {
            let field = match self.payload_len(row, column)? {
                None => 4,
                Some(len) => 4usize
                    .checked_add(len)
                    .ok_or_else(|| CopyError::new("encode_length", "").at(row, column.attnum))?,
            };
            total = total
                .checked_add(field)
                .ok_or_else(|| CopyError::new("encode_length", "").at(row, column.attnum))?;
        }
        let cap = usize::try_from(self.budgets.max_row_bytes).unwrap_or(usize::MAX);
        if total > cap {
            return Err(CopyError::new("row_budget", "").at(row, None));
        }
        Ok(total)
    }

    fn payload_len(&self, row: u32, column: &Resolved) -> Result<Option<usize>, CopyError> {
        if column.kind != Kind::Position && self.present(row, column)?.is_none() {
            return Ok(None);
        }
        let len = match column.kind {
            Kind::Position => numeric_layout(self.position(row)?)
                .map_err(|error| error.at(row, None))?
                .wire_len(),
            Kind::Bool => {
                let _checked = self.bool_of(row, column)?;
                1
            }
            Kind::Int2 => {
                let _checked = self.int_of(row, column, 2)?;
                2
            }
            Kind::Int4 => {
                let _checked = self.int_of(row, column, 4)?;
                4
            }
            Kind::Int8 => {
                let _checked = self.int_of(row, column, 8)?;
                8
            }
            Kind::Float4 => {
                let _checked = self.float4_of(row, column)?;
                4
            }
            Kind::Float8 => {
                let _checked = self.float8_of(row, column)?;
                8
            }
            Kind::Text { limit } => self.text_of(row, column, limit)?.len(),
            Kind::Json => self.text_of(row, column, None)?.len(),
            Kind::Jsonb => self.jsonb_of(row, column)?.len(),
            Kind::Bytes | Kind::Uuid => self.bytes_of(row, column)?.len(),
            Kind::Date => {
                let _checked = self.date_of(row, column)?;
                4
            }
            Kind::Time => {
                let _checked = self.time_of(row, column)?;
                8
            }
            Kind::Interval => {
                let _checked = self.interval_of(row, column)?;
                16
            }
            Kind::Timestamp | Kind::Timestamptz => {
                let _checked = self.timestamp_of(row, column)?;
                8
            }
            Kind::Numeric => numeric_layout(self.decimal_of(row, column)?)
                .map_err(|error| error.at(row, column.attnum))?
                .wire_len(),
        };
        self.budget_value(row, column, len)?;
        Ok(Some(len))
    }

    fn write_row(&self, row: u32, out: &mut Vec<u8>) -> Result<(), CopyError> {
        let start = out.len();
        let count = i16::try_from(self.columns.len())
            .map_err(|_| CopyError::new("column_count", "").at(row, None))?;
        out.extend_from_slice(&count.to_be_bytes());
        for column in &self.columns {
            match self.payload_len(row, column)? {
                None => out.extend_from_slice(&(-1i32).to_be_bytes()),
                Some(expected) => {
                    let mark = out.len();
                    out.extend_from_slice(&0i32.to_be_bytes());
                    let before = out.len();
                    self.write_payload(row, column, out)?;
                    let written = out.len() - before;
                    if written != expected {
                        return Err(CopyError::new("encode_length", "").at(row, column.attnum));
                    }
                    let wire = i32::try_from(written)
                        .map_err(|_| CopyError::new("encode_length", "").at(row, column.attnum))?;
                    out[mark..mark + 4].copy_from_slice(&wire.to_be_bytes());
                }
            }
        }
        if out.len() - start != self.row_len(row)? {
            return Err(CopyError::new("encode_length", "").at(row, None));
        }
        Ok(())
    }

    fn write_payload(
        &self,
        row: u32,
        column: &Resolved,
        out: &mut Vec<u8>,
    ) -> Result<(), CopyError> {
        match column.kind {
            Kind::Position => {
                write_numeric(out, self.position(row)?).map_err(|error| error.at(row, None))
            }
            Kind::Bool => {
                out.push(u8::from(self.bool_of(row, column)?));
                Ok(())
            }
            Kind::Int2 => {
                let value = i16::try_from(self.int_of(row, column, 2)?)
                    .map_err(|_| CopyError::new("value_mismatch", "int").at(row, column.attnum))?;
                out.extend_from_slice(&value.to_be_bytes());
                Ok(())
            }
            Kind::Int4 => {
                let value = i32::try_from(self.int_of(row, column, 4)?)
                    .map_err(|_| CopyError::new("value_mismatch", "int").at(row, column.attnum))?;
                out.extend_from_slice(&value.to_be_bytes());
                Ok(())
            }
            Kind::Int8 => {
                out.extend_from_slice(&self.int_of(row, column, 8)?.to_be_bytes());
                Ok(())
            }
            Kind::Float4 => {
                out.extend_from_slice(&self.float4_of(row, column)?.to_be_bytes());
                Ok(())
            }
            Kind::Float8 => {
                out.extend_from_slice(&self.float8_of(row, column)?.to_be_bytes());
                Ok(())
            }
            Kind::Text { limit } => {
                out.extend_from_slice(self.text_of(row, column, limit)?.as_bytes());
                Ok(())
            }
            Kind::Json => {
                out.extend_from_slice(self.text_of(row, column, None)?.as_bytes());
                Ok(())
            }
            Kind::Jsonb => {
                out.extend_from_slice(&self.jsonb_of(row, column)?);
                Ok(())
            }
            Kind::Bytes | Kind::Uuid => {
                out.extend_from_slice(self.bytes_of(row, column)?);
                Ok(())
            }
            Kind::Date => {
                out.extend_from_slice(&self.date_of(row, column)?.to_be_bytes());
                Ok(())
            }
            Kind::Time => {
                out.extend_from_slice(&self.time_of(row, column)?.to_be_bytes());
                Ok(())
            }
            Kind::Interval => {
                out.extend_from_slice(&self.interval_of(row, column)?.to_be_bytes());
                out.extend_from_slice(&0_i32.to_be_bytes());
                out.extend_from_slice(&0_i32.to_be_bytes());
                Ok(())
            }
            Kind::Timestamp | Kind::Timestamptz => {
                out.extend_from_slice(&self.timestamp_of(row, column)?.to_be_bytes());
                Ok(())
            }
            Kind::Numeric => write_numeric(out, self.decimal_of(row, column)?)
                .map_err(|error| error.at(row, column.attnum)),
        }
    }

    fn budget_value(&self, row: u32, column: &Resolved, len: usize) -> Result<(), CopyError> {
        let cap = usize::try_from(self.budgets.max_value_bytes).unwrap_or(usize::MAX);
        if len > cap || len > usize::try_from(i32::MAX).unwrap_or(usize::MAX) {
            Err(CopyError::new("value_budget", "").at(row, column.attnum))
        } else {
            Ok(())
        }
    }

    fn position(&self, row: u32) -> Result<&ExactDecimal, CopyError> {
        self.positions
            .get(usize::try_from(row).unwrap_or(usize::MAX))
            .ok_or_else(|| CopyError::new("position_count", "").at(row, None))
    }

    fn cell(&self, row: u32, column: &Resolved) -> Result<&DecodedCell, CopyError> {
        let index = usize::try_from(row).unwrap_or(usize::MAX);
        let cells = &self
            .batch
            .rows
            .get(index)
            .ok_or_else(|| CopyError::new("row_width", "").at(row, column.attnum))?
            .cells;
        if cells.len() != self.lossless {
            return Err(CopyError::new("row_width", "").at(row, column.attnum));
        }
        let slot = usize::try_from(column.slot).unwrap_or(usize::MAX);
        cells
            .get(slot)
            .ok_or_else(|| CopyError::new("row_width", "").at(row, column.attnum))
    }

    /// `None` is SQL NULL. Unavailable is an error and is not NULL.
    fn present(&self, row: u32, column: &Resolved) -> Result<Option<&DecodedCell>, CopyError> {
        let cell = self.cell(row, column)?;
        match cell.presence {
            CellPresence::Null => {
                if cell.value.is_some()
                    || cell.reject.is_some()
                    || !matches!(cell.arrow, None | Some(ArrowScalar::Null))
                {
                    return Err(CopyError::new("inconsistent_cell", "").at(row, column.attnum));
                }
                if !column.nullable {
                    return Err(CopyError::new("null_not_allowed", "").at(row, column.attnum));
                }
                Ok(None)
            }
            CellPresence::Unavailable => {
                let detail = cell
                    .reject
                    .map(|reject| reject.reason)
                    .unwrap_or("unavailable");
                Err(CopyError::new("unavailable", detail).at(row, column.attnum))
            }
            CellPresence::Value => {
                if cell.value.is_none()
                    || cell.reject.is_some()
                    || matches!(cell.arrow, None | Some(ArrowScalar::Null))
                {
                    return Err(CopyError::new("inconsistent_cell", "").at(row, column.attnum));
                }
                Ok(Some(cell))
            }
        }
    }

    fn bool_of(&self, row: u32, column: &Resolved) -> Result<bool, CopyError> {
        let cell = self.required(row, column)?;
        match (cell.value.as_ref(), cell.arrow.as_ref()) {
            (Some(LogicalValue::Bool(value)), Some(ArrowScalar::Bool(arrow))) if value == arrow => {
                Ok(*value)
            }
            _ => Err(CopyError::new("value_mismatch", "bool").at(row, column.attnum)),
        }
    }

    fn int_of(&self, row: u32, column: &Resolved, width: u8) -> Result<i64, CopyError> {
        let cell = self.required(row, column)?;
        let Some(LogicalValue::Int(exact)) = cell.value.as_ref() else {
            return Err(CopyError::new("value_mismatch", "int").at(row, column.attnum));
        };
        let number = i64_of(exact)
            .ok_or_else(|| CopyError::new("value_mismatch", "int").at(row, column.attnum))?;
        let arrow_ok = match (width, cell.arrow.as_ref()) {
            (2, Some(ArrowScalar::Int16(value))) => i64::from(*value) == number,
            (4, Some(ArrowScalar::Int32(value))) => i64::from(*value) == number,
            (8, Some(ArrowScalar::Int64(value))) => *value == number,
            _ => false,
        };
        if !arrow_ok {
            return Err(CopyError::new("value_mismatch", "int").at(row, column.attnum));
        }
        let fits = match width {
            2 => i16::try_from(number).is_ok(),
            4 => i32::try_from(number).is_ok(),
            _ => true,
        };
        if !fits {
            return Err(CopyError::new("value_mismatch", "int").at(row, column.attnum));
        }
        Ok(number)
    }

    fn float8_of(&self, row: u32, column: &Resolved) -> Result<u64, CopyError> {
        let cell = self.required(row, column)?;
        self.plain_cell(row, column, cell)?;
        let Some(ArrowScalar::Float64(bits)) = cell.arrow.as_ref() else {
            return Err(CopyError::new("value_mismatch", "float").at(row, column.attnum));
        };
        if cell.float_bits.is_some_and(|stored| stored != *bits) {
            return Err(CopyError::new("value_mismatch", "float_bits").at(row, column.attnum));
        }
        let Some(LogicalValue::Float(class)) = cell.value.as_ref() else {
            return Err(CopyError::new("value_mismatch", "float").at(row, column.attnum));
        };
        if !float_class_matches(*class, *bits) {
            return Err(CopyError::new("value_mismatch", "float").at(row, column.attnum));
        }
        Ok(*bits)
    }

    fn float4_of(&self, row: u32, column: &Resolved) -> Result<u32, CopyError> {
        let bits = self.float8_of(row, column)?;
        let wide = f64::from_bits(bits);
        let narrowed = wide as f32;
        if narrowed.is_nan() {
            return Err(CopyError::new("float4_nan", "").at(row, column.attnum));
        }
        if f64::from(narrowed).to_bits() != bits {
            return Err(CopyError::new("float_precision", "").at(row, column.attnum));
        }
        Ok(narrowed.to_bits())
    }

    fn text_of(
        &self,
        row: u32,
        column: &Resolved,
        limit: Option<usize>,
    ) -> Result<&str, CopyError> {
        let cell = self.required(row, column)?;
        self.plain_cell(row, column, cell)?;
        match (cell.value.as_ref(), cell.arrow.as_ref()) {
            (Some(LogicalValue::Text(text)), Some(ArrowScalar::Utf8(arrow))) if text == arrow => {
                if text.as_bytes().contains(&0) {
                    return Err(CopyError::new("text_nul", "").at(row, column.attnum));
                }
                if limit.is_some_and(|max| text.chars().count() > max) {
                    return Err(CopyError::new("char_length", "").at(row, column.attnum));
                }
                Ok(text.as_str())
            }
            _ => Err(CopyError::new("value_mismatch", "text").at(row, column.attnum)),
        }
    }

    fn jsonb_of(&self, row: u32, column: &Resolved) -> Result<Vec<u8>, CopyError> {
        let cell = self.required(row, column)?;
        self.plain_cell(row, column, cell)?;
        let Some(LogicalValue::Json(node)) = cell.value.as_ref() else {
            return Err(CopyError::new("value_mismatch", "jsonb").at(row, column.attnum));
        };
        let Some(ArrowScalar::Utf8(text)) = cell.arrow.as_ref() else {
            return Err(CopyError::new("value_mismatch", "jsonb").at(row, column.attnum));
        };
        if text.as_bytes().contains(&0) {
            return Err(CopyError::new("text_nul", "").at(row, column.attnum));
        }
        let parsed = parse_json(text)
            .map_err(|_| CopyError::new("value_mismatch", "jsonb").at(row, column.attnum))?;
        if &parsed != node {
            return Err(CopyError::new("value_mismatch", "jsonb").at(row, column.attnum));
        }
        let mut wire = Vec::with_capacity(text.len().saturating_add(1));
        wire.push(oo_core::jsonb_doc::JSONB_SEND_VERSION);
        wire.extend_from_slice(text.as_bytes());
        let file = oo_core::jsonb_doc::inspect_jsonb_send(&wire)
            .map_err(|_| CopyError::new("value_mismatch", "jsonb").at(row, column.attnum))?;
        if file.text != *text {
            return Err(CopyError::new("value_mismatch", "jsonb").at(row, column.attnum));
        }
        Ok(wire)
    }

    fn bytes_of(&self, row: u32, column: &Resolved) -> Result<&[u8], CopyError> {
        let cell = self.required(row, column)?;
        self.plain_cell(row, column, cell)?;
        match (cell.value.as_ref(), cell.arrow.as_ref()) {
            (Some(LogicalValue::Bytes(value)), Some(ArrowScalar::Binary(arrow)))
                if value == arrow =>
            {
                if column.kind == Kind::Uuid && value.len() != 16 {
                    return Err(
                        CopyError::new("value_mismatch", "uuid_payload").at(row, column.attnum)
                    );
                }
                Ok(value.as_slice())
            }
            _ => Err(CopyError::new("value_mismatch", "bytes").at(row, column.attnum)),
        }
    }

    fn date_of(&self, row: u32, column: &Resolved) -> Result<i32, CopyError> {
        let cell = self.required(row, column)?;
        self.plain_cell(row, column, cell)?;
        let Some(LogicalValue::Date { year, month, day }) = cell.value.as_ref() else {
            return Err(CopyError::new("value_mismatch", "date").at(row, column.attnum));
        };
        validate_date(*year, *month, *day)
            .map_err(|_| CopyError::new("date_range", "").at(row, column.attnum))?;
        let unix = unix_days(*year, *month, *day);
        let arrow_days = i32::try_from(unix)
            .map_err(|_| CopyError::new("date_range", "").at(row, column.attnum))?;
        let Some(ArrowScalar::Date32(arrow)) = cell.arrow.as_ref() else {
            return Err(CopyError::new("value_mismatch", "date").at(row, column.attnum));
        };
        if *arrow != arrow_days {
            return Err(CopyError::new("value_mismatch", "date").at(row, column.attnum));
        }
        i32::try_from(unix - PG_UNIX_DAYS)
            .map_err(|_| CopyError::new("date_range", "").at(row, column.attnum))
    }

    fn time_of(&self, row: u32, column: &Resolved) -> Result<i64, CopyError> {
        let cell = self.required(row, column)?;
        self.plain_cell(row, column, cell)?;
        match (cell.value.as_ref(), cell.arrow.as_ref()) {
            (Some(LogicalValue::TimeMicros(value)), Some(ArrowScalar::Time64(arrow)))
                if value == arrow && (0..86_400_000_000).contains(value) =>
            {
                Ok(*value)
            }
            (Some(LogicalValue::TimeMicros(value)), _) if !(0..86_400_000_000).contains(value) => {
                Err(CopyError::new("time_range", "").at(row, column.attnum))
            }
            _ => Err(CopyError::new("value_mismatch", "time").at(row, column.attnum)),
        }
    }

    fn interval_of(&self, row: u32, column: &Resolved) -> Result<i64, CopyError> {
        let cell = self.required(row, column)?;
        self.plain_cell(row, column, cell)?;
        match (cell.value.as_ref(), cell.arrow.as_ref()) {
            (
                Some(LogicalValue::DurationMicros(value)),
                Some(ArrowScalar::DurationMicros(arrow)),
            ) if value == arrow => Ok(*value),
            _ => Err(CopyError::new("value_mismatch", "interval").at(row, column.attnum)),
        }
    }

    fn timestamp_of(&self, row: u32, column: &Resolved) -> Result<i64, CopyError> {
        let cell = self.required(row, column)?;
        self.plain_cell(row, column, cell)?;
        let unix = match (column.kind, cell.value.as_ref(), cell.arrow.as_ref()) {
            (
                Kind::Timestamp,
                Some(LogicalValue::TimestampNaive {
                    year,
                    month,
                    day,
                    hour,
                    minute,
                    second,
                    micros,
                }),
                Some(ArrowScalar::TimestampNaive(arrow)),
            ) => {
                validate_date(*year, *month, *day)
                    .map_err(|_| CopyError::new("timestamp_range", "").at(row, column.attnum))?;
                if *hour > 23 || *minute > 59 || *second > 59 || *micros > 999_999 {
                    return Err(CopyError::new("timestamp_range", "").at(row, column.attnum));
                }
                let computed = naive_unix(*year, *month, *day, *hour, *minute, *second, *micros)
                    .map_err(|_| CopyError::new("timestamp_range", "").at(row, column.attnum))?;
                if computed != *arrow {
                    return Err(
                        CopyError::new("value_mismatch", "timestamp").at(row, column.attnum)
                    );
                }
                computed
            }
            (
                Kind::Timestamptz,
                Some(LogicalValue::TimestampTzMicros(value)),
                Some(ArrowScalar::TimestampUtc(arrow)),
            ) if value == arrow => *value,
            _ => {
                return Err(CopyError::new("value_mismatch", "timestamp").at(row, column.attnum));
            }
        };
        i64::try_from(i128::from(unix) - PG_UNIX_MICROS)
            .map_err(|_| CopyError::new("timestamp_range", "").at(row, column.attnum))
    }

    fn decimal_of(&self, row: u32, column: &Resolved) -> Result<&ExactDecimal, CopyError> {
        let cell = self.required(row, column)?;
        if cell.float_bits.is_some() {
            return Err(CopyError::new("inconsistent_cell", "float_bits").at(row, column.attnum));
        }
        let Some(LogicalValue::Decimal(exact)) = cell.value.as_ref() else {
            return Err(CopyError::new("value_mismatch", "numeric").at(row, column.attnum));
        };
        if exact.scale() < 0 {
            return Err(CopyError::new("numeric_scale", "negative").at(row, column.attnum));
        }
        let plan = self.plan(column)?;
        let Some(LogicalType::Decimal { precision, scale }) = &plan.logical else {
            return Err(CopyError::new("value_mismatch", "numeric").at(row, column.attnum));
        };
        if *precision == 0 && *scale != ARBITRARY_DECIMAL_SCALE {
            return Err(CopyError::new("value_mismatch", "numeric_scale").at(row, column.attnum));
        }
        if *precision != 0 && exact.scale() != i32::from(*scale) {
            return Err(CopyError::new("value_mismatch", "numeric_scale").at(row, column.attnum));
        }
        if *precision != 0 && exact.coefficient().len() > usize::from(*precision) {
            return Err(
                CopyError::new("value_mismatch", "numeric_precision").at(row, column.attnum)
            );
        }
        if let Some(actual) = cell.actual_scale
            && actual != exact.scale()
        {
            return Err(CopyError::new("value_mismatch", "numeric_scale").at(row, column.attnum));
        }
        if let Some(actual) = cell.actual_precision {
            let storage = i32::try_from(exact.storage_precision()).unwrap_or(i32::MAX);
            if actual != storage {
                return Err(
                    CopyError::new("value_mismatch", "numeric_precision").at(row, column.attnum)
                );
            }
        }
        if !decimal_arrow_matches(exact, cell.arrow.as_ref(), plan.arrow.as_ref()) {
            return Err(CopyError::new("value_mismatch", "numeric_arrow").at(row, column.attnum));
        }
        Ok(exact)
    }

    fn required(&self, row: u32, column: &Resolved) -> Result<&DecodedCell, CopyError> {
        self.present(row, column)?
            .ok_or_else(|| CopyError::new("inconsistent_cell", "").at(row, column.attnum))
    }

    fn plain_cell(&self, row: u32, column: &Resolved, cell: &DecodedCell) -> Result<(), CopyError> {
        if cell.actual_precision.is_some() || cell.actual_scale.is_some() {
            return Err(CopyError::new("inconsistent_cell", "actual_scale").at(row, column.attnum));
        }
        if !matches!(column.kind, Kind::Float4 | Kind::Float8) && cell.float_bits.is_some() {
            return Err(CopyError::new("inconsistent_cell", "float_bits").at(row, column.attnum));
        }
        Ok(())
    }

    fn plan(&self, column: &Resolved) -> Result<&ColumnPlan, CopyError> {
        let attnum = column
            .attnum
            .ok_or_else(|| CopyError::new("unknown_column", ""))?;
        self.batch
            .plans
            .iter()
            .find(|plan| plan.attnum == attnum)
            .ok_or_else(|| CopyError::new("unknown_column", "").at(0, Some(attnum)))
    }
}

fn resolve_user(batch: &TypedBatch, attnum: i32) -> Result<Resolved, CopyError> {
    let mut slot = 0u32;
    for plan in &batch.plans {
        if plan.attnum == attnum {
            return match plan.mapping.decision {
                MapDecision::Lossless => Ok(Resolved {
                    kind: kind_of(plan)?,
                    slot,
                    nullable: plan.mapping.nullable,
                    attnum: Some(attnum),
                }),
                MapDecision::Unsupported(reason) => {
                    Err(CopyError::new("unsupported_column", reason).at(0, Some(attnum)))
                }
            };
        }
        if plan.mapping.decision == MapDecision::Lossless {
            slot = slot
                .checked_add(1)
                .ok_or_else(|| CopyError::new("row_width", ""))?;
        }
    }
    Err(CopyError::new("unknown_column", "").at(0, Some(attnum)))
}

fn kind_of(plan: &ColumnPlan) -> Result<Kind, CopyError> {
    let attnum = Some(plan.attnum);
    if plan.element.is_some() {
        return Err(CopyError::new("type_not_encoded", "array").at(0, attnum));
    }
    if !plan.enum_labels.is_empty() {
        return Err(CopyError::new("type_not_encoded", "enum").at(0, attnum));
    }
    let Some(logical) = plan.logical.as_ref() else {
        return Err(CopyError::new("type_not_encoded", "logical").at(0, attnum));
    };
    let Some(arrow) = plan.arrow.as_ref() else {
        return Err(CopyError::new("type_not_encoded", "arrow").at(0, attnum));
    };
    let kind = match plan.decode_oid {
        OID_BOOL => Kind::Bool,
        OID_INT2 => Kind::Int2,
        OID_INT4 => Kind::Int4,
        OID_INT8 => Kind::Int8,
        OID_FLOAT4 => Kind::Float4,
        OID_FLOAT8 => Kind::Float8,
        OID_TEXT => Kind::Text { limit: None },
        OID_VARCHAR | OID_BPCHAR => Kind::Text {
            limit: char_limit(plan)?,
        },
        OID_JSON => Kind::Json,
        OID_JSONB => {
            if plan.mapping.precision.is_some()
                || plan.mapping.scale.is_some()
                || plan.mapping.timezone.is_some()
            {
                return Err(CopyError::new("type_not_encoded", "jsonb").at(0, attnum));
            }
            Kind::Jsonb
        }
        OID_BYTEA => Kind::Bytes,
        OID_UUID => Kind::Uuid,
        OID_DATE => Kind::Date,
        OID_TIME => Kind::Time,
        OID_TIMESTAMP => Kind::Timestamp,
        OID_TIMESTAMPTZ => Kind::Timestamptz,
        OID_NUMERIC => numeric_kind(plan, logical, attnum)?,
        OID_INTERVAL => Kind::Interval,
        _ => return Err(CopyError::new("type_not_encoded", "oid").at(0, attnum)),
    };
    if !logical_matches(logical, kind) || !arrow_matches(arrow, kind) {
        return Err(CopyError::new("type_not_encoded", "logical").at(0, attnum));
    }
    Ok(kind)
}

fn numeric_kind(
    plan: &ColumnPlan,
    logical: &LogicalType,
    attnum: Option<i32>,
) -> Result<Kind, CopyError> {
    match logical {
        LogicalType::Decimal { precision, scale } if *precision == 0 => {
            if *scale != ARBITRARY_DECIMAL_SCALE {
                return Err(CopyError::new("type_not_encoded", "numeric_scale").at(0, attnum));
            }
        }
        LogicalType::Decimal { scale, .. } if *scale < 0 => {
            return Err(CopyError::new("numeric_scale", "negative").at(0, attnum));
        }
        LogicalType::Decimal { .. } => {}
        _ => return Err(CopyError::new("type_not_encoded", "numeric").at(0, attnum)),
    }
    match plan.arrow {
        Some(
            ArrowPhysical::Decimal128 { .. }
            | ArrowPhysical::Decimal256 { .. }
            | ArrowPhysical::DecimalUtf8 { .. },
        ) => Ok(Kind::Numeric),
        _ => Err(CopyError::new("type_not_encoded", "arrow").at(0, attnum)),
    }
}

fn char_limit(plan: &ColumnPlan) -> Result<Option<usize>, CopyError> {
    match plan.mapping.precision {
        None => Ok(None),
        Some(value) if value >= 0 => usize::try_from(value)
            .map(Some)
            .map_err(|_| CopyError::new("char_length", "precision").at(0, Some(plan.attnum))),
        Some(_) => Err(CopyError::new("type_not_encoded", "char_length").at(0, Some(plan.attnum))),
    }
}

fn logical_matches(logical: &LogicalType, kind: Kind) -> bool {
    matches!(
        (kind, logical),
        (Kind::Bool, LogicalType::Bool)
            | (Kind::Int2 | Kind::Int4 | Kind::Int8, LogicalType::Int)
            | (Kind::Float4 | Kind::Float8, LogicalType::Float64)
            | (Kind::Text { .. } | Kind::Json, LogicalType::Text)
            | (Kind::Jsonb, LogicalType::Json)
            | (Kind::Bytes | Kind::Uuid, LogicalType::Bytes)
            | (Kind::Date, LogicalType::Date)
            | (Kind::Time, LogicalType::Time)
            | (Kind::Interval, LogicalType::DurationMicros)
            | (Kind::Timestamp, LogicalType::TimestampNaive)
            | (Kind::Timestamptz, LogicalType::TimestampTz)
            | (Kind::Numeric, LogicalType::Decimal { .. })
    )
}

fn arrow_matches(arrow: &ArrowPhysical, kind: Kind) -> bool {
    matches!(
        (kind, arrow),
        (Kind::Bool, ArrowPhysical::Boolean)
            | (Kind::Int2, ArrowPhysical::Int16)
            | (Kind::Int4, ArrowPhysical::Int32)
            | (Kind::Int8, ArrowPhysical::Int64)
            | (Kind::Float4 | Kind::Float8, ArrowPhysical::Float64)
            | (
                Kind::Text { .. } | Kind::Json | Kind::Jsonb,
                ArrowPhysical::Utf8
            )
            | (Kind::Bytes | Kind::Uuid, ArrowPhysical::Binary)
            | (Kind::Date, ArrowPhysical::Date32)
            | (Kind::Time, ArrowPhysical::Time64Micros)
            | (Kind::Interval, ArrowPhysical::DurationMicros)
            | (Kind::Timestamp, ArrowPhysical::TimestampMicrosNaive)
            | (Kind::Timestamptz, ArrowPhysical::TimestampMicrosUtc)
            | (
                Kind::Numeric,
                ArrowPhysical::Decimal128 { .. }
                    | ArrowPhysical::Decimal256 { .. }
                    | ArrowPhysical::DecimalUtf8 { .. },
            )
    )
}

fn float_class_matches(class: FloatClass, bits: u64) -> bool {
    match (class, classify_f64(f64::from_bits(bits))) {
        (FloatClass::Nan, FloatClass::Nan)
        | (FloatClass::Zero, FloatClass::Zero)
        | (FloatClass::PosInf, FloatClass::PosInf)
        | (FloatClass::NegInf, FloatClass::NegInf) => true,
        (FloatClass::Finite(stored), FloatClass::Finite(actual)) => {
            stored == bits && actual == bits
        }
        _ => false,
    }
}

struct NumericLayout {
    ndigits: u16,
    weight: i16,
    negative: bool,
    dscale: i16,
}

impl NumericLayout {
    fn wire_len(self) -> usize {
        8 + 2 * usize::from(self.ndigits)
    }
}

fn numeric_layout(exact: &ExactDecimal) -> Result<NumericLayout, CopyError> {
    if exact.scale() < 0 {
        return Err(CopyError::new("numeric_scale", "negative"));
    }
    let dscale =
        i16::try_from(exact.scale()).map_err(|_| CopyError::new("numeric_range", "dscale"))?;
    if exact.coefficient() == "0" {
        return Ok(NumericLayout {
            ndigits: 0,
            weight: 0,
            negative: false,
            dscale,
        });
    }
    let (high, low) =
        nonzero_offsets(exact).ok_or_else(|| CopyError::new("numeric_range", "digits"))?;
    let high_weight = high.div_euclid(4);
    let low_weight = low.div_euclid(4);
    let count = high_weight
        .checked_sub(low_weight)
        .and_then(|span| span.checked_add(1))
        .ok_or_else(|| CopyError::new("numeric_range", "ndigits"))?;
    Ok(NumericLayout {
        ndigits: u16::try_from(count).map_err(|_| CopyError::new("numeric_range", "ndigits"))?,
        weight: i16::try_from(high_weight)
            .map_err(|_| CopyError::new("numeric_range", "weight"))?,
        negative: exact.is_negative(),
        dscale,
    })
}

/// Base-10000 groups. Weight 0 contains the units digit. Trailing zero groups
/// are omitted; `dscale` still records the fractional digits.
fn write_numeric(out: &mut Vec<u8>, exact: &ExactDecimal) -> Result<(), CopyError> {
    let layout = numeric_layout(exact)?;
    out.extend_from_slice(&layout.ndigits.to_be_bytes());
    out.extend_from_slice(&layout.weight.to_be_bytes());
    let sign: u16 = if layout.negative { 0x4000 } else { 0 };
    out.extend_from_slice(&sign.to_be_bytes());
    out.extend_from_slice(&layout.dscale.to_be_bytes());
    for index in 0..layout.ndigits {
        let weight = i32::from(layout.weight) - i32::from(index);
        let digit =
            group_at(exact, weight).ok_or_else(|| CopyError::new("numeric_range", "digit"))?;
        out.extend_from_slice(&digit.to_be_bytes());
    }
    Ok(())
}

fn nonzero_offsets(exact: &ExactDecimal) -> Option<(i32, i32)> {
    let bytes = exact.coefficient().as_bytes();
    let right = exact.scale().checked_neg()?;
    let mut high = None;
    let mut low = None;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'0' {
            continue;
        }
        let from_right = i32::try_from(bytes.len() - 1 - index).ok()?;
        let offset = right.checked_add(from_right)?;
        if high.is_none() {
            high = Some(offset);
        }
        low = Some(offset);
    }
    Some((high?, low?))
}

fn dec_digit(exact: &ExactDecimal, offset: i32) -> u8 {
    let bytes = exact.coefficient().as_bytes();
    let Ok(len) = i32::try_from(bytes.len()) else {
        return 0;
    };
    let Some(right) = exact.scale().checked_neg() else {
        return 0;
    };
    let left = right + len - 1;
    if offset > left || offset < right {
        return 0;
    }
    let Ok(index) = usize::try_from(left - offset) else {
        return 0;
    };
    bytes.get(index).copied().unwrap_or(b'0') - b'0'
}

fn group_at(exact: &ExactDecimal, weight: i32) -> Option<u16> {
    let base = weight.checked_mul(4)?;
    let mut value = 0u16;
    for place in (0..4).rev() {
        let offset = base.checked_add(place)?;
        value = value
            .checked_mul(10)?
            .checked_add(u16::from(dec_digit(exact, offset)))?;
    }
    if value < 10_000 { Some(value) } else { None }
}

fn decimal_arrow_matches(
    exact: &ExactDecimal,
    arrow: Option<&ArrowScalar>,
    physical: Option<&ArrowPhysical>,
) -> bool {
    match (physical, arrow) {
        (Some(ArrowPhysical::DecimalUtf8 { .. }), Some(ArrowScalar::Utf8(text))) => {
            text == &exact.to_plain_string()
        }
        (
            Some(ArrowPhysical::Decimal128 { scale, .. }),
            Some(ArrowScalar::Decimal128(unscaled)),
        ) => i32::from(*scale) == exact.scale() && unscaled_i128(exact) == Some(*unscaled),
        (Some(ArrowPhysical::Decimal256 { scale, .. }), Some(ArrowScalar::Decimal256(bytes))) => {
            i32::from(*scale) == exact.scale() && unscaled_i256(exact).as_ref() == Some(bytes)
        }
        _ => false,
    }
}

fn unscaled_i128(exact: &ExactDecimal) -> Option<i128> {
    let mut value: i128 = 0;
    for byte in exact.coefficient().bytes() {
        value = value
            .checked_mul(10)?
            .checked_add(i128::from(byte - b'0'))?;
    }
    if exact.is_negative() {
        value = value.checked_neg()?;
    }
    Some(value)
}

fn unscaled_i256(exact: &ExactDecimal) -> Option<[u8; 32]> {
    let mut bytes = [0u8; 32];
    for digit in exact.coefficient().bytes() {
        mul_add_le(&mut bytes, 10, u32::from(digit - b'0')).ok()?;
    }
    if exact.is_negative() && exact.coefficient() != "0" {
        neg_le(&mut bytes).ok()?;
    }
    Some(bytes)
}

fn mul_add_le(bytes: &mut [u8; 32], factor: u32, add: u32) -> Result<(), ()> {
    let mut carry = add;
    for byte in bytes.iter_mut() {
        let wide = u32::from(*byte) * factor + carry;
        *byte = (wide & 0xff) as u8;
        carry = wide >> 8;
    }
    if carry == 0 { Ok(()) } else { Err(()) }
}

fn neg_le(bytes: &mut [u8; 32]) -> Result<(), ()> {
    let mut carry = 1u16;
    for byte in bytes.iter_mut() {
        let wide = u16::from(!*byte) + carry;
        *byte = (wide & 0xff) as u8;
        carry = wide >> 8;
    }
    if bytes.last().is_some_and(|byte| byte & 0x80 != 0) {
        Ok(())
    } else {
        Err(())
    }
}

fn i64_of(exact: &oo_core::ExactInt) -> Option<i64> {
    let mut value: i128 = 0;
    for byte in exact.coefficient().bytes() {
        value = value
            .checked_mul(10)?
            .checked_add(i128::from(byte - b'0'))?;
    }
    if exact.is_negative() {
        value = value.checked_neg()?;
    }
    i64::try_from(value).ok()
}

fn unix_days(year: i32, month: u8, day: u8) -> i64 {
    let year = i64::from(year);
    let shifted = if month <= 2 { year - 1 } else { year };
    let era = if shifted >= 0 { shifted } else { shifted - 399 } / 400;
    let year_of_era = u64::try_from(shifted - era * 400).unwrap_or(0);
    let month_prime = if month > 2 {
        u64::from(month) - 3
    } else {
        u64::from(month) + 9
    };
    let day_of_year = (153 * month_prime + 2) / 5 + u64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + i64::try_from(day_of_era).unwrap_or(0) - 719_468
}

fn naive_unix(
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    micros: u32,
) -> Result<i64, ()> {
    let days = i128::from(unix_days(year, month, day));
    let sod = i128::from(hour) * 3_600 + i128::from(minute) * 60 + i128::from(second);
    let unix = days
        .checked_mul(DAY_MICROS)
        .ok_or(())?
        .checked_add(sod.checked_mul(1_000_000).ok_or(())?)
        .ok_or(())?
        .checked_add(i128::from(micros))
        .ok_or(())?;
    i64::try_from(unix).map_err(|_| ())
}
