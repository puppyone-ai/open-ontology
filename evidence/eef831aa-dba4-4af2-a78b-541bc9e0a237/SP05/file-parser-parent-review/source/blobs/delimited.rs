//! Bounded CSV and TSV pull. Protocol `oo.source.delimited/v1`.
//!
//! The caller supplies an explicit dialect, BOM policy, header policy, null
//! policy, column profile, and the SHA-256 of the whole object. This reader
//! does not sniff a delimiter, a column type, or a semantic class. It does not
//! import, publish, or open a database. `DelimitedComplete` is minted only
//! after a clean EOF, every profile column was covered, and the streamed byte
//! hash matches. It is not `RunComplete` and it does not admit a load.
//!
//! A caller `Read` can block inside one `read`. Cancel and the deadline are
//! checked before each `read`, not during it. This module does not claim that
//! a stalled read is interrupted.

use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use arrow_array::{Array, ArrayRef, BooleanArray, Decimal128Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use oo_core::{DecimalError, ExactDecimal};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

/// Delimited source protocol. This is not `oo.source.pgsql/v1` and not JSONL.
pub const DELIMITED_PROTOCOL: &str = "oo.source.delimited/v1";

/// Profile identity version. The preimage is documented in the contract.
pub const DELIMITED_PROFILE_VERSION: &str = "oo.source.delimited.profile/v1";

/// Row and field lineage carried on each batch.
pub const DELIMITED_LINEAGE_VERSION: &str = "oo.source.delimited.lineage/v1";

/// Header and completion receipt version.
pub const DELIMITED_RECEIPT_VERSION: &str = "oo.source.delimited.receipt/v1";

/// Hard cap for one pull's row count.
pub const DELIMITED_MAX_BATCH_ROWS: u32 = 65_536;

/// Hard cap for the raw record bytes accumulated in one batch.
pub const DELIMITED_MAX_BATCH_BYTES: u64 = 16 * 1024 * 1024;

/// Hard cap for one record, including quotes, separators, and the record ending.
/// The floor is 3 so a UTF-8 BOM fits in the single record window.
pub const DELIMITED_MAX_RECORD_BYTES: u32 = 1_048_576;

/// Minimum record window. It is the UTF-8 BOM length.
pub const DELIMITED_MIN_RECORD_BYTES: u32 = 3;

/// Hard cap for one decoded field. This is not a second copy of the record window.
pub const DELIMITED_MAX_FIELD_BYTES: u32 = 1_048_576;

/// Hard cap for hashed input bytes. This is not an allocation of that size.
pub const DELIMITED_MAX_TOTAL_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// Hard cap for data rows emitted by one reader. The header is not a data row.
pub const DELIMITED_MAX_ROWS: u64 = 16_777_216;

/// Hard cap for profile columns and for `max_columns`.
pub const DELIMITED_MAX_COLUMNS: u32 = 256;

/// Hard cap for fields in one record.
pub const DELIMITED_MAX_FIELDS: u32 = 256;

/// Maximum byte length of a profile id or a column name.
pub const DELIMITED_MAX_NAME_BYTES: usize = 256;

/// Maximum byte length of an explicit null token.
pub const DELIMITED_MAX_NULL_TOKEN_BYTES: usize = 32;

const PROFILE_PREFIX: &str = "oo-delimited-profile/v1\n";
const CONFIG_PREFIX: &str = "oo-delimited-config-identity/v1\n";
const BOM: [u8; 3] = [0xef, 0xbb, 0xbf];

/// Closed delimited failure. Display is a kind and a static fact. It has no cell bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelimitedError {
    kind: DelimitedErrorKind,
    fact: &'static str,
}

/// Class of a delimited failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelimitedErrorKind {
    /// The profile or budget is rejected before a read.
    Config,
    /// A row, byte, field, or deadline limit failed.
    Budget,
    /// The bytes are not the profile's CSV or TSV contract.
    Protocol,
    /// The input is not strict UTF-8, or the BOM policy refused it.
    Encoding,
    /// The full-object SHA-256 did not match. Compared only at a clean EOF.
    Hash,
    /// The caller cancelled the pull.
    Cancelled,
    /// The underlying reader failed. The OS message is dropped.
    Io,
}

impl DelimitedError {
    const fn new(kind: DelimitedErrorKind, fact: &'static str) -> Self {
        Self { kind, fact }
    }

    pub const fn config(fact: &'static str) -> Self {
        Self::new(DelimitedErrorKind::Config, fact)
    }

    pub const fn budget(fact: &'static str) -> Self {
        Self::new(DelimitedErrorKind::Budget, fact)
    }

    pub const fn protocol(fact: &'static str) -> Self {
        Self::new(DelimitedErrorKind::Protocol, fact)
    }

    pub const fn encoding(fact: &'static str) -> Self {
        Self::new(DelimitedErrorKind::Encoding, fact)
    }

    pub const fn hash(fact: &'static str) -> Self {
        Self::new(DelimitedErrorKind::Hash, fact)
    }

    pub const fn cancelled(fact: &'static str) -> Self {
        Self::new(DelimitedErrorKind::Cancelled, fact)
    }

    pub const fn io(fact: &'static str) -> Self {
        Self::new(DelimitedErrorKind::Io, fact)
    }

    pub const fn kind(self) -> DelimitedErrorKind {
        self.kind
    }

    pub const fn fact(self) -> &'static str {
        self.fact
    }
}

impl std::fmt::Display for DelimitedError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "delimited error: {} fact={}",
            kind_name(self.kind),
            self.fact
        )
    }
}

impl std::error::Error for DelimitedError {}

fn kind_name(kind: DelimitedErrorKind) -> &'static str {
    match kind {
        DelimitedErrorKind::Config => "config",
        DelimitedErrorKind::Budget => "budget",
        DelimitedErrorKind::Protocol => "protocol",
        DelimitedErrorKind::Encoding => "encoding",
        DelimitedErrorKind::Hash => "hash",
        DelimitedErrorKind::Cancelled => "cancelled",
        DelimitedErrorKind::Io => "io",
    }
}

/// Explicit separator. Anything else is [`DelimitedDialect::try_from_separator`]'s
/// `ambiguous_dialect` error. The reader never chooses between these two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelimitedDialect {
    /// Comma separator. The quote is `"`.
    Csv,
    /// Tab separator. The quote is `"`.
    Tsv,
}

impl DelimitedDialect {
    pub const fn separator(self) -> u8 {
        match self {
            Self::Csv => b',',
            Self::Tsv => b'\t',
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Tsv => "tsv",
        }
    }

    /// Accept only comma or tab. Every other byte, including semicolon, pipe,
    /// space, quote, and newline, is an ambiguous dialect.
    pub const fn try_from_separator(byte: u8) -> Result<Self, DelimitedError> {
        match byte {
            b',' => Ok(Self::Csv),
            b'\t' => Ok(Self::Tsv),
            _ => Err(DelimitedError::config("ambiguous_dialect")),
        }
    }
}

/// What to do with a leading UTF-8 BOM (`EF BB BF`). There is no sniff mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BomPolicy {
    /// A leading BOM is `bom_refused`.
    Refuse,
    /// A leading BOM is consumed and is not part of the first field. Absence is allowed.
    Strip,
    /// The stream must start with a BOM. Absence is `bom_required`.
    Require,
}

impl BomPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Refuse => "refuse",
            Self::Strip => "strip",
            Self::Require => "require",
        }
    }
}

/// Whether the first record is a header. Names are not invented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderPolicy {
    /// Every record is data, in profile order.
    Absent,
    /// The first record's decoded fields must equal the profile names, in order.
    Exact,
}

impl HeaderPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Exact => "exact",
        }
    }
}

/// How an unquoted field becomes Arrow null. A quoted field is never null.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NullPolicy {
    /// An unquoted empty field is null. A quoted empty field is an empty string.
    Empty,
    /// An unquoted field whose exact text is the token is null. Empty is empty.
    Token(String),
}

/// Column types this slice will project. There is no float.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelimitedType {
    /// The decoded field text. Empty text is a value, not a null, unless the null policy says so.
    Utf8,
    /// The exact bytes `true` or `false`.
    Bool,
    /// An optional `-` and digits, with no leading zero, that fit in `i64`.
    Int64,
    /// An exact decimal rescaled onto this Arrow `decimal128` precision and scale.
    Decimal { precision: u8, scale: i8 },
}

impl DelimitedType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf8",
            Self::Bool => "bool",
            Self::Int64 => "int64",
            Self::Decimal { .. } => "decimal",
        }
    }

    pub const fn precision(self) -> Option<u8> {
        match self {
            Self::Decimal { precision, .. } => Some(precision),
            _ => None,
        }
    }

    pub const fn scale(self) -> Option<i8> {
        match self {
            Self::Decimal { scale, .. } => Some(scale),
            _ => None,
        }
    }
}

/// One selected column. Position in the profile is the field position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelimitedColumn {
    name: String,
    ty: DelimitedType,
    nullable: bool,
}

impl DelimitedColumn {
    /// Reject an empty name, surrounding whitespace, ASCII controls, names past 256 bytes,
    /// and a decimal precision or scale Arrow decimal128 cannot store.
    pub fn try_new(name: &str, ty: DelimitedType, nullable: bool) -> Result<Self, DelimitedError> {
        if !valid_label(name) {
            return Err(DelimitedError::config("column_name"));
        }
        if let DelimitedType::Decimal { precision, scale } = ty {
            validate_decimal_params(precision, scale)?;
        }
        Ok(Self {
            name: name.to_owned(),
            ty,
            nullable,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn ty(&self) -> DelimitedType {
        self.ty
    }

    pub fn nullable(&self) -> bool {
        self.nullable
    }
}

fn validate_decimal_params(precision: u8, scale: i8) -> Result<(), DelimitedError> {
    if !(1..=38).contains(&precision) {
        return Err(DelimitedError::config("decimal_precision"));
    }
    if !(-38..=38).contains(&scale) {
        return Err(DelimitedError::config("decimal_scale"));
    }
    // A positive scale wider than precision cannot be an Arrow decimal128.
    // A negative scale has no fractional width, so it stays allowed.
    if let Ok(width) = u8::try_from(scale)
        && width > precision
    {
        return Err(DelimitedError::config("decimal_scale"));
    }
    Ok(())
}

/// Immutable projection. Column order is the Arrow order and the field order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelimitedProfile {
    id: String,
    dialect: DelimitedDialect,
    bom: BomPolicy,
    header: HeaderPolicy,
    nulls: NullPolicy,
    columns: Vec<DelimitedColumn>,
}

impl DelimitedProfile {
    /// Reject an empty selection, a duplicate name, a bad id, and a null token
    /// that contains the separator, a quote, or a non-printable byte.
    /// The profile has no PII or semantic-class field and does not infer one.
    pub fn try_new(
        id: &str,
        dialect: DelimitedDialect,
        bom: BomPolicy,
        header: HeaderPolicy,
        nulls: NullPolicy,
        columns: Vec<DelimitedColumn>,
    ) -> Result<Self, DelimitedError> {
        if !valid_label(id) {
            return Err(DelimitedError::config("profile_id"));
        }
        if columns.is_empty() {
            return Err(DelimitedError::config("empty_selection"));
        }
        if columns.len() > DELIMITED_MAX_COLUMNS as usize {
            return Err(DelimitedError::config("column_count"));
        }
        if let NullPolicy::Token(token) = &nulls
            && !valid_token(token, dialect)
        {
            return Err(DelimitedError::config("null_token"));
        }
        for (index, column) in columns.iter().enumerate() {
            if columns[..index].iter().any(|other| other.name == column.name) {
                return Err(DelimitedError::config("duplicate_column"));
            }
        }
        Ok(Self {
            id: id.to_owned(),
            dialect,
            bom,
            header,
            nulls,
            columns,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn dialect(&self) -> DelimitedDialect {
        self.dialect
    }

    pub fn bom(&self) -> BomPolicy {
        self.bom
    }

    pub fn header(&self) -> HeaderPolicy {
        self.header
    }

    pub fn nulls(&self) -> &NullPolicy {
        &self.nulls
    }

    pub fn columns(&self) -> &[DelimitedColumn] {
        &self.columns
    }

    /// Exact profile preimage. Budgets and the object hash are not included.
    pub fn profile_preimage(&self) -> String {
        let mut body = String::from(PROFILE_PREFIX);
        body.push_str("id=");
        body.push_str(&self.id);
        body.push('\n');
        body.push_str("dialect=");
        body.push_str(self.dialect.as_str());
        body.push('\n');
        body.push_str("bom=");
        body.push_str(self.bom.as_str());
        body.push('\n');
        body.push_str("header=");
        body.push_str(self.header.as_str());
        body.push('\n');
        match &self.nulls {
            NullPolicy::Empty => body.push_str("null=empty\n"),
            NullPolicy::Token(token) => {
                body.push_str("null=token\nnull_token=");
                body.push_str(token);
                body.push('\n');
            }
        }
        body.push_str("quote=double\nrecord=lf-or-crlf\nescape=double-quote\n");
        for column in &self.columns {
            body.push_str("column=");
            body.push_str(&column.name);
            body.push('\t');
            body.push_str(column.ty.as_str());
            body.push('\t');
            body.push(if column.nullable { '1' } else { '0' });
            body.push('\t');
            match column.ty.precision() {
                Some(precision) => body.push_str(&precision.to_string()),
                None => body.push('-'),
            }
            body.push('\t');
            match column.ty.scale() {
                Some(scale) => body.push_str(&scale.to_string()),
                None => body.push('-'),
            }
            body.push('\n');
        }
        body
    }

    /// SHA-256 of [`Self::profile_preimage`].
    pub fn profile_digest(&self) -> [u8; 32] {
        sha256(self.profile_preimage().as_bytes())
    }

    /// Profile preimage plus the expected object hash. Pull budgets are not included.
    pub fn config_preimage(&self, expected_sha256: [u8; 32]) -> String {
        let mut body = String::from(CONFIG_PREFIX);
        body.push_str(&self.profile_preimage());
        body.push_str("expected_sha256=");
        body.push_str(&hex_encode(&expected_sha256));
        body.push('\n');
        body
    }

    /// SHA-256 of [`Self::config_preimage`].
    pub fn config_identity(&self, expected_sha256: [u8; 32]) -> [u8; 32] {
        sha256(self.config_preimage(expected_sha256).as_bytes())
    }
}

fn valid_label(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= DELIMITED_MAX_NAME_BYTES
        && text.trim() == text
        && bytes.iter().all(|byte| (0x20..0x7f).contains(byte) || *byte >= 0x80)
        && !text.chars().any(|ch| ch.is_whitespace() && ch != ' ')
}

fn valid_token(token: &str, dialect: DelimitedDialect) -> bool {
    let bytes = token.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= DELIMITED_MAX_NULL_TOKEN_BYTES
        && bytes.iter().all(|byte| {
            (0x21..0x7f).contains(byte) && *byte != b'"' && *byte != dialect.separator()
        })
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn hex_encode(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn read_into(inner: &mut impl Read, buf: &mut [u8]) -> Result<usize, DelimitedError> {
    loop {
        match inner.read(buf) {
            Ok(read) => return Ok(read),
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err(DelimitedError::io("read")),
        }
    }
}

fn is_control(byte: u8) -> bool {
    matches!(byte, 0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f)
}

/// Caps for one pull. `queue_batches` is 1 and is not a parameter.
/// These fields are not part of [`DelimitedProfile::config_identity`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelimitedBudget {
    batch_rows: u32,
    batch_bytes: u64,
    max_record_bytes: u32,
    max_field_bytes: u32,
    max_total_bytes: u64,
    max_rows: u64,
    max_columns: u32,
    max_fields: u32,
    deadline: Instant,
}

impl DelimitedBudget {
    /// Reject a zero cap, a record window below 3 bytes, and any cap above the hard ceiling.
    /// `max_field_bytes` must fit in one record. `max_total_bytes` must hold one record.
    /// The arguments stay explicit so a caller cannot omit a cap.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        batch_rows: u32,
        batch_bytes: u64,
        max_record_bytes: u32,
        max_field_bytes: u32,
        max_total_bytes: u64,
        max_rows: u64,
        max_columns: u32,
        max_fields: u32,
        deadline: Instant,
    ) -> Result<Self, DelimitedError> {
        if batch_rows == 0 || batch_rows > DELIMITED_MAX_BATCH_ROWS {
            return Err(DelimitedError::config("batch_rows"));
        }
        if batch_bytes == 0 || batch_bytes > DELIMITED_MAX_BATCH_BYTES {
            return Err(DelimitedError::config("batch_bytes"));
        }
        if !(DELIMITED_MIN_RECORD_BYTES..=DELIMITED_MAX_RECORD_BYTES).contains(&max_record_bytes) {
            return Err(DelimitedError::config("max_record"));
        }
        if max_field_bytes == 0
            || max_field_bytes > DELIMITED_MAX_FIELD_BYTES
            || max_field_bytes > max_record_bytes
        {
            return Err(DelimitedError::config("max_field"));
        }
        if max_total_bytes < u64::from(max_record_bytes) || max_total_bytes > DELIMITED_MAX_TOTAL_BYTES
        {
            return Err(DelimitedError::config("max_total"));
        }
        if max_rows == 0 || max_rows > DELIMITED_MAX_ROWS {
            return Err(DelimitedError::config("max_rows"));
        }
        if max_columns == 0 || max_columns > DELIMITED_MAX_COLUMNS {
            return Err(DelimitedError::config("max_columns"));
        }
        if max_fields == 0 || max_fields > DELIMITED_MAX_FIELDS {
            return Err(DelimitedError::config("max_fields"));
        }
        Ok(Self {
            batch_rows,
            batch_bytes,
            max_record_bytes,
            max_field_bytes,
            max_total_bytes,
            max_rows,
            max_columns,
            max_fields,
            deadline,
        })
    }

    pub fn batch_rows(self) -> u32 {
        self.batch_rows
    }

    pub fn batch_bytes(self) -> u64 {
        self.batch_bytes
    }

    pub fn max_record_bytes(self) -> u32 {
        self.max_record_bytes
    }

    pub fn max_field_bytes(self) -> u32 {
        self.max_field_bytes
    }

    pub fn max_total_bytes(self) -> u64 {
        self.max_total_bytes
    }

    pub fn max_rows(self) -> u64 {
        self.max_rows
    }

    pub fn max_columns(self) -> u32 {
        self.max_columns
    }

    pub fn max_fields(self) -> u32 {
        self.max_fields
    }

    pub fn deadline(self) -> Instant {
        self.deadline
    }

    /// The pull retains one batch. This is not configurable.
    pub fn queue_batches(self) -> u32 {
        1
    }
}

/// Versioned description of the profile. Import and publication are false.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelimitedHeader {
    pub protocol: &'static str,
    pub profile_version: &'static str,
    pub lineage_version: &'static str,
    pub receipt_version: &'static str,
    pub profile_id: String,
    pub profile_digest: [u8; 32],
    pub config_identity: [u8; 32],
    pub dialect: DelimitedDialect,
    pub columns: Vec<String>,
    pub types: Vec<DelimitedType>,
    pub nullability: Vec<bool>,
    /// Always false. A pure parse does not admit a database load.
    pub admits_import: bool,
    /// Always false. A pure parse does not publish a table.
    pub published: bool,
    /// Always false. This slice builds a `RecordBatch` and does not write Arrow IPC.
    pub arrow_ipc: bool,
    /// Always false. The parser does not execute a native payload.
    pub executes_native: bool,
}

/// How a profile column was present on one record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldPresence {
    /// A decoded value other than an empty string.
    Value,
    /// The null policy matched an unquoted field on a nullable column.
    Null,
    /// A present empty string. This is not null.
    Empty,
}

/// One selected field on one record. `name` is the profile name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldLineage {
    pub name: String,
    pub line: u64,
    pub byte_offset: u64,
    pub presence: FieldPresence,
}

/// Lineage for one emitted data row. The header is not a data row.
/// `line` is the 1-based line of the record's first byte. `byte_offset` counts
/// from the start of the stream, including a stripped BOM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowLineage {
    pub row_index: u64,
    pub line: u64,
    pub byte_offset: u64,
    pub fields: Vec<FieldLineage>,
}

/// One demanded batch. `resumable` is false. This is not [`DelimitedComplete`].
#[allow(exported_private_dependencies)]
#[derive(Debug)]
pub struct DelimitedBatch {
    pub batch_index: u64,
    pub record_batch: RecordBatch,
    pub lineage: Vec<RowLineage>,
    pub raw_bytes: u64,
    pub arrow_bytes: u64,
    pub header: DelimitedHeader,
    pub resumable: bool,
}

/// The pull has no further record. This is not [`DelimitedComplete`].
/// There is no boolean EOF admission on this value.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DelimitedExhausted {
    pub rows: u64,
    pub batches: u64,
    pub object_sha256: [u8; 32],
    seal: ExhaustSeal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExhaustSeal;

/// One answer from [`DelimitedReader::next_batch`].
#[allow(exported_private_dependencies)]
#[derive(Debug)]
pub enum DelimitedItem {
    /// Boxed so the exhaustion variant is not the size of a record batch.
    Batch(Box<DelimitedBatch>),
    Exhausted(DelimitedExhausted),
}

/// Proof that the object was read to a clean EOF, the hash matched, and every
/// emitted row covered the profile. Callers cannot construct it. `admits_import`
/// and `published` stay false. This is not a PostgreSQL `RunComplete`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DelimitedComplete {
    pub protocol: &'static str,
    pub profile_version: &'static str,
    pub lineage_version: &'static str,
    pub receipt_version: &'static str,
    pub profile_id: String,
    pub profile_digest: [u8; 32],
    pub config_identity: [u8; 32],
    pub object_sha256: [u8; 32],
    pub rows: u64,
    pub columns: u32,
    pub batches: u64,
    pub admits_import: bool,
    pub published: bool,
    seal: CompleteSeal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompleteSeal;

/// The owned reader was released without a completion token.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DelimitedCancelled {
    pub profile_id: String,
    pub rows_emitted: u64,
    pub released: bool,
    seal: CancelSeal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CancelSeal;

/// Caller handle. [`DelimitedCancel::request`] only sets the flag.
#[derive(Clone, Debug)]
pub struct DelimitedCancel {
    flag: Arc<AtomicBool>,
}

impl DelimitedCancel {
    pub fn request(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }
}

/// Pull-driven CSV or TSV reader over any [`Read`].
///
/// The reader owns `inner` and drops it on [`DelimitedReader::finish`],
/// [`DelimitedReader::cancel`], and `Drop`. Bytes move through `Read::read`
/// into one fixed window. The object hash is updated from those raw bytes
/// before they are parsed, and it is compared to `expected_sha256` only after
/// a real EOF at a record boundary.
#[allow(clippy::struct_excessive_bools)]
pub struct DelimitedReader<R: Read> {
    inner: R,
    profile: DelimitedProfile,
    expected: [u8; 32],
    budget: DelimitedBudget,
    window: Vec<u8>,
    base: u64,
    pos: usize,
    len: usize,
    eof: bool,
    hasher: Sha256,
    total: u64,
    line: u64,
    rows: u64,
    batches: u64,
    hash_ok: bool,
    object_sha256: [u8; 32],
    exhausted_delivered: bool,
    failed: Option<DelimitedError>,
    cancel: Arc<AtomicBool>,
    bom_done: bool,
    header_done: bool,
    names_covered: bool,
    keep_abs: u64,
}

impl<R: Read> std::fmt::Debug for DelimitedReader<R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DelimitedReader")
            .field("profile_id", &self.profile.id)
            .field("rows", &self.rows)
            .field("batches", &self.batches)
            .field("exhausted", &self.exhausted_delivered)
            .finish()
    }
}

impl<R: Read> Drop for DelimitedReader<R> {
    fn drop(&mut self) {
        self.window.zeroize();
    }
}

impl<R: Read> DelimitedReader<R> {
    /// Check the budget, the profile width, and the deadline, then allocate
    /// the single record window. No bytes are read here.
    pub fn open(
        inner: R,
        profile: DelimitedProfile,
        expected_sha256: [u8; 32],
        budget: DelimitedBudget,
    ) -> Result<Self, DelimitedError> {
        if profile.columns.len() > budget.max_columns as usize {
            return Err(DelimitedError::config("max_columns"));
        }
        if u32::try_from(profile.columns.len()).unwrap_or(u32::MAX) > budget.max_fields {
            return Err(DelimitedError::config("max_fields"));
        }
        if Instant::now() >= budget.deadline {
            return Err(DelimitedError::budget("deadline"));
        }
        let covered = matches!(profile.header, HeaderPolicy::Absent);
        let window = vec![0u8; budget.max_record_bytes as usize];
        Ok(Self {
            inner,
            profile,
            expected: expected_sha256,
            budget,
            window,
            base: 0,
            pos: 0,
            len: 0,
            eof: false,
            hasher: Sha256::new(),
            total: 0,
            line: 1,
            rows: 0,
            batches: 0,
            hash_ok: false,
            object_sha256: [0; 32],
            exhausted_delivered: false,
            failed: None,
            cancel: Arc::new(AtomicBool::new(false)),
            bom_done: false,
            header_done: covered,
            names_covered: covered,
            keep_abs: 0,
        })
    }

    pub fn header(&self) -> DelimitedHeader {
        DelimitedHeader {
            protocol: DELIMITED_PROTOCOL,
            profile_version: DELIMITED_PROFILE_VERSION,
            lineage_version: DELIMITED_LINEAGE_VERSION,
            receipt_version: DELIMITED_RECEIPT_VERSION,
            profile_id: self.profile.id.clone(),
            profile_digest: self.profile.profile_digest(),
            config_identity: self.profile.config_identity(self.expected),
            dialect: self.profile.dialect,
            columns: self.profile.columns.iter().map(|column| column.name.clone()).collect(),
            types: self.profile.columns.iter().map(|column| column.ty).collect(),
            nullability: self.profile.columns.iter().map(|column| column.nullable).collect(),
            admits_import: false,
            published: false,
            arrow_ipc: false,
            executes_native: false,
        }
    }

    pub fn cancel_handle(&self) -> DelimitedCancel {
        DelimitedCancel {
            flag: Arc::clone(&self.cancel),
        }
    }

    /// Demand the next batch, or [`DelimitedItem::Exhausted`] after a clean EOF.
    /// Exhaustion is not a completion token. A hash mismatch is returned only
    /// once EOF has been observed at a record boundary.
    pub fn next_batch(&mut self) -> Result<DelimitedItem, DelimitedError> {
        if let Some(err) = self.failed {
            return Err(err);
        }
        if self.exhausted_delivered {
            return Ok(self.exhausted_item());
        }
        self.guard()?;
        self.prepare_bom()?;
        let mut pending: Option<Pending> = None;
        loop {
            self.guard()?;
            self.keep_abs = self.current_abs();
            if self.peek()?.is_none() {
                return self.finish_stream(pending);
            }
            if pending.is_none() && self.rows >= self.budget.max_rows {
                return Err(self.fail(DelimitedError::budget("max_rows")));
            }
            let mark = self.mark();
            self.keep_abs = mark.abs;
            let record = self.parse_record()?;
            if !self.header_done {
                self.accept_header(&record)?;
                continue;
            }
            let typed = self.type_record(record)?;
            if let Some(item) = self.consider(&mut pending, mark, typed)? {
                return Ok(item);
            }
        }
    }

    /// Mint [`DelimitedComplete`] only after [`DelimitedItem::Exhausted`] was
    /// delivered, the profile columns were covered, and the hash matched.
    /// A partial pull, a cancel, and a hash mismatch return an error and no token.
    pub fn finish(self) -> Result<DelimitedComplete, DelimitedError> {
        if let Some(err) = self.failed {
            return Err(err);
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(DelimitedError::cancelled("cancelled"));
        }
        if !self.exhausted_delivered
            || !self.hash_ok
            || !self.names_covered
            || self.object_sha256 != self.expected
        {
            return Err(DelimitedError::protocol("not_exhausted"));
        }
        let columns = u32::try_from(self.profile.columns.len())
            .map_err(|_| DelimitedError::protocol("column_count"))?;
        Ok(DelimitedComplete {
            protocol: DELIMITED_PROTOCOL,
            profile_version: DELIMITED_PROFILE_VERSION,
            lineage_version: DELIMITED_LINEAGE_VERSION,
            receipt_version: DELIMITED_RECEIPT_VERSION,
            profile_id: self.profile.id.clone(),
            profile_digest: self.profile.profile_digest(),
            config_identity: self.profile.config_identity(self.expected),
            object_sha256: self.object_sha256,
            rows: self.rows,
            columns,
            batches: self.batches,
            admits_import: false,
            published: false,
            seal: CompleteSeal,
        })
    }

    /// Release the reader. This does not mint [`DelimitedComplete`].
    pub fn cancel(self) -> DelimitedCancelled {
        self.cancel.store(true, Ordering::Relaxed);
        DelimitedCancelled {
            profile_id: self.profile.id.clone(),
            rows_emitted: self.rows,
            released: true,
            seal: CancelSeal,
        }
    }

    fn accept_header(&mut self, record: &RawRecord) -> Result<(), DelimitedError> {
        if record.fields.len() != self.profile.columns.len() {
            return Err(self.fail(DelimitedError::protocol("width_mismatch")));
        }
        for (field, column) in record.fields.iter().zip(self.profile.columns.iter()) {
            if field.text != column.name {
                return Err(self.fail(DelimitedError::protocol("header_mismatch")));
            }
        }
        self.header_done = true;
        self.names_covered = true;
        Ok(())
    }

    fn consider(
        &mut self,
        pending: &mut Option<Pending>,
        mark: Mark,
        typed: TypedRecord,
    ) -> Result<Option<DelimitedItem>, DelimitedError> {
        let pending_rows = pending.as_ref().map(|batch| batch.rows).unwrap_or(0);
        let pending_bytes = pending.as_ref().map(|batch| batch.bytes).unwrap_or(0);
        let accepted = self.rows + pending_rows;
        let overflow_rows = pending_rows + 1 > u64::from(self.budget.batch_rows);
        let overflow_bytes = pending_bytes.saturating_add(typed.raw_len) > self.budget.batch_bytes;
        let overflow_total = accepted + 1 > self.budget.max_rows;
        if pending_rows == 0 && overflow_bytes {
            return Err(self.fail(DelimitedError::budget("batch_bytes")));
        }
        if pending_rows == 0 && overflow_total {
            return Err(self.fail(DelimitedError::budget("max_rows")));
        }
        if pending_rows > 0 && (overflow_rows || overflow_bytes || overflow_total) {
            self.rewind(mark)?;
            let Some(batch) = pending.take() else {
                return Err(self.fail(DelimitedError::protocol("row_width")));
            };
            return Ok(Some(self.emit(batch)?));
        }
        if pending.is_none() {
            *pending = Some(self.begin_pending());
        }
        let Some(slot) = pending.as_mut() else {
            return Err(self.fail(DelimitedError::protocol("row_width")));
        };
        self.push_row(slot, typed)?;
        if slot.rows == u64::from(self.budget.batch_rows) {
            let Some(batch) = pending.take() else {
                return Err(self.fail(DelimitedError::protocol("row_width")));
            };
            return Ok(Some(self.emit(batch)?));
        }
        Ok(None)
    }

    fn finish_stream(&mut self, pending: Option<Pending>) -> Result<DelimitedItem, DelimitedError> {
        if !self.header_done {
            return Err(self.fail(DelimitedError::protocol("header_required")));
        }
        if !self.names_covered {
            return Err(self.fail(DelimitedError::protocol("columns_uncovered")));
        }
        let hash_err = self.note_hash().err();
        if let Some(batch) = pending
            && batch.rows > 0
        {
            if let Some(err) = hash_err {
                self.failed = Some(err);
            }
            return self.emit(batch);
        }
        if let Some(err) = self.failed {
            return Err(err);
        }
        if let Some(err) = hash_err {
            return Err(err);
        }
        self.exhausted_delivered = true;
        Ok(self.exhausted_item())
    }

    fn exhausted_item(&self) -> DelimitedItem {
        DelimitedItem::Exhausted(DelimitedExhausted {
            rows: self.rows,
            batches: self.batches,
            object_sha256: self.object_sha256,
            seal: ExhaustSeal,
        })
    }

    fn note_hash(&mut self) -> Result<(), DelimitedError> {
        if !self.eof {
            return Err(self.fail(DelimitedError::protocol("truncated")));
        }
        if self.hash_ok {
            return Ok(());
        }
        if let Some(err) = self.failed {
            return Err(err);
        }
        let digest: [u8; 32] = self.hasher.clone().finalize().into();
        self.object_sha256 = digest;
        if digest != self.expected {
            return Err(self.fail(DelimitedError::hash("hash_mismatch")));
        }
        self.hash_ok = true;
        Ok(())
    }

    fn begin_pending(&self) -> Pending {
        let capacity = self.budget.batch_rows as usize;
        Pending {
            cols: self
                .profile
                .columns
                .iter()
                .map(|column| ColBuf::empty(column.ty, capacity))
                .collect(),
            lineage: Vec::with_capacity(capacity),
            bytes: 0,
            rows: 0,
        }
    }

    fn push_row(&mut self, pending: &mut Pending, row: TypedRecord) -> Result<(), DelimitedError> {
        if row.vals.len() != pending.cols.len() || row.fields.len() != pending.cols.len() {
            return Err(self.fail(DelimitedError::protocol("row_width")));
        }
        for (buf, value) in pending.cols.iter_mut().zip(row.vals) {
            push_cell(buf, value).map_err(|err| self.fail(err))?;
        }
        let row_index = self.rows + pending.rows;
        pending.lineage.push(RowLineage {
            row_index,
            line: row.line,
            byte_offset: row.offset,
            fields: row.fields,
        });
        pending.bytes = pending.bytes.saturating_add(row.raw_len);
        pending.rows += 1;
        Ok(())
    }

    fn emit(&mut self, pending: Pending) -> Result<DelimitedItem, DelimitedError> {
        let record_batch = match self.build_batch(pending.cols) {
            Ok(batch) => batch,
            Err(err) => return Err(self.fail(err)),
        };
        let arrow_bytes = record_batch
            .columns()
            .iter()
            .map(|column| column.get_array_memory_size() as u64)
            .sum();
        let batch = DelimitedBatch {
            batch_index: self.batches,
            record_batch,
            lineage: pending.lineage,
            raw_bytes: pending.bytes,
            arrow_bytes,
            header: self.header(),
            resumable: false,
        };
        self.rows += pending.rows;
        self.batches += 1;
        Ok(DelimitedItem::Batch(Box::new(batch)))
    }

    fn build_batch(&self, cols: Vec<ColBuf>) -> Result<RecordBatch, DelimitedError> {
        let mut fields = Vec::with_capacity(cols.len());
        let mut arrays: Vec<ArrayRef> = Vec::with_capacity(cols.len());
        for (column, buf) in self.profile.columns.iter().zip(cols) {
            let (data_type, array, logical) = match buf {
                ColBuf::Utf8(values) => (
                    DataType::Utf8,
                    Arc::new(StringArray::from(values)) as ArrayRef,
                    "utf8",
                ),
                ColBuf::Bool(values) => (
                    DataType::Boolean,
                    Arc::new(BooleanArray::from(values)) as ArrayRef,
                    "bool",
                ),
                ColBuf::Int(values) => (
                    DataType::Int64,
                    Arc::new(Int64Array::from(values)) as ArrayRef,
                    "int64",
                ),
                ColBuf::Decimal(values) => {
                    let DelimitedType::Decimal { precision, scale } = column.ty else {
                        return Err(DelimitedError::protocol("column_type"));
                    };
                    let array = Decimal128Array::from(values)
                        .with_precision_and_scale(precision, scale)
                        .map_err(|_| DelimitedError::protocol("arrow_decimal"))?;
                    (
                        DataType::Decimal128(precision, scale),
                        Arc::new(array) as ArrayRef,
                        "decimal",
                    )
                }
            };
            let mut metadata = HashMap::new();
            metadata.insert("oo.delimited.protocol".to_owned(), DELIMITED_PROTOCOL.to_owned());
            metadata.insert("oo.delimited.logical".to_owned(), logical.to_owned());
            if let DelimitedType::Decimal { precision, scale } = column.ty {
                metadata.insert("oo.delimited.precision".to_owned(), precision.to_string());
                metadata.insert("oo.delimited.scale".to_owned(), scale.to_string());
            }
            fields.push(Field::new(&column.name, data_type, column.nullable).with_metadata(metadata));
            arrays.push(array);
        }
        RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
            .map_err(|_| DelimitedError::protocol("arrow_batch"))
    }

    fn prepare_bom(&mut self) -> Result<(), DelimitedError> {
        if self.bom_done {
            return Ok(());
        }
        self.bom_done = true;
        self.keep_abs = 0;
        self.pull_at_least(BOM.len())?;
        let available = self.len.saturating_sub(self.pos);
        let is_bom = available >= BOM.len() && self.window[self.pos..self.pos + BOM.len()] == BOM;
        if is_bom {
            match self.profile.bom {
                BomPolicy::Refuse => {
                    return Err(self.fail(DelimitedError::encoding("bom_refused")));
                }
                BomPolicy::Strip | BomPolicy::Require => {
                    self.bump();
                    self.bump();
                    self.bump();
                }
            }
        } else if self.profile.bom == BomPolicy::Require {
            return Err(self.fail(DelimitedError::encoding("bom_required")));
        }
        Ok(())
    }

    fn pull_at_least(&mut self, needed: usize) -> Result<(), DelimitedError> {
        while self.len.saturating_sub(self.pos) < needed && !self.eof {
            let before = self.len;
            self.pull_bytes(self.keep_abs)?;
            if self.len == before {
                break;
            }
        }
        Ok(())
    }

    fn guard(&mut self) -> Result<(), DelimitedError> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(self.fail(DelimitedError::cancelled("cancelled")));
        }
        if Instant::now() >= self.budget.deadline {
            return Err(self.fail(DelimitedError::budget("deadline")));
        }
        Ok(())
    }

    fn fail(&mut self, err: DelimitedError) -> DelimitedError {
        if self.failed.is_none() {
            self.failed = Some(err);
        }
        self.failed.unwrap_or(err)
    }

    fn mark(&self) -> Mark {
        Mark {
            abs: self.current_abs(),
            line: self.line,
        }
    }

    fn current_abs(&self) -> u64 {
        self.base + self.pos as u64
    }

    fn rewind(&mut self, mark: Mark) -> Result<(), DelimitedError> {
        if mark.abs < self.base {
            return Err(self.fail(DelimitedError::protocol("offset")));
        }
        let index = usize::try_from(mark.abs - self.base)
            .map_err(|_| self.fail(DelimitedError::protocol("offset")))?;
        if index > self.len {
            return Err(self.fail(DelimitedError::protocol("offset")));
        }
        self.pos = index;
        self.line = mark.line;
        self.keep_abs = mark.abs;
        Ok(())
    }

    fn separator(&self) -> u8 {
        self.profile.dialect.separator()
    }

    fn peek(&mut self) -> Result<Option<u8>, DelimitedError> {
        if self.pos >= self.len && !self.eof {
            self.pull_bytes(self.keep_abs)?;
        }
        if self.pos >= self.len {
            if self.eof {
                return Ok(None);
            }
            return Err(self.fail(DelimitedError::protocol("read_stall")));
        }
        Ok(Some(self.window[self.pos]))
    }

    fn bump(&mut self) {
        let byte = self.window[self.pos];
        self.pos += 1;
        if byte == b'\n' {
            self.line += 1;
        }
    }

    fn pull_bytes(&mut self, keep_abs: u64) -> Result<(), DelimitedError> {
        self.guard()?;
        if keep_abs < self.base {
            return Err(self.fail(DelimitedError::protocol("offset")));
        }
        let keep = usize::try_from(keep_abs - self.base)
            .map_err(|_| self.fail(DelimitedError::protocol("offset")))?;
        if keep > self.pos || keep > self.len {
            return Err(self.fail(DelimitedError::protocol("offset")));
        }
        self.slide(keep);
        if self.eof {
            return Ok(());
        }
        if self.total >= self.budget.max_total_bytes {
            let mut probe = [0u8; 1];
            let read = self.read_some(&mut probe)?;
            if read == 0 {
                self.eof = true;
                return Ok(());
            }
            return Err(self.fail(DelimitedError::budget("max_total")));
        }
        if self.len == self.window.len() {
            return Err(self.fail(DelimitedError::budget("max_record")));
        }
        let room = self.window.len() - self.len;
        let remain = usize::try_from(self.budget.max_total_bytes - self.total).unwrap_or(room);
        let allow = room.min(remain);
        let start = self.len;
        let read = match read_into(&mut self.inner, &mut self.window[start..start + allow]) {
            Ok(read) => read,
            Err(err) => return Err(self.fail(err)),
        };
        if read == 0 {
            self.eof = true;
            return Ok(());
        }
        self.hasher.update(&self.window[start..start + read]);
        self.len += read;
        self.total += read as u64;
        Ok(())
    }

    fn slide(&mut self, keep_from: usize) {
        if keep_from == 0 {
            return;
        }
        self.window.copy_within(keep_from..self.len, 0);
        self.len -= keep_from;
        self.pos -= keep_from;
        self.base += keep_from as u64;
        self.window[self.len..].fill(0);
    }

    fn read_some(&mut self, buf: &mut [u8]) -> Result<usize, DelimitedError> {
        read_into(&mut self.inner, buf).map_err(|err| self.fail(err))
    }

    fn parse_record(&mut self) -> Result<RawRecord, DelimitedError> {
        let mark = self.mark();
        let mut fields = Vec::new();
        loop {
            self.guard()?;
            if u32::try_from(fields.len()).unwrap_or(u32::MAX) >= self.budget.max_fields {
                return Err(self.fail(DelimitedError::budget("max_fields")));
            }
            fields.push(self.parse_field()?);
            if self.at_record_end()? {
                break;
            }
        }
        let raw_len = self.current_abs().saturating_sub(mark.abs);
        if raw_len > u64::from(self.budget.max_record_bytes) {
            return Err(self.fail(DelimitedError::budget("max_record")));
        }
        Ok(RawRecord {
            fields,
            line: mark.line,
            offset: mark.abs,
            raw_len,
        })
    }

    fn at_record_end(&mut self) -> Result<bool, DelimitedError> {
        match self.peek()? {
            Some(byte) if byte == self.separator() => {
                self.bump();
                Ok(false)
            }
            Some(b'\n') => {
                self.bump();
                Ok(true)
            }
            Some(b'\r') => {
                self.bump();
                if self.peek()? != Some(b'\n') {
                    return Err(self.fail(DelimitedError::protocol("bare_cr")));
                }
                self.bump();
                Ok(true)
            }
            None => Ok(true),
            Some(_) => Err(self.fail(DelimitedError::protocol("malformed_quote"))),
        }
    }

    fn parse_field(&mut self) -> Result<RawField, DelimitedError> {
        let line = self.line;
        let offset = self.current_abs();
        match self.peek()? {
            Some(b'"') => self.parse_quoted(line, offset),
            Some(byte) if byte == self.separator() || byte == b'\n' || byte == b'\r' => {
                Ok(RawField::empty(line, offset))
            }
            None => Ok(RawField::empty(line, offset)),
            Some(_) => self.parse_unquoted(line, offset),
        }
    }

    fn parse_quoted(&mut self, line: u64, offset: u64) -> Result<RawField, DelimitedError> {
        self.bump();
        let mut raw = Vec::new();
        loop {
            self.guard()?;
            match self.peek()? {
                None => return Err(self.fail(DelimitedError::protocol("malformed_quote"))),
                Some(b'"') => {
                    self.bump();
                    if self.peek()? == Some(b'"') {
                        self.bump();
                        self.push_content(&mut raw, b'"')?;
                    } else {
                        return Ok(RawField {
                            text: self.finish_text(raw)?,
                            line,
                            offset,
                            quoted: true,
                        });
                    }
                }
                Some(byte) if is_control(byte) => {
                    return Err(self.fail(DelimitedError::protocol("control_char")));
                }
                Some(byte) => {
                    self.bump();
                    self.push_content(&mut raw, byte)?;
                }
            }
        }
    }

    fn parse_unquoted(&mut self, line: u64, offset: u64) -> Result<RawField, DelimitedError> {
        let mut raw = Vec::new();
        loop {
            match self.peek()? {
                None | Some(b'\n' | b'\r') => break,
                Some(byte) if byte == self.separator() => break,
                Some(b'"') => return Err(self.fail(DelimitedError::protocol("malformed_quote"))),
                Some(byte) if is_control(byte) => {
                    return Err(self.fail(DelimitedError::protocol("control_char")));
                }
                Some(byte) => {
                    self.bump();
                    self.push_content(&mut raw, byte)?;
                }
            }
        }
        Ok(RawField {
            text: self.finish_text(raw)?,
            line,
            offset,
            quoted: false,
        })
    }

    fn push_content(&mut self, buf: &mut Vec<u8>, byte: u8) -> Result<(), DelimitedError> {
        let max = self.budget.max_field_bytes as usize;
        if buf.len() >= max {
            return Err(self.fail(DelimitedError::budget("max_field")));
        }
        if buf.len() == buf.capacity() {
            let grow = (max - buf.len()).min(buf.capacity().max(32));
            buf.reserve_exact(grow);
        }
        buf.push(byte);
        Ok(())
    }

    fn finish_text(&mut self, raw: Vec<u8>) -> Result<String, DelimitedError> {
        String::from_utf8(raw).map_err(|_| self.fail(DelimitedError::encoding("utf8")))
    }

    fn type_record(&mut self, record: RawRecord) -> Result<TypedRecord, DelimitedError> {
        if record.fields.len() != self.profile.columns.len() {
            return Err(self.fail(DelimitedError::protocol("width_mismatch")));
        }
        let columns: Vec<(String, DelimitedType, bool)> = self
            .profile
            .columns
            .iter()
            .map(|column| (column.name.clone(), column.ty, column.nullable))
            .collect();
        let mut vals = Vec::with_capacity(record.fields.len());
        let mut fields = Vec::with_capacity(record.fields.len());
        for (field, (name, ty, nullable)) in record.fields.into_iter().zip(columns) {
            let line = field.line;
            let offset = field.offset;
            let (value, presence) = self.classify(field, ty, nullable)?;
            vals.push(value);
            fields.push(FieldLineage {
                name,
                line,
                byte_offset: offset,
                presence,
            });
        }
        Ok(TypedRecord {
            vals,
            fields,
            line: record.line,
            offset: record.offset,
            raw_len: record.raw_len,
        })
    }

    fn classify(
        &mut self,
        field: RawField,
        ty: DelimitedType,
        nullable: bool,
    ) -> Result<(Option<CellVal>, FieldPresence), DelimitedError> {
        if self.is_null(&field) {
            if !nullable {
                return Err(self.fail(DelimitedError::protocol("null_not_allowed")));
            }
            return Ok((None, FieldPresence::Null));
        }
        if field.text.is_empty() && !matches!(ty, DelimitedType::Utf8) {
            return Err(self.fail(DelimitedError::protocol("empty_field")));
        }
        match ty {
            DelimitedType::Utf8 => {
                let presence = if field.text.is_empty() {
                    FieldPresence::Empty
                } else {
                    FieldPresence::Value
                };
                Ok((Some(CellVal::Utf8(field.text)), presence))
            }
            DelimitedType::Bool => {
                let flag = parse_bool(&field.text).map_err(|err| self.fail(err))?;
                Ok((Some(CellVal::Bool(flag)), FieldPresence::Value))
            }
            DelimitedType::Int64 => {
                let number = parse_i64(&field.text).map_err(|err| self.fail(err))?;
                Ok((Some(CellVal::Int(number)), FieldPresence::Value))
            }
            DelimitedType::Decimal { precision, scale } => {
                let number = parse_decimal(&field.text, precision, scale).map_err(|err| self.fail(err))?;
                Ok((Some(CellVal::Decimal(number)), FieldPresence::Value))
            }
        }
    }

    fn is_null(&self, field: &RawField) -> bool {
        if field.quoted {
            return false;
        }
        match &self.profile.nulls {
            NullPolicy::Empty => field.text.is_empty(),
            NullPolicy::Token(token) => field.text == *token,
        }
    }
}

struct Mark {
    abs: u64,
    line: u64,
}

struct RawField {
    text: String,
    line: u64,
    offset: u64,
    quoted: bool,
}

impl RawField {
    fn empty(line: u64, offset: u64) -> Self {
        Self {
            text: String::new(),
            line,
            offset,
            quoted: false,
        }
    }
}

struct RawRecord {
    fields: Vec<RawField>,
    line: u64,
    offset: u64,
    raw_len: u64,
}

enum CellVal {
    Utf8(String),
    Bool(bool),
    Int(i64),
    Decimal(i128),
}

struct TypedRecord {
    vals: Vec<Option<CellVal>>,
    fields: Vec<FieldLineage>,
    line: u64,
    offset: u64,
    raw_len: u64,
}

enum ColBuf {
    Utf8(Vec<Option<String>>),
    Bool(Vec<Option<bool>>),
    Int(Vec<Option<i64>>),
    Decimal(Vec<Option<i128>>),
}

impl ColBuf {
    fn empty(ty: DelimitedType, capacity: usize) -> Self {
        match ty {
            DelimitedType::Utf8 => Self::Utf8(Vec::with_capacity(capacity)),
            DelimitedType::Bool => Self::Bool(Vec::with_capacity(capacity)),
            DelimitedType::Int64 => Self::Int(Vec::with_capacity(capacity)),
            DelimitedType::Decimal { .. } => Self::Decimal(Vec::with_capacity(capacity)),
        }
    }
}

struct Pending {
    cols: Vec<ColBuf>,
    lineage: Vec<RowLineage>,
    bytes: u64,
    rows: u64,
}

fn push_cell(buf: &mut ColBuf, value: Option<CellVal>) -> Result<(), DelimitedError> {
    match (buf, value) {
        (ColBuf::Utf8(values), Some(CellVal::Utf8(text))) => values.push(Some(text)),
        (ColBuf::Utf8(values), None) => values.push(None),
        (ColBuf::Bool(values), Some(CellVal::Bool(flag))) => values.push(Some(flag)),
        (ColBuf::Bool(values), None) => values.push(None),
        (ColBuf::Int(values), Some(CellVal::Int(number))) => values.push(Some(number)),
        (ColBuf::Int(values), None) => values.push(None),
        (ColBuf::Decimal(values), Some(CellVal::Decimal(number))) => values.push(Some(number)),
        (ColBuf::Decimal(values), None) => values.push(None),
        _ => return Err(DelimitedError::protocol("column_type")),
    }
    Ok(())
}

fn parse_bool(text: &str) -> Result<bool, DelimitedError> {
    match text {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(DelimitedError::protocol("not_bool")),
    }
}

fn parse_i64(text: &str) -> Result<i64, DelimitedError> {
    if !int_lexical(text) {
        return Err(DelimitedError::protocol("not_int64"));
    }
    let exact = match ExactDecimal::parse_sql(text) {
        Ok(value) => value,
        Err(DecimalError::Budget) => return Err(DelimitedError::budget("number_budget")),
        Err(_) => return Err(DelimitedError::protocol("not_int64")),
    };
    if exact.scale() != 0 {
        return Err(DelimitedError::protocol("not_int64"));
    }
    let wide = coeff_i128(&exact).map_err(|_| DelimitedError::protocol("int64_overflow"))?;
    i64::try_from(wide).map_err(|_| DelimitedError::protocol("int64_overflow"))
}

fn parse_decimal(text: &str, precision: u8, scale: i8) -> Result<i128, DelimitedError> {
    if !decimal_lexical(text) {
        return Err(DelimitedError::protocol("not_decimal"));
    }
    let exact = match ExactDecimal::parse_sql(text) {
        Ok(value) => value,
        Err(DecimalError::Budget) => return Err(DelimitedError::budget("number_budget")),
        Err(_) => return Err(DelimitedError::protocol("not_decimal")),
    };
    let scaled = match exact.rescale_exact(i32::from(scale)) {
        Ok(value) => value,
        Err(DecimalError::Inexact) => return Err(DelimitedError::protocol("inexact_decimal")),
        Err(DecimalError::Budget) => return Err(DelimitedError::budget("number_budget")),
        Err(_) => return Err(DelimitedError::protocol("not_decimal")),
    };
    if !scaled.fits_decimal128() {
        return Err(DelimitedError::protocol("decimal_storage"));
    }
    if scaled.coefficient().len() > usize::from(precision) {
        return Err(DelimitedError::protocol("decimal_precision"));
    }
    coeff_i128(&scaled).map_err(|_| DelimitedError::protocol("decimal_storage"))
}

fn int_lexical(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes[0] == b'+' {
        return false;
    }
    let digits = if bytes[0] == b'-' { &bytes[1..] } else { bytes };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return false;
    }
    digits.len() == 1 || digits[0] != b'0'
}

fn decimal_lexical(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes[0] == b'+' {
        return false;
    }
    let rest = if bytes[0] == b'-' { &bytes[1..] } else { bytes };
    if rest.is_empty() {
        return false;
    }
    let (int_part, frac) = match rest.iter().position(|byte| *byte == b'.') {
        Some(index) => (&rest[..index], Some(&rest[index + 1..])),
        None => (rest, None),
    };
    if int_part.is_empty() || !int_part.iter().all(u8::is_ascii_digit) {
        return false;
    }
    if int_part.len() > 1 && int_part[0] == b'0' {
        return false;
    }
    match frac {
        Some(frac) => !frac.is_empty() && frac.iter().all(u8::is_ascii_digit),
        None => true,
    }
}

fn coeff_i128(exact: &ExactDecimal) -> Result<i128, ()> {
    let mut value: i128 = 0;
    for byte in exact.coefficient().bytes() {
        value = value.checked_mul(10).ok_or(())?;
        value = value.checked_add(i128::from(byte - b'0')).ok_or(())?;
    }
    if exact.is_negative() {
        value = value.checked_neg().ok_or(())?;
    }
    Ok(value)
}
