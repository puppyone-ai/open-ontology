//! Lease-bound private C1 bootstrap staging. CDC transactions are applied
//! before snapshot fill; snapshot rows cannot replace CDC rows or tombstones.
use serde_json::{Value, json};
use std::path::PathBuf;

use sqlx::{Connection, PgConnection, Postgres, Transaction};

use crate::c1_parts::{PART_MAX_BYTES, PartEntry, PartKind};
use crate::native_c1_apply::{self, CopyRows, Tags};

const PROTOCOL: &str = "oo.pipeline.native-c1-baseline-stage/v1";
const MAX_REQUEST: usize = 32 * 1024 * 1024;

pub async fn run() -> Result<(), u8> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin().take((MAX_REQUEST + 1) as u64).read_to_end(&mut bytes).map_err(|_| 2u8)?;
    if bytes.is_empty() || bytes.len() > MAX_REQUEST { return refuse("request_size"); }
    let request: Value = serde_json::from_slice(&bytes).map_err(|_| 2u8)?;
    let result = match execute(&request).await {
        Ok(value) => value,
        Err(token) => json!({"protocol":PROTOCOL,"outcome":"refused","reason":token}),
    };
    let ok = !matches!(result["outcome"].as_str(), Some("refused"));
    let mut out = serde_json::to_vec(&result).map_err(|_| 1u8)?;
    out.push(b'\n');
    use std::io::Write;
    std::io::stdout().write_all(&out).map_err(|_| 1u8)?;
    if ok { Ok(()) } else { Err(1) }
}

async fn execute(request: &Value) -> Result<Value, &'static str> {
    if request["protocol"].as_str() != Some(PROTOCOL) { return Err("protocol"); }
    let operation = field(request, "operation")?;
    let source = field(request, "source_id")?;
    let task = field(request, "lease_id")?;
    let generation = integer(request, "lease_generation")?;
    let epoch = i32::try_from(integer(request, "source_epoch")?).map_err(|_| "source_epoch")?;
    let target = request.get("target").ok_or("target")?;
    let mut conn = crate::native_update::connect_target(target).await?;
    let result: String = match operation {
        "open" => sqlx::query_scalar("SELECT _oo.native_c1_baseline_open($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::numeric)::text")
            .bind(source).bind(task).bind(generation).bind(epoch)
            .bind(field(request,"measured_id")?).bind(digest(request,"config_digest")?)
            .bind(digest(request,"schema_digest")?).bind(field(request,"l0")?)
            .bind(decimal(request,"l0_pos")?).fetch_one(&mut conn).await.map_err(sql_token)?,
        "ack_seed" => sqlx::query_scalar("SELECT _oo.native_c1_ack_seed($1,$2::uuid,$3,$4,$5::uuid,$6)::text")
            .bind(source).bind(task).bind(generation).bind(epoch).bind(field(request,"previous_task")?)
            .bind(integer(request,"previous_source_generation")?).fetch_one(&mut conn).await.map_err(sql_token)?,
        "transaction" => sqlx::query_scalar("SELECT _oo.native_c1_baseline_apply_timed_txn($1,$2::uuid,$3,$4,$5::numeric,$6,$7::jsonb,$8,$9)::text")
            .bind(source).bind(task).bind(generation).bind(epoch)
            .bind(decimal(request,"applied_pos")?).bind(field(request,"txn_id")?)
            .bind(json_text(request,"rows")?).bind(digest(request,"payload_sha256")?)
            .bind(field(request,"source_commit_timestamp")?)
            .fetch_one(&mut conn).await.map_err(sql_token)?,
        "snapshot_fill" => sqlx::query_scalar("SELECT _oo.native_c1_baseline_fill($1,$2::uuid,$3,$4,$5::jsonb)::text")
            .bind(source).bind(task).bind(generation).bind(epoch).bind(json_text(request,"rows")?)
            .fetch_one(&mut conn).await.map_err(sql_token)?,
        "seal" => sqlx::query_scalar("SELECT _oo.native_c1_baseline_seal($1,$2::uuid,$3,$4,$5,$6::numeric)::text")
            .bind(source).bind(task).bind(generation).bind(epoch)
            .bind(field(request,"l_end")?).bind(decimal(request,"l_end_pos")?)
            .fetch_one(&mut conn).await.map_err(sql_token)?,
        _ => return Err("operation"),
    };
    let value: Value = serde_json::from_str(&result).map_err(|_| "target_receipt")?;
    Ok(value)
}

fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("request_field")
}
fn integer(v: &Value, key: &str) -> Result<i64, &'static str> { v.get(key).and_then(Value::as_i64).ok_or("request_field") }
fn digest<'a>(v: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    let s=field(v,key)?;
    if s.len()!=64 || !s.bytes().all(|b|b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) { return Err("digest"); }
    Ok(s)
}
fn decimal<'a>(v: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    let s=field(v,key)?;
    if s.len()>38 || !s.bytes().all(|b|b.is_ascii_digit()) || s.len()>1 && s.starts_with('0') { return Err("position"); }
    Ok(s)
}
fn json_text(v: &Value, key: &str) -> Result<String, &'static str> {
    serde_json::to_string(v.get(key).filter(|item| item.is_array()).ok_or("request_field")?).map_err(|_| "request_field")
}
fn sql_token(err: sqlx::Error) -> &'static str {
    let state=err.as_database_error().and_then(|e|e.code().map(|c|c.to_string())).unwrap_or_default();
    eprintln!("oo-pipeline: c1 baseline target sqlstate={state}");
    match state.as_str() { "OO010"=>"lease_or_barrier", "OO030"=>"closed", "42P01"=>"not_ready", "42501"=>"permission", _=>"target_apply" }
}
fn refuse(token: &'static str) -> Result<(),u8> {
    let out=json!({"protocol":PROTOCOL,"outcome":"refused","reason":token});
    let mut bytes=serde_json::to_vec(&out).map_err(|_|1u8)?;bytes.push(b'\n');
    use std::io::Write;std::io::stdout().write_all(&bytes).map_err(|_|1u8)?;Err(1)
}

// ---------------------------------------------------------------- chunked-typed/v2
//
// Stage operations for `chunked-typed/v2` (DESIGN §3.3–3.7, §4.3). v1 above is
// unchanged. A v2 request carries paths and entries, never rows, and is capped at
// `MAX_REQUEST_V2`. Each write runs in one transaction that takes the task lock
// first (R2). The fence (adopt, bind, release) is taken only by the operations
// that need it: open, rebind, seal and retire.

/// Stage protocol for `chunked-typed/v2`.
pub const PROTOCOL_V2: &str = "oo.pipeline.native-c1-baseline-stage/v2";
/// Largest v2 request. Parts travel by path, so a valid request is a few KiB.
pub const MAX_REQUEST_V2: usize = 1024 * 1024;
/// Default tombstone bound per table (N6). A request may set another bound.
pub const DEFAULT_MAX_TOMBSTONES: u64 = 50_000_000;
/// Tables in one task. Matches the v1 collection bound (migration 0036).
const MAX_TABLES: usize = 32;
const MAX_COLUMNS: usize = 1024;
const MAX_TXNS: u64 = 1000;
const MAX_BATCH_PARTS: usize = 32;
/// SHA-256 of zero bytes. A terminal chunk without a part carries this digest.
pub const EMPTY_PART_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const KEY_MIN: i128 = -(1 << 63);
const KEY_MAX: i128 = (1 << 64) - 1;

type Token = &'static str;

/// Connection and identity common to every v2 operation.
#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    /// `source_id`, the lowercase identifier of the source.
    pub source: String,
    /// `lease_id`, the jobs task id that holds the C1 lease.
    pub task: String,
    /// `lease_generation`.
    pub generation: i64,
    /// `source_epoch`.
    pub epoch: i32,
    /// Runtime connection object, as `connect_target` reads it.
    pub target: Value,
}

/// Target fence owner. Required by open, rebind, seal and retire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fence {
    /// Target shard.
    pub shard: String,
    /// Owner token, a uuid.
    pub owner: String,
}

/// One table as the open operation declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenTable {
    /// Logical schema.
    pub schema: String,
    /// Logical table.
    pub table: String,
    /// Digest of the table schema, stored with the table.
    pub table_schema_digest: String,
    /// Logical column names in stage order.
    pub columns: Vec<String>,
    /// PostgreSQL type oid of each column.
    pub oids: Vec<i64>,
    /// PostgreSQL typmod of each column.
    pub typmods: Vec<i32>,
    /// Not-null flag of each column.
    pub not_null: Vec<bool>,
    /// Logical key column. Must be one of `columns`.
    pub key: String,
}

/// Open request body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenBody {
    /// `mysql.<uuid>` measured identity.
    pub measured_id: String,
    /// Config digest.
    pub config_digest: String,
    /// Source schema digest.
    pub schema_digest: String,
    /// Target type digest.
    pub type_digest: String,
    /// Input digest.
    pub input_digest: String,
    /// `<uuid>:1-<n>` L0 position.
    pub l0: String,
    /// L0 position number.
    pub l0_pos: String,
    /// Source generation the mappings are registered under.
    pub source_generation: i64,
    /// Acknowledged predecessor whose source-instance lease this cut succeeds.
    pub previous_task: Option<String>,
    /// Tables of the task, in order.
    pub tables: Vec<OpenTable>,
}

/// A part file, as the request names it. The file name must be the canonical name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartRef {
    /// Absolute path. Its file name is `fill-NNNNNNNN.arrow` or `cdc-NNNNNNNN.arrow`.
    pub path: PathBuf,
    /// File size.
    pub bytes: u64,
    /// File SHA-256.
    pub sha256: String,
    /// Row count.
    pub rows: u64,
    /// Smallest key in the part, if it has rows.
    pub key_lo: Option<i128>,
    /// Largest key in the part, if it has rows.
    pub key_hi: Option<i128>,
}

/// Snapshot chunk request body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkBody {
    /// Logical schema.
    pub schema: String,
    /// Logical table.
    pub table: String,
    /// Chunk number, 1-based and contiguous.
    pub chunk_no: i32,
    /// Attempt, for diagnostics. Not part of replay identity.
    pub attempt: i32,
    /// Exclusive lower bound: the previous chunk's `key_hi`. Absent for the first chunk.
    pub key_lo: Option<i128>,
    /// Inclusive upper bound. Absent for the terminal chunk.
    pub key_hi: Option<i128>,
    /// True for the chunk that ends the table.
    pub terminal: bool,
    /// Row count. Equal to `part.rows`.
    pub row_count: u64,
    /// Digest of the part. Equal to `part.sha256`, or the empty-file digest when absent.
    pub part_sha256: String,
    /// Logical columns in stage order.
    pub columns: Vec<String>,
    /// Logical key column.
    pub key: String,
    /// The part. Absent only for a terminal chunk.
    pub part: Option<PartRef>,
}

/// One table's CDC part in a batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchPart {
    /// Logical schema.
    pub schema: String,
    /// Logical table.
    pub table: String,
    /// Logical columns in stage order.
    pub columns: Vec<String>,
    /// Logical key column.
    pub key: String,
    /// The part. Its rows are at least one.
    pub part: PartRef,
}

/// CDC batch request body.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchBody {
    /// Batch number, 1-based and contiguous.
    pub batch_no: i32,
    /// First position of the batch. Must equal the applied position plus one.
    pub first_pos: String,
    /// Last position of the batch.
    pub last_pos: String,
    /// Transactions in the batch. `last_pos - first_pos + 1` equals this.
    pub txns: i32,
    /// Total rows of all parts.
    pub row_count: u64,
    /// Batch digest, as the producer computed it. Replay compares it.
    pub batch_sha256: String,
    /// One record per transaction, as migration 0042 reads them.
    pub txn_records: Value,
    /// Parts of the batch, one per table with rows.
    pub parts: Vec<BatchPart>,
    /// Tombstone bound per table (N6).
    pub max_tombstones: u64,
}

/// One table of a seal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealTable {
    /// Logical schema.
    pub schema: String,
    /// Logical table.
    pub table: String,
    /// Expected row count. The SQL checks it against the shadow.
    pub row_count: u64,
    /// Composed digest of the table. The caller computes it (T8).
    pub payload_digest: String,
}

/// Seal request body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealBody {
    /// `<uuid>:1-<n>` L_end.
    pub l_end: String,
    /// L_end position number.
    pub l_end_pos: String,
    /// Hook digest recorded on the apply receipts.
    pub hook_digest: String,
    /// Tables of the task, each with its verified composed digest.
    pub tables: Vec<SealTable>,
}

/// Operation and its body.
#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    /// `open`.
    Open(OpenBody),
    /// `chunk`.
    Chunk(ChunkBody),
    /// `txn_batch`.
    TxnBatch(BatchBody),
    /// `rebind`, no body.
    Rebind,
    /// `seal`.
    Seal(SealBody),
    /// `retire`, no body. Abandons the run (N1).
    Retire,
}

/// One validated v2 request.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// Identity and connection.
    pub envelope: Envelope,
    /// Fence, for the operations that take it.
    pub fence: Option<Fence>,
    /// The operation.
    pub body: Body,
}

/// Refuse a request that is empty or over the cap, before it is parsed.
pub fn check_request_size(len: usize) -> Result<(), Token> {
    if len == 0 || len > MAX_REQUEST_V2 { Err("request_size") } else { Ok(()) }
}

/// Validate one v2 request. Every field is checked before any connection opens.
pub fn parse_request(value: &Value) -> Result<Request, Token> {
    if value.get("protocol").and_then(Value::as_str) != Some(PROTOCOL_V2) {
        return Err("protocol");
    }
    let envelope = Envelope {
        source: ident(value, "source_id")?,
        task: uuid(value, "lease_id")?,
        generation: bounded(value, "lease_generation", 1, i64::MAX)?,
        epoch: i32::try_from(bounded(value, "source_epoch", 1, i64::from(i32::MAX))?).map_err(|_| "source_epoch")?,
        target: value.get("target").filter(|item| item.is_object()).cloned().ok_or("target")?,
    };
    let operation = text(value, "operation")?;
    let (fence, body) = match operation {
        "open" => (Some(fence(value)?), Body::Open(open_body(value)?)),
        "chunk" => (None, Body::Chunk(chunk_body(value)?)),
        "txn_batch" => (None, Body::TxnBatch(batch_body(value)?)),
        "rebind" => (Some(fence(value)?), Body::Rebind),
        "seal" => (Some(fence(value)?), Body::Seal(seal_body(value)?)),
        "retire" => (Some(fence(value)?), Body::Retire),
        _ => return Err("operation"),
    };
    Ok(Request { envelope, fence, body })
}

fn fence(value: &Value) -> Result<Fence, Token> {
    let item = value.get("fence").filter(|item| item.is_object()).ok_or("fence")?;
    Ok(Fence { shard: text(item, "shard")?.to_owned(), owner: uuid(item, "owner")? })
}

fn open_body(value: &Value) -> Result<OpenBody, Token> {
    let mut tables = Vec::new();
    for item in array(value, "tables", 1, MAX_TABLES)? {
        let table = OpenTable {
            schema: text(item, "schema")?.to_owned(),
            table: text(item, "table")?.to_owned(),
            table_schema_digest: hex64(item, "table_schema_digest")?,
            columns: columns(item)?,
            oids: array(item, "oids", 1, MAX_COLUMNS)?.iter().map(|v| v.as_i64().ok_or("oids")).collect::<Result<_, _>>()?,
            typmods: array(item, "typmods", 1, MAX_COLUMNS)?
                .iter()
                .map(|v| v.as_i64().and_then(|n| i32::try_from(n).ok()).ok_or("typmods"))
                .collect::<Result<_, _>>()?,
            not_null: array(item, "not_null", 1, MAX_COLUMNS)?.iter().map(|v| v.as_bool().ok_or("not_null")).collect::<Result<_, _>>()?,
            key: text(item, "key")?.to_owned(),
        };
        let n = table.columns.len();
        if table.oids.len() != n || table.typmods.len() != n || table.not_null.len() != n || !table.columns.contains(&table.key) {
            return Err("open_shape");
        }
        if tables.iter().any(|old: &OpenTable| old.schema == table.schema && old.table == table.table) {
            return Err("duplicate_table");
        }
        tables.push(table);
    }
    let source_generation = bounded(value, "source_generation", 1, i64::MAX)?;
    let previous_task = match value.get("previous_task") {
        None | Some(Value::Null) => None,
        Some(_) if source_generation > 1 => Some(uuid(value, "previous_task")?),
        Some(_) => return Err("previous_generation"),
    };
    Ok(OpenBody {
        measured_id: text(value, "measured_id")?.to_owned(),
        config_digest: hex64(value, "config_digest")?,
        schema_digest: hex64(value, "schema_digest")?,
        type_digest: hex64(value, "type_digest")?,
        input_digest: hex64(value, "input_digest")?,
        l0: text(value, "l0")?.to_owned(),
        l0_pos: position_text(value, "l0_pos")?,
        source_generation,
        previous_task,
        tables,
    })
}

fn chunk_body(value: &Value) -> Result<ChunkBody, Token> {
    let terminal = value.get("terminal").and_then(Value::as_bool).ok_or("request_field")?;
    let key_lo = opt_key(value, "key_lo")?;
    let key_hi = opt_key(value, "key_hi")?;
    let row_count = bounded(value, "row_count", 0, i64::MAX)?.unsigned_abs();
    let part_sha256 = hex64(value, "part_sha256")?;
    let part = match value.get("part") {
        None | Some(Value::Null) => None,
        Some(item) => Some(part_ref(item)?),
    };
    let body = ChunkBody {
        schema: text(value, "schema")?.to_owned(),
        table: text(value, "table")?.to_owned(),
        chunk_no: i32::try_from(bounded(value, "chunk_no", 1, i64::from(i32::MAX))?).map_err(|_| "chunk_no")?,
        attempt: i32::try_from(bounded(value, "attempt", 1, i64::from(i32::MAX))?).map_err(|_| "attempt")?,
        key_lo,
        key_hi,
        terminal,
        row_count,
        part_sha256,
        columns: columns(value)?,
        key: text(value, "key")?.to_owned(),
        part,
    };
    if !body.columns.contains(&body.key) {
        return Err("open_shape");
    }
    if body.terminal {
        if body.key_hi.is_some() || body.row_count != 0 {
            return Err("invalid_chunk");
        }
        match &body.part {
            None if body.part_sha256 == EMPTY_PART_SHA256 => {}
            None => return Err("part_sha256"),
            Some(part) if part.rows == 0 => {}
            Some(_) => return Err("invalid_chunk"),
        }
    } else {
        let part = body.part.as_ref().ok_or("invalid_chunk")?;
        if body.key_hi.is_none() || body.row_count != part.rows || part.rows == 0 {
            return Err("invalid_chunk");
        }
        if let (Some(lo), Some(hi)) = (body.key_lo, body.key_hi) {
            if lo >= hi {
                return Err("invalid_chunk");
            }
        }
        if let (Some(lo), Some(high)) = (body.key_lo, part.key_lo) {
            if high <= lo {
                return Err("chunk_keys");
            }
        }
        if let (Some(hi), Some(high)) = (body.key_hi, part.key_hi) {
            if high > hi {
                return Err("chunk_keys");
            }
        }
    }
    if let Some(part) = &body.part {
        if part.sha256 != body.part_sha256 {
            return Err("part_sha256");
        }
    }
    Ok(body)
}

fn batch_body(value: &Value) -> Result<BatchBody, Token> {
    let first_pos = position_text(value, "first_pos")?;
    let last_pos = position_text(value, "last_pos")?;
    let txns = bounded(value, "txns", 1, i64::try_from(MAX_TXNS).unwrap_or(1000))?;
    let row_count = bounded(value, "row_count", 0, i64::MAX)?.unsigned_abs();
    let txn_records = value.get("txn_records").filter(|item| item.is_array()).cloned().ok_or("txn_records")?;
    if txn_records.as_array().map_or(0, Vec::len) as i64 != txns {
        return Err("batch_txns");
    }
    let first: u128 = first_pos.parse().map_err(|_| "position")?;
    let last: u128 = last_pos.parse().map_err(|_| "position")?;
    if last < first || last - first + 1 != u128::try_from(txns).unwrap_or(0) {
        return Err("batch_txns");
    }
    let mut parts = Vec::new();
    for item in array(value, "parts", 0, MAX_BATCH_PARTS)? {
        let part = BatchPart {
            schema: text(item, "schema")?.to_owned(),
            table: text(item, "table")?.to_owned(),
            columns: columns(item)?,
            key: text(item, "key")?.to_owned(),
            part: part_ref(item.get("part").ok_or("part")?)?,
        };
        if !part.columns.contains(&part.key) || part.part.rows == 0 {
            return Err("batch_part");
        }
        if parts.iter().any(|old: &BatchPart| old.schema == part.schema && old.table == part.table) {
            return Err("duplicate_table");
        }
        parts.push(part);
    }
    if parts.iter().map(|part| part.part.rows).sum::<u64>() != row_count {
        return Err("batch_rows");
    }
    Ok(BatchBody {
        batch_no: i32::try_from(bounded(value, "batch_no", 1, i64::from(i32::MAX))?).map_err(|_| "batch_no")?,
        first_pos,
        last_pos,
        txns: i32::try_from(txns).map_err(|_| "txns")?,
        row_count,
        batch_sha256: hex64(value, "batch_sha256")?,
        txn_records,
        parts,
        max_tombstones: match value.get("max_tombstones") {
            None | Some(Value::Null) => DEFAULT_MAX_TOMBSTONES,
            Some(item) => item.as_u64().filter(|n| *n >= 1).ok_or("max_tombstones")?,
        },
    })
}

fn seal_body(value: &Value) -> Result<SealBody, Token> {
    let mut tables = Vec::new();
    for item in array(value, "tables", 1, MAX_TABLES)? {
        let table = SealTable {
            schema: text(item, "schema")?.to_owned(),
            table: text(item, "table")?.to_owned(),
            row_count: bounded(item, "row_count", 0, i64::MAX)?.unsigned_abs(),
            payload_digest: hex64(item, "payload_digest")?,
        };
        if tables.iter().any(|old: &SealTable| old.schema == table.schema && old.table == table.table) {
            return Err("duplicate_table");
        }
        tables.push(table);
    }
    Ok(SealBody {
        l_end: text(value, "l_end")?.to_owned(),
        l_end_pos: position_text(value, "l_end_pos")?,
        hook_digest: hex64(value, "hook_digest")?,
        tables,
    })
}

fn part_ref(item: &Value) -> Result<PartRef, Token> {
    let raw = item.get("path").and_then(Value::as_str).ok_or("part_path")?;
    if raw.is_empty() || raw.len() > 4096 || raw.contains('\0') {
        return Err("part_path");
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err("part_path");
    }
    let bytes = item.get("bytes").and_then(Value::as_u64).ok_or("request_field")?;
    if bytes == 0 || bytes > PART_MAX_BYTES {
        return Err("part_bytes");
    }
    let rows = bounded(item, "rows", 0, i64::MAX)?.unsigned_abs();
    let key_lo = opt_key(item, "key_lo")?;
    let key_hi = opt_key(item, "key_hi")?;
    if (rows == 0) != (key_lo.is_none() && key_hi.is_none()) || (rows > 0 && key_lo.zip(key_hi).is_some_and(|(lo, hi)| lo > hi)) {
        return Err("key_range");
    }
    Ok(PartRef { path, bytes, sha256: hex64(item, "sha256")?, rows, key_lo, key_hi })
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, Token> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|item| !item.is_empty() && item.len() <= 255 && !item.contains('\0'))
        .ok_or("request_field")
}

fn ident(value: &Value, key: &str) -> Result<String, Token> {
    let item = text(value, key)?;
    let bytes = item.as_bytes();
    let ok = bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_');
    if ok { Ok(item.to_owned()) } else { Err("request_field") }
}

fn uuid(value: &Value, key: &str) -> Result<String, Token> {
    let item = text(value, key)?;
    let shaped = item.len() == 36
        && item.bytes().enumerate().all(|(index, b)| match index {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        });
    if shaped { Ok(item.to_owned()) } else { Err("request_field") }
}

fn hex64(value: &Value, key: &str) -> Result<String, Token> {
    let item = text(value, key)?;
    if item.len() == 64 && item.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        Ok(item.to_owned())
    } else {
        Err("digest")
    }
}

fn position_text(value: &Value, key: &str) -> Result<String, Token> {
    let item = text(value, key)?;
    let ok = item.len() <= 38 && item.bytes().all(|b| b.is_ascii_digit()) && (item.len() == 1 || !item.starts_with('0'));
    if ok { Ok(item.to_owned()) } else { Err("position") }
}

fn bounded(value: &Value, key: &str, min: i64, max: i64) -> Result<i64, Token> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .filter(|n| (min..=max).contains(n))
        .ok_or("request_field")
}

fn array<'a>(value: &'a Value, key: &str, min: usize, max: usize) -> Result<&'a Vec<Value>, Token> {
    value
        .get(key)
        .and_then(Value::as_array)
        .filter(|items| (min..=max).contains(&items.len()))
        .ok_or("request_field")
}

fn columns(value: &Value) -> Result<Vec<String>, Token> {
    let mut names: Vec<String> = Vec::new();
    for item in array(value, "columns", 1, MAX_COLUMNS)? {
        let name = item.as_str().filter(|n| !n.is_empty() && n.len() <= 255 && !n.contains('\0')).ok_or("request_field")?;
        if names.iter().any(|old| old == name) {
            return Err("duplicate_column");
        }
        names.push(name.to_owned());
    }
    Ok(names)
}

fn opt_key(value: &Value, key: &str) -> Result<Option<i128>, Token> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(item)) => {
            let parsed: i128 = item.parse().map_err(|_| "key_range")?;
            if (KEY_MIN..=KEY_MAX).contains(&parsed) && parsed.to_string() == *item {
                Ok(Some(parsed))
            } else {
                Err("key_range")
            }
        }
        Some(_) => Err("key_range"),
    }
}

/// Add `protocol` and `operation` to a result document.
fn with_operation(mut value: Value, operation: &str) -> Value {
    if let Value::Object(map) = &mut value {
        map.insert("protocol".into(), json!(PROTOCOL_V2));
        map.insert("operation".into(), json!(operation));
    }
    value
}

/// Refusal document for a v2 request. The reason is a closed token.
pub fn refused_v2(reason: Token) -> Value {
    json!({"protocol": PROTOCOL_V2, "outcome": "refused", "reason": reason})
}

/// Execute one validated v2 request on a runtime-role connection.
///
/// The fence is released after every operation that took it, whatever the outcome.
/// The release is a best effort: a failed release only delays other writers.
pub async fn execute_v2(conn: &mut PgConnection, request: &Request) -> Result<Value, Token> {
    let result = match &request.body {
        Body::Open(body) => open_tables(conn, &request.envelope, request.fence.as_ref(), body).await,
        Body::Chunk(body) => chunk(conn, &request.envelope, body).await,
        Body::TxnBatch(body) => txn_batch(conn, &request.envelope, body).await,
        Body::Rebind => rebind(conn, &request.envelope, request.fence.as_ref()).await,
        Body::Seal(body) => seal(conn, &request.envelope, request.fence.as_ref(), body).await,
        Body::Retire => retire(conn, &request.envelope, request.fence.as_ref()).await,
    };
    if let Some(fence) = &request.fence {
        let _ = release_fence(conn, &fence.shard).await;
    }
    result
}

/// Adopt the target generation of the fence and bind this session to it. Returns the generation.
pub async fn adopt_and_bind(tx: &mut Transaction<'_, Postgres>, fence: &Fence) -> Result<i64, Token> {
    let generation: i64 = sqlx::query_scalar("SELECT _oo.adopt_target_generation($1, $2::uuid)")
        .bind(&fence.shard)
        .bind(&fence.owner)
        .fetch_one(&mut **tx)
        .await
        .map_err(native_c1_apply::sql_error)?;
    sqlx::query("SELECT _oo.full_import_bind_session($1, $2::uuid, $3)")
        .bind(&fence.shard)
        .bind(&fence.owner)
        .bind(generation)
        .execute(&mut **tx)
        .await
        .map_err(native_c1_apply::sql_error)?;
    Ok(generation)
}

fn is_sqlstate(err: &sqlx::Error, code: &str) -> bool {
    err.as_database_error().and_then(|db| db.code()).is_some_and(|found| found == code)
}

/// Release the shard lock that adoption took on this session.
pub async fn release_fence(conn: &mut PgConnection, shard: &str) -> Result<(), Token> {
    sqlx::query("SELECT _oo.native_update_release_fence($1)")
        .bind(shard)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(native_c1_apply::sql_error)
}

fn required<'a>(fence: Option<&'a Fence>) -> Result<&'a Fence, Token> {
    fence.ok_or("fence")
}

async fn lock(tx: &mut Transaction<'_, Postgres>, envelope: &Envelope) -> Result<(), Token> {
    sqlx::query("SELECT _oo.native_c1_baseline_lock($1, $2::uuid)")
        .bind(&envelope.source)
        .bind(&envelope.task)
        .execute(&mut **tx)
        .await
        .map(|_| ())
        .map_err(native_c1_apply::sql_error)
}

async fn open_tables(
    conn: &mut PgConnection,
    envelope: &Envelope,
    fence: Option<&Fence>,
    body: &OpenBody,
) -> Result<Value, Token> {
    let fence = required(fence)?;
    let mut tx = conn.begin().await.map_err(native_c1_apply::sql_error)?;
    lock(&mut tx, envelope).await?;
    if let Some(previous) = &body.previous_task {
        // Transfer only an exactly acknowledged predecessor, atomically with v2 open.
        sqlx::query("SELECT _oo.native_c1_ack_seed($1,$2::uuid,$3,$4,$5::uuid,$6)")
            .bind(&envelope.source)
            .bind(&envelope.task)
            .bind(envelope.generation)
            .bind(envelope.epoch)
            .bind(previous)
            .bind(body.source_generation - 1)
            .execute(&mut *tx)
            .await
            .map_err(native_c1_apply::sql_error)?;
    }
    sqlx::query(
        "SELECT _oo.native_c1_baseline_v2_open($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::numeric)",
    )
    .bind(&envelope.source)
    .bind(&envelope.task)
    .bind(envelope.generation)
    .bind(envelope.epoch)
    .bind(&body.measured_id)
    .bind(&body.config_digest)
    .bind(&body.schema_digest)
    .bind(&body.l0)
    .bind(&body.l0_pos)
    .execute(&mut *tx)
    .await
    .map_err(native_c1_apply::sql_error)?;
    for table in &body.tables {
        register_table_mappings(&mut tx, envelope, body, table).await?;
    }
    let target_generation = adopt_and_bind(&mut tx, fence).await?;
    let mut tables = Vec::with_capacity(body.tables.len());
    for table in &body.tables {
        let text: String = sqlx::query_scalar(
            "SELECT _oo.native_c1_baseline_v2_open_target($1,$2::uuid,$3,$4,$5,$6::uuid,$7,$8,$9,$10,$11,$12,$13,$14::text[],$15::bigint[],$16::integer[],$17::boolean[],$18)::text",
        )
        .bind(&envelope.source)
        .bind(&envelope.task)
        .bind(envelope.generation)
        .bind(envelope.epoch)
        .bind(&fence.shard)
        .bind(&fence.owner)
        .bind(target_generation)
        .bind(body.source_generation)
        .bind(&body.type_digest)
        .bind(&body.input_digest)
        .bind(&table.table_schema_digest)
        .bind(&table.schema)
        .bind(&table.table)
        .bind(&table.columns)
        .bind(&table.oids)
        .bind(&table.typmods)
        .bind(&table.not_null)
        .bind(&table.key)
        .fetch_one(&mut *tx)
        .await
        .map_err(native_c1_apply::sql_error)?;
        let mut item: Value = serde_json::from_str(&text).map_err(|_| "target_receipt")?;
        item["schema"] = json!(table.schema);
        item["table"] = json!(table.table);
        tables.push(item);
    }
    tx.commit().await.map_err(native_c1_apply::sql_error)?;
    let outcome = if tables.iter().any(|item| item["outcome"] == "opened") { "opened" } else { "replay" };
    Ok(with_operation(
        json!({"outcome": outcome, "format": "chunked-typed/v2", "target_generation": target_generation, "tables": tables}),
        "open",
    ))
}

async fn register_table_mappings(
    tx: &mut Transaction<'_, Postgres>,
    envelope: &Envelope,
    body: &OpenBody,
    table: &OpenTable,
) -> Result<(), Token> {
    use crate::native_arrow::register_mapping;
    register_mapping(tx, &envelope.source, &body.config_digest, body.source_generation, "namespace", &table.schema, "", "")
        .await
        .map_err(|_| "physical")?;
    register_mapping(tx, &envelope.source, &body.config_digest, body.source_generation, "table", &table.schema, &table.table, "")
        .await
        .map_err(|_| "physical")?;
    for column in &table.columns {
        register_mapping(tx, &envelope.source, &body.config_digest, body.source_generation, "column", &table.schema, &table.table, column)
            .await
            .map_err(|_| "physical")?;
    }
    Ok(())
}

async fn chunk(conn: &mut PgConnection, envelope: &Envelope, body: &ChunkBody) -> Result<Value, Token> {
    let mut tx = conn.begin().await.map_err(native_c1_apply::sql_error)?;
    lock(&mut tx, envelope).await?;
    let target = native_c1_apply::stage_target(&mut tx, &envelope.source, &envelope.task, &body.schema, &body.table).await?;
    if let Some(part) = &body.part {
        let entry = PartEntry {
            part_no: u32::try_from(body.chunk_no).map_err(|_| "chunk_no")?,
            kind: PartKind::Fill,
            key_lo: part.key_lo,
            key_hi: part.key_hi,
            rows: part.rows,
            bytes: part.bytes,
            sha256: part.sha256.clone(),
        };
        let decoded = native_c1_apply::read_verified(&part.path, PartKind::Fill, &entry)?;
        let mut rows = CopyRows::new(&decoded, &body.columns, &target.columns, Tags::Snapshot { part_no: body.chunk_no })?;
        let key_index = body.columns.iter().position(|name| *name == body.key).ok_or("open_shape")?;
        native_c1_apply::check_keys(&decoded, key_index, &entry)?;
        let copied = native_c1_apply::copy_into_stage(&mut tx, &target.stage, &target.columns, &mut rows).await?;
        if copied != part.rows {
            return Err("chunk_part_rows");
        }
    }
    let out: String = sqlx::query_scalar(
        "SELECT _oo.native_c1_baseline_v2_chunk_apply($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::numeric,$10::numeric,$11,$12,$13)::text",
    )
    .bind(&envelope.source)
    .bind(&envelope.task)
    .bind(envelope.generation)
    .bind(envelope.epoch)
    .bind(&body.schema)
    .bind(&body.table)
    .bind(body.chunk_no)
    .bind(body.attempt)
    .bind(body.key_lo.map(|key| key.to_string()))
    .bind(body.key_hi.map(|key| key.to_string()))
    .bind(body.terminal)
    .bind(i64::try_from(body.row_count).map_err(|_| "request_field")?)
    .bind(&body.part_sha256)
    .fetch_one(&mut *tx)
    .await
    .map_err(native_c1_apply::sql_error)?;
    tx.commit().await.map_err(native_c1_apply::sql_error)?;
    Ok(with_operation(serde_json::from_str(&out).map_err(|_| "target_receipt")?, "chunk"))
}

async fn txn_batch(conn: &mut PgConnection, envelope: &Envelope, body: &BatchBody) -> Result<Value, Token> {
    let mut tx = conn.begin().await.map_err(native_c1_apply::sql_error)?;
    lock(&mut tx, envelope).await?;
    for part in &body.parts {
        let target = native_c1_apply::stage_target(&mut tx, &envelope.source, &envelope.task, &part.schema, &part.table).await?;
        let entry = PartEntry {
            part_no: u32::try_from(body.batch_no).map_err(|_| "batch_no")?,
            kind: PartKind::Cdc,
            key_lo: part.part.key_lo,
            key_hi: part.part.key_hi,
            rows: part.part.rows,
            bytes: part.part.bytes,
            sha256: part.part.sha256.clone(),
        };
        let decoded = native_c1_apply::read_verified(&part.part.path, PartKind::Cdc, &entry)?;
        let mut rows = CopyRows::new(&decoded, &part.columns, &target.columns, Tags::Cdc { part_no: body.batch_no })?;
        let key_index = part.columns.iter().position(|name| *name == part.key).ok_or("open_shape")?;
        native_c1_apply::check_keys(&decoded, key_index, &entry)?;
        let deletes = native_c1_apply::delete_count(&decoded, part.columns.len())?;
        let live = native_c1_apply::tombstone_count(&mut tx, envelope, &part.schema, &part.table).await?;
        if live.saturating_add(deletes) > body.max_tombstones {
            return Err("tombstone_overflow");
        }
        let copied = native_c1_apply::copy_into_stage(&mut tx, &target.stage, &target.columns, &mut rows).await?;
        if copied != part.part.rows {
            return Err("batch_rows");
        }
    }
    let out: String = sqlx::query_scalar(
        "SELECT _oo.native_c1_baseline_v2_txn_batch($1,$2::uuid,$3,$4,$5,$6::numeric,$7::numeric,$8,$9,$10,$11::jsonb)::text",
    )
    .bind(&envelope.source)
    .bind(&envelope.task)
    .bind(envelope.generation)
    .bind(envelope.epoch)
    .bind(body.batch_no)
    .bind(&body.first_pos)
    .bind(&body.last_pos)
    .bind(body.txns)
    .bind(i64::try_from(body.row_count).map_err(|_| "batch_rows")?)
    .bind(&body.batch_sha256)
    .bind(body.txn_records.to_string())
    .fetch_one(&mut *tx)
    .await
    .map_err(native_c1_apply::sql_error)?;
    tx.commit().await.map_err(native_c1_apply::sql_error)?;
    Ok(with_operation(serde_json::from_str(&out).map_err(|_| "target_receipt")?, "txn_batch"))
}

async fn rebind(conn: &mut PgConnection, envelope: &Envelope, fence: Option<&Fence>) -> Result<Value, Token> {
    let fence = required(fence)?;
    let mut tx = conn.begin().await.map_err(native_c1_apply::sql_error)?;
    let generation = adopt_and_bind(&mut tx, fence).await?;
    let out = rebind_in_tx(&mut tx, envelope, fence, generation).await?;
    tx.commit().await.map_err(native_c1_apply::sql_error)?;
    Ok(with_operation(out, "rebind"))
}

async fn rebind_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    envelope: &Envelope,
    fence: &Fence,
    target_generation: i64,
) -> Result<Value, Token> {
    let out: String = sqlx::query_scalar("SELECT _oo.native_c1_baseline_v2_rebind($1,$2::uuid,$3,$4,$5::uuid,$6)::text")
        .bind(&envelope.source)
        .bind(&envelope.task)
        .bind(envelope.generation)
        .bind(&fence.shard)
        .bind(&fence.owner)
        .bind(target_generation)
        .fetch_one(&mut **tx)
        .await
        .map_err(native_c1_apply::sql_error)?;
    serde_json::from_str(&out).map_err(|_| "target_receipt")
}

/// Seal, with the caller's composed digests (T8 interface).
///
/// Call this in one transaction after [`adopt_and_bind`] returned `target_generation`
/// on that same transaction. Adoption starts a new target generation, so the open
/// full-import runs are rebound to it first (`native_c1_baseline_v2_rebind`). The
/// caller computes each `payload_digest` from the shadow in this same transaction.
/// This function does not verify digests. It checks row counts through the SQL,
/// takes the task lock, and writes the seal. A rebind or seal error rolls back the
/// caller's transaction.
pub async fn seal_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    envelope: &Envelope,
    fence: &Fence,
    target_generation: i64,
    body: &SealBody,
) -> Result<Value, Token> {
    rebind_in_tx(tx, envelope, fence, target_generation).await?;
    lock(tx, envelope).await?;
    let tables: Vec<Value> = body
        .tables
        .iter()
        .map(|table| json!({"schema": table.schema, "table": table.table, "row_count": table.row_count, "payload_digest": table.payload_digest}))
        .collect();
    let out: String = sqlx::query_scalar(
        "SELECT _oo.native_c1_baseline_v2_seal($1,$2::uuid,$3,$4,$5,$6::uuid,$7,$8,$9::numeric,$10,$11::jsonb)::text",
    )
    .bind(&envelope.source)
    .bind(&envelope.task)
    .bind(envelope.generation)
    .bind(envelope.epoch)
    .bind(&fence.shard)
    .bind(&fence.owner)
    .bind(target_generation)
    .bind(&body.l_end)
    .bind(&body.l_end_pos)
    .bind(&body.hook_digest)
    .bind(Value::Array(tables).to_string())
    .fetch_one(&mut **tx)
    .await
    .map_err(native_c1_apply::sql_error)?;
    serde_json::from_str(&out).map_err(|_| "target_receipt")
}

async fn seal(conn: &mut PgConnection, envelope: &Envelope, fence: Option<&Fence>, body: &SealBody) -> Result<Value, Token> {
    let fence = required(fence)?;
    let mut tx = conn.begin().await.map_err(native_c1_apply::sql_error)?;
    let generation = adopt_and_bind(&mut tx, fence).await?;
    let out = seal_in_tx(&mut tx, envelope, fence, generation, body).await?;
    tx.commit().await.map_err(native_c1_apply::sql_error)?;
    Ok(with_operation(out, "seal"))
}

/// Retire every open full-import run of the task (N1). Reuses `full_import_retire_run`.
///
/// Runs already retired are skipped. A run that is sealed or missing is refused,
/// so a sealed task cannot be retired by accident. The rebind inside this
/// operation adopts the live claim, so the call must come from the holder of the
/// current claim (on failure, before the job completes; on abandonment, from the
/// worker that reclaims the task).
async fn retire(conn: &mut PgConnection, envelope: &Envelope, fence: Option<&Fence>) -> Result<Value, Token> {
    let fence = required(fence)?;
    let mut tx = conn.begin().await.map_err(native_c1_apply::sql_error)?;
    let generation = adopt_and_bind(&mut tx, fence).await?;
    let rebound = rebind_in_tx(&mut tx, envelope, fence, generation).await?;
    let mut runs = Vec::new();
    let mut already = Vec::new();
    for table in rebound["tables"].as_array().ok_or("target_receipt")? {
        let run = table["run_id"].as_str().ok_or("target_receipt")?.to_owned();
        let retired = sqlx::query("SELECT _oo.full_import_retire_run($1, $2::uuid, $3, $4::uuid)")
            .bind(&fence.shard)
            .bind(&fence.owner)
            .bind(generation)
            .bind(&run)
            .execute(&mut *tx)
            .await;
        match retired {
            Ok(_) => runs.push(run),
            // The rebind moves only open runs. A run that is already retired keeps an
            // older generation, so the lookup finds nothing (OO031). Nothing is open.
            Err(err) if is_sqlstate(&err, "OO031") => already.push(run),
            Err(err) => return Err(native_c1_apply::sql_error(err)),
        }
    }
    tx.commit().await.map_err(native_c1_apply::sql_error)?;
    Ok(with_operation(
        json!({"outcome": "retired", "target_generation": generation, "runs": runs, "already_retired": already}),
        "retire",
    ))
}

/// Stdin to stdout for the v2 stage command. Exit 0 when the operation applied
/// or replayed, 1 on any refusal. The document on stdout carries the token.
pub async fn run_v2() -> Result<(), u8> {
    use std::io::{Read, Write};
    let mut bytes = Vec::new();
    std::io::stdin()
        .take((MAX_REQUEST_V2 + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| 2u8)?;
    let result = match run_document(&bytes).await {
        Ok(value) => value,
        Err(reason) => refused_v2(reason),
    };
    let ok = result["outcome"] != "refused";
    let mut out = serde_json::to_vec(&result).map_err(|_| 1u8)?;
    out.push(b'\n');
    std::io::stdout().write_all(&out).map_err(|_| 1u8)?;
    if ok { Ok(()) } else { Err(1) }
}

async fn run_document(bytes: &[u8]) -> Result<Value, Token> {
    check_request_size(bytes.len())?;
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "request_json")?;
    let request = parse_request(&value)?;
    let mut conn = crate::native_update::connect_target(&request.envelope.target).await?;
    execute_v2(&mut conn, &request).await
}

#[cfg(test)]
mod v2_tests {
    use super::*;

    const TASK: &str = "11111111-2222-3333-4444-555555555555";
    const H1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const H2: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn envelope(operation: &str) -> Value {
        json!({
            "protocol": PROTOCOL_V2, "operation": operation,
            "source_id": "mysql_x", "lease_id": TASK, "lease_generation": 7, "source_epoch": 4,
            "target": {"host": "127.0.0.1", "port": 5432, "database": "db",
                       "runtime_principal": "oo_pipeline_runtime", "password_file": "/p", "ssl": "disable"}
        })
    }

    fn with_fence(mut value: Value) -> Value {
        value["fence"] = json!({"shard": "shard_x", "owner": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"});
        value
    }

    fn part(name: &str, rows: u64, lo: Option<&str>, hi: Option<&str>, sha: &str) -> Value {
        json!({"path": format!("/parts/app.items/{name}"), "bytes": 1000, "sha256": sha, "rows": rows,
               "key_lo": lo, "key_hi": hi})
    }

    fn chunk(no: i64, lo: Value, hi: Value, rows: u64, sha: &str) -> Value {
        let file = format!("fill-{no:08}.arrow");
        json!({"schema": "app", "table": "items", "chunk_no": no, "attempt": 1,
               "key_lo": lo, "key_hi": hi, "terminal": false, "row_count": rows, "part_sha256": sha,
               "columns": ["id", "val"], "key": "id",
               "part": part(&file, rows, Some("1"), Some("3"), sha)})
    }

    fn chunk_request() -> Value {
        let mut value = envelope("chunk");
        let body = chunk(1, Value::Null, json!("3"), 3, H1);
        for (key, item) in body.as_object().expect("object") {
            value[key] = item.clone();
        }
        value
    }

    fn batch_request() -> Value {
        let mut value = envelope("txn_batch");
        value["batch_no"] = json!(1);
        value["first_pos"] = json!("101");
        value["last_pos"] = json!("103");
        value["txns"] = json!(3);
        value["row_count"] = json!(6);
        value["batch_sha256"] = json!(H2);
        value["txn_records"] = json!([
            {"applied_pos": "101", "txn_id": "x:101", "payload_sha256": H1, "source_commit_timestamp": "2026-10-17T10:00:00Z", "row_count": 2},
            {"applied_pos": "102", "txn_id": "x:102", "payload_sha256": H1, "source_commit_timestamp": "2026-10-17T10:00:00Z", "row_count": 2},
            {"applied_pos": "103", "txn_id": "x:103", "payload_sha256": H1, "source_commit_timestamp": "2026-10-17T10:00:00Z", "row_count": 2}
        ]);
        value["parts"] = json!([{
            "schema": "app", "table": "items", "columns": ["id", "val"], "key": "id",
            "part": part("cdc-00000001.arrow", 6, Some("1"), Some("11"), H1)
        }]);
        value
    }

    fn expect(value: &Value, token: &str) {
        assert_eq!(parse_request(value).err(), Some(token), "{value}");
    }

    #[test]
    fn a_valid_chunk_parses_with_its_part_and_bounds() {
        let request = parse_request(&chunk_request()).expect("valid");
        assert_eq!(request.envelope.generation, 7);
        assert_eq!(request.envelope.epoch, 4);
        assert!(request.fence.is_none());
        let Body::Chunk(body) = request.body else { panic!("chunk") };
        assert_eq!((body.chunk_no, body.key_lo, body.key_hi), (1, None, Some(3)));
        assert_eq!(body.part.expect("part").rows, 3);
    }

    #[test]
    fn a_terminal_chunk_needs_no_part_and_the_empty_digest() {
        let mut value = chunk_request();
        value["terminal"] = json!(true);
        value["key_lo"] = json!("7");
        value["key_hi"] = Value::Null;
        value["row_count"] = json!(0);
        value["part"] = Value::Null;
        value["part_sha256"] = json!(EMPTY_PART_SHA256);
        let request = parse_request(&value).expect("terminal");
        let Body::Chunk(body) = request.body else { panic!("chunk") };
        assert!(body.terminal && body.part.is_none() && body.key_lo == Some(7));
        value["part_sha256"] = json!(H1);
        expect(&value, "part_sha256");
    }

    #[test]
    fn chunk_refusals_are_closed_tokens() {
        let cases: Vec<(&str, Box<dyn Fn(&mut Value)>, &str)> = vec![
            ("protocol", Box::new(|v: &mut Value| v["protocol"] = json!("oo.pipeline.native-c1-baseline-stage/v1")), "protocol"),
            ("operation", Box::new(|v: &mut Value| v["operation"] = json!("drop")), "operation"),
            ("generation", Box::new(|v: &mut Value| v["lease_generation"] = json!(0)), "request_field"),
            ("source", Box::new(|v: &mut Value| v["source_id"] = json!("MySQL")), "request_field"),
            ("lease", Box::new(|v: &mut Value| v["lease_id"] = json!("not-a-uuid")), "request_field"),
            ("epoch", Box::new(|v: &mut Value| v["source_epoch"] = json!(0)), "request_field"),
            ("target", Box::new(|v: &mut Value| { v.as_object_mut().expect("o").remove("target"); }), "target"),
            ("chunk_no", Box::new(|v: &mut Value| v["chunk_no"] = json!(0)), "request_field"),
            ("digest case", Box::new(|v: &mut Value| v["part_sha256"] = json!(H1.replace('1', "A"))), "digest"),
            ("relative path", Box::new(|v: &mut Value| v["part"]["path"] = json!("fill-00000001.arrow")), "part_path"),
            ("part digest", Box::new(|v: &mut Value| v["part"]["sha256"] = json!(H2)), "part_sha256"),
            ("row count", Box::new(|v: &mut Value| v["row_count"] = json!(2)), "invalid_chunk"),
            ("missing key_hi", Box::new(|v: &mut Value| v["key_hi"] = Value::Null), "invalid_chunk"),
            ("lo >= hi", Box::new(|v: &mut Value| { v["key_lo"] = json!("3"); v["key_hi"] = json!("3"); }), "invalid_chunk"),
            ("part above hi", Box::new(|v: &mut Value| v["part"]["key_hi"] = json!("4")), "chunk_keys"),
            ("part at lo", Box::new(|v: &mut Value| { v["key_lo"] = json!("1"); }), "chunk_keys"),
            ("key not a column", Box::new(|v: &mut Value| v["key"] = json!("missing")), "open_shape"),
            ("duplicate column", Box::new(|v: &mut Value| v["columns"] = json!(["id", "id"])), "duplicate_column"),
            ("leading zero key", Box::new(|v: &mut Value| v["key_hi"] = json!("03")), "key_range"),
            ("terminal with hi", Box::new(|v: &mut Value| { v["terminal"] = json!(true); v["row_count"] = json!(0); v["part"] = Value::Null; v["part_sha256"] = json!(EMPTY_PART_SHA256); }), "invalid_chunk"),
            ("empty non-terminal chunk", Box::new(|v: &mut Value| { v["part"]["rows"] = json!(0); v["part"]["key_lo"] = Value::Null; v["part"]["key_hi"] = Value::Null; v["row_count"] = json!(0); }), "invalid_chunk"),
            ("empty part with keys", Box::new(|v: &mut Value| { v["part"]["rows"] = json!(0); v["row_count"] = json!(0); }), "key_range"),
            ("oversize part", Box::new(|v: &mut Value| v["part"]["bytes"] = json!(PART_MAX_BYTES + 1)), "part_bytes"),
        ];
        for (name, mutate, token) in cases {
            let mut value = chunk_request();
            mutate(&mut value);
            assert_eq!(parse_request(&value).err(), Some(token), "case {name}: {value}");
        }
    }

    #[test]
    fn a_valid_batch_parses_and_defaults_the_tombstone_bound() {
        let request = parse_request(&batch_request()).expect("valid");
        let Body::TxnBatch(body) = request.body else { panic!("batch") };
        assert_eq!(body.max_tombstones, DEFAULT_MAX_TOMBSTONES);
        assert_eq!(body.parts.len(), 1);
        assert_eq!(body.txns, 3);
    }

    #[test]
    fn batch_refusals_are_closed_tokens() {
        let cases: Vec<(&str, Box<dyn Fn(&mut Value)>, &str)> = vec![
            ("txn list length", Box::new(|v: &mut Value| { v["txns"] = json!(2); }), "batch_txns"),
            ("position span", Box::new(|v: &mut Value| { v["last_pos"] = json!("104"); v["txns"] = json!(3); }), "batch_txns"),
            ("row sum", Box::new(|v: &mut Value| v["row_count"] = json!(7)), "batch_rows"),
            ("zero rows part", Box::new(|v: &mut Value| { v["parts"][0]["part"]["rows"] = json!(0); v["row_count"] = json!(0); }), "key_range"),
            ("leading zero pos", Box::new(|v: &mut Value| v["first_pos"] = json!("0101")), "position"),
            ("zero bound", Box::new(|v: &mut Value| v["max_tombstones"] = json!(0)), "max_tombstones"),
            ("part key missing", Box::new(|v: &mut Value| v["parts"][0]["key"] = json!("nope")), "batch_part"),
        ];
        for (name, mutate, token) in cases {
            let mut value = batch_request();
            mutate(&mut value);
            assert_eq!(parse_request(&value).err(), Some(token), "case {name}: {value}");
        }
    }

    #[test]
    fn a_batch_may_carry_a_custom_tombstone_bound() {
        let mut value = batch_request();
        value["max_tombstones"] = json!(1000);
        let Body::TxnBatch(body) = parse_request(&value).expect("valid").body else { panic!("batch") };
        assert_eq!(body.max_tombstones, 1000);
    }

    #[test]
    fn duplicate_tables_in_a_batch_are_refused() {
        let mut value = batch_request();
        let second = value["parts"][0].clone();
        value["parts"].as_array_mut().expect("array").push(second);
        value["row_count"] = json!(12);
        expect(&value, "duplicate_table");
    }

    fn open_request() -> Value {
        let mut value = with_fence(envelope("open"));
        value["measured_id"] = json!("mysql.11111111-2222-3333-4444-555555555555");
        value["config_digest"] = json!(H1);
        value["schema_digest"] = json!(H1);
        value["type_digest"] = json!(H1);
        value["input_digest"] = json!(H2);
        value["l0"] = json!("11111111-2222-3333-4444-555555555555:1-100");
        value["l0_pos"] = json!("100");
        value["source_generation"] = json!(1);
        value["tables"] = json!([{
            "schema": "app", "table": "items", "table_schema_digest": H2,
            "columns": ["id", "val"], "oids": [20, 25], "typmods": [-1, -1], "not_null": [true, false], "key": "id"
        }]);
        value
    }

    #[test]
    fn open_parses_tables_and_refuses_shape_faults() {
        let Body::Open(body) = parse_request(&open_request()).expect("valid").body else { panic!("open") };
        assert_eq!(body.tables[0].oids, vec![20, 25]);
        let mut shape = open_request();
        shape["tables"][0]["not_null"] = json!([true]);
        expect(&shape, "open_shape");
        let mut key = open_request();
        key["tables"][0]["key"] = json!("missing");
        expect(&key, "open_shape");
        let mut twice = open_request();
        let table = twice["tables"][0].clone();
        twice["tables"].as_array_mut().expect("array").push(table);
        expect(&twice, "duplicate_table");
    }

    #[test]
    fn successor_open_binds_a_valid_predecessor_only_after_the_first_generation() {
        let Body::Open(first) = parse_request(&open_request()).unwrap().body else { panic!("open") };
        assert!(first.previous_task.is_none());
        let mut value = open_request();
        value["previous_task"] = json!("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        expect(&value, "previous_generation");
        value["source_generation"] = json!(2);
        let Body::Open(next) = parse_request(&value).unwrap().body else { panic!("open") };
        assert_eq!(next.previous_task.as_deref(), Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"));
        value["previous_task"] = json!("not-a-task");
        expect(&value, "request_field");
    }

    #[test]
    fn fence_operations_need_a_fence() {
        let mut rebind = envelope("rebind");
        expect(&rebind, "fence");
        rebind = with_fence(rebind);
        let request = parse_request(&rebind).expect("valid");
        assert_eq!(request.body, Body::Rebind);
        assert_eq!(request.fence.expect("fence").shard, "shard_x");
        let mut retire = envelope("retire");
        expect(&retire, "fence");
        retire = with_fence(retire);
        assert_eq!(parse_request(&retire).expect("valid").body, Body::Retire);
    }

    #[test]
    fn seal_parses_each_table_digest_and_refuses_bad_positions() {
        let mut value = with_fence(envelope("seal"));
        value["l_end"] = json!("11111111-2222-3333-4444-555555555555:1-103");
        value["l_end_pos"] = json!("103");
        value["hook_digest"] = json!(H1);
        value["tables"] = json!([{"schema": "app", "table": "items", "row_count": 4, "payload_digest": H2}]);
        let Body::Seal(body) = parse_request(&value).expect("valid").body else { panic!("seal") };
        assert_eq!(body.tables[0].row_count, 4);
        value["l_end_pos"] = json!("0103");
        expect(&value, "position");
    }

    #[test]
    fn unknown_operations_and_extra_protocols_are_refused() {
        expect(&envelope("apply_everything"), "operation");
        let mut old = envelope("chunk");
        old["protocol"] = json!("oo.pipeline.native-c1-baseline-stage/v1");
        expect(&old, "protocol");
    }

    #[test]
    fn the_request_is_capped_before_it_is_parsed() {
        assert_eq!(check_request_size(0), Err("request_size"));
        assert_eq!(check_request_size(MAX_REQUEST_V2 + 1), Err("request_size"));
        assert_eq!(check_request_size(MAX_REQUEST_V2), Ok(()));
        assert_eq!(MAX_REQUEST_V2, 1024 * 1024);
    }

    #[test]
    fn a_refusal_names_only_the_token() {
        let value = refused_v2("chunk_conflict");
        assert_eq!(value, json!({"protocol": PROTOCOL_V2, "outcome": "refused", "reason": "chunk_conflict"}));
    }

    #[test]
    fn keys_outside_the_loader_domain_are_refused() {
        let mut value = chunk_request();
        value["key_hi"] = json!("18446744073709551616");
        expect(&value, "key_range");
        let mut ok = chunk_request();
        ok["key_hi"] = json!("18446744073709551615");
        ok["part"]["key_hi"] = json!("18446744073709551615");
        assert!(parse_request(&ok).is_ok());
    }
}
