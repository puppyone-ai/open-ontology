//! Worker driver for `chunked-typed/v2` (DESIGN §3.5, §4.3, §6 T9; FRAMES-V2.md).
//!
//! This module is a child of `native_schedule` (see the `#[path]` line there). It
//! reuses that module's claim, lease, and publication tail, and widens no visibility.
//!
//! Layers:
//! - pure frame validation (`validate_*`), frame → loader request mapping (`map_*`),
//!   resume from the loader's `rebind` (`resume_from_rebind`), and retry policy
//!   (`producer_retry`, `loader_retry`). None of these touch I/O.
//! - the stream driver (`run`), generic over [`Port`]. It spawns the producer, acks
//!   each frame only after the loader has committed it (R1), and restarts the
//!   producer from a fresh `rebind` after a crash.
//! - production glue (`ProdPort`, `apply_v2`). The post-`end` steps (materialize v2,
//!   admission v2, final load v2) sit behind [`PostEnd`]; until T8 and T10 land, the
//!   only implementation is [`Pending`], which stops the run in the seal-ready state.

use super::{
    Config, Executable, Manifest, apply_cdc_baseline, confirmed_generation, executable_check,
    finish_c1_quiet, target_owner,
};
use crate::{
    api::{LeaseStatus, is_uuid},
    error::{Error, Result},
    lease, session,
    sha256::sha256_hex,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
};

/// The format name for the manifest and the bootstrap.
pub const FORMAT: &str = "chunked-typed/v2";
/// Loader argument of the final load for this format (manifest `loader.args`).
pub const LOADER_ARG: &str = "import-native-c1-baseline-v2";
/// Producer subcommand for this format (manifest `producer.args[0]`).
pub const PRODUCER_SUBCOMMAND: &str = "c1-baseline-stream-v2";
/// Loader stage subcommand (stdin JSON, stdout JSON). Mirrors the v1 `native-c1-baseline-stage`.
pub const STAGE_SUBCOMMAND: &str = "native-c1-baseline-stage-v2";
/// Protocol of the request the post-`end` steps hand to the final load (T8/T10 interface).
pub const REQUEST_PROTOCOL: &str = "oo.pipeline.native-c1-baseline/v2";

const PROTO_BOOTSTRAP: &str = "oo.jobs.mysql-c1-bootstrap/v2";
const PROTO_CHUNK: &str = "oo.jobs.mysql-c1-chunk/v2";
const PROTO_TXN_BATCH: &str = "oo.jobs.mysql-c1-txn-batch/v2";
const PROTO_ACK: &str = "oo.jobs.mysql-c1-baseline-ack/v2";
const PROTO_NOCHANGE: &str = "oo.jobs.mysql-c1-nochange/v2";
const PROTO_STAGE: &str = "oo.pipeline.native-c1-baseline-stage/v2";
const PROTO_EVIDENCE: &str = "oo.jobs.mysql-c1-stream-evidence/v2";
const PARTS_MANIFEST: &str = "parts-manifest.json";
const LEDGER_FILE: &str = "worker-frames.ndjson";
const PRODUCER_STDERR: &str = "producer-stderr.txt";
/// Producer attempts per claim. Retries sleep 1, 2, 4, 8, 16, then 30 s each, which is
/// about 5.5 minutes in total. A tunnel outage of a few minutes is survived in one claim.
const MAX_PRODUCER_ATTEMPTS: u32 = 16;
/// Producer rounds per claim. When a round's attempts are spent, the producer is restarted
/// from a fresh rebind, with the loader run kept open. Five rounds bound the claim.
const MAX_PRODUCER_ROUNDS: u32 = 5;
const MAX_LOADER_ATTEMPTS: u32 = 3;
/// Loader refusals recorded per claim in the ledger. The ledger stays small.
const MAX_REFUSAL_RECORDS: u32 = 16;
const MAX_TABLES: usize = 32;
const MAX_COLUMNS: usize = 1024;
const MAX_TXNS: usize = 1000;
const MAX_BATCH_PARTS: usize = 32;
const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LOADER_OUTPUT: usize = 1024 * 1024;
const MAX_BACKOFF_SECS: u64 = 30;
const HEARTBEAT: Duration = Duration::from_secs(10);
/// Loader key domain (the same bounds as `native_c1_baseline::KEY_MIN/KEY_MAX`).
const KEY_MIN: i128 = -(1 << 63);
const KEY_MAX: i128 = (1 << 64) - 1;

type Token = &'static str;

/// Why a step failed. `Lost` means the task lease is gone: nothing more may be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The lease or the target fence was lost. The driver stops without retiring.
    Lost,
    /// A closed token (a loader refusal, or a worker check that failed).
    Token(Token),
}

impl From<Token> for Fault {
    fn from(token: Token) -> Self {
        Fault::Token(token)
    }
}

impl From<Fault> for Error {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Lost => Error::new("lost_lease"),
            Fault::Token(token) => Error::new(token),
        }
    }
}

// ---------------------------------------------------------------- encoding helpers

fn canonical_u64(text: &str) -> Option<u64> {
    let ok = !text.is_empty()
        && text.len() <= 20
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text.len() == 1 || !text.starts_with('0'));
    if ok { text.parse().ok() } else { None }
}

fn text<'a>(value: &'a Value, key: &str) -> std::result::Result<&'a str, Token> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && !s.contains('\0'))
        .ok_or("frame_field")
}

fn uint(value: &Value, key: &str) -> std::result::Result<u64, Token> {
    value.get(key).and_then(Value::as_u64).ok_or("frame_field")
}

fn uint_text(value: &Value, key: &str) -> std::result::Result<u64, Token> {
    canonical_u64(text(value, key)?).ok_or("frame_field")
}

fn hex64(value: &Value, key: &str) -> std::result::Result<String, Token> {
    let item = text(value, key)?;
    if is_hex64(item) {
        Ok(item.to_owned())
    } else {
        Err("digest")
    }
}

fn is_hex64(item: &str) -> bool {
    item.len() == 64
        && item
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A key as a canonical decimal string (or null / absent). Mirrors the loader's `opt_key`.
fn opt_key(value: &Value, key: &str) -> std::result::Result<Option<i128>, Token> {
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

fn key_text(key: Option<i128>) -> Value {
    key.map_or(Value::Null, |k| json!(k.to_string()))
}

/// A relative part path of the form `parts/<schema>.<table>/<prefix>NNNNNNNN.arrow`.
fn check_part_rel(rel: &str, prefix: &str) -> std::result::Result<(), Token> {
    let path = Path::new(rel);
    let components_ok = !path.is_absolute()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
        && matches!(path.components().next(), Some(Component::Normal(dir)) if dir == "parts");
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let name_ok = name.len() == prefix.len() + 8 + ".arrow".len()
        && name.starts_with(prefix)
        && name.ends_with(".arrow")
        && name[prefix.len()..prefix.len() + 8]
            .bytes()
            .all(|b| b.is_ascii_digit());
    if components_ok && name_ok && rel.len() <= 512 {
        Ok(())
    } else {
        Err("part_path")
    }
}

fn merge(mut base: Value, extra: Value) -> Value {
    if let (Value::Object(target), Value::Object(source)) = (&mut base, extra) {
        for (key, item) in source {
            target.insert(key, item);
        }
    }
    base
}

// ---------------------------------------------------------------- facts

/// One column of a target table, as the loader needs it for `open`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Typed {
    pub name: String,
    pub oid: i64,
    pub typmod: i32,
    pub not_null: bool,
}

/// Where the target typing of each table comes from.
///
/// `FromFrame` reads `table_schemas[].target_columns` from the producer's `open`
/// frame. That field is not in FRAMES-V2 yet (the gap is reported). `Fixed` supplies
/// the typing from outside, for tests that know the fixture DDL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Typing {
    FromFrame,
    Fixed(BTreeMap<String, Vec<Typed>>),
}

/// One table of the run, validated from the `open` frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSpec {
    pub schema: String,
    pub table: String,
    pub schema_digest: String,
    pub columns: Vec<String>,
    pub key: String,
    pub typed: Vec<Typed>,
}

impl TableSpec {
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.schema, self.table)
    }
}

/// The validated `open` frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenFacts {
    pub uuid: String,
    pub l0: String,
    pub l0_pos: u64,
    pub config_digest: String,
    pub schema_digest: String,
    /// Bound at open, exactly as the producer sent them (FRAMES-V2 §3.1). Never recomputed here.
    pub type_digest: String,
    pub input_digest: String,
    pub applied: u64,
    pub tables: Vec<TableSpec>,
}

impl OpenFacts {
    pub fn spec(&self, qualified: &str) -> Option<&TableSpec> {
        self.tables.iter().find(|t| t.qualified() == qualified)
    }
}

/// What the worker has acknowledged for one table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableProgress {
    pub last_chunk_no: u64,
    pub last_key_hi: Option<i128>,
    pub terminal: bool,
    pub rows: u64,
    /// Last CDC part number seen in this run. `None` after a resume (the loader does not
    /// report it), so the first CDC part of a resumed run is not checked for continuity.
    pub cdc_part_no: Option<u64>,
}

/// The worker's acknowledged state for the whole run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub applied: u64,
    pub batch_no: u64,
    pub tables: BTreeMap<String, TableProgress>,
}

/// A validated chunk frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkFacts {
    pub qualified: String,
    pub chunk_no: u64,
    pub attempt: u64,
    pub terminal: bool,
    pub key_lo: Option<i128>,
    pub key_hi: Option<i128>,
    pub rows: u64,
    pub part_rel: String,
    pub part_sha256: String,
    pub part_bytes: u64,
}

/// One CDC part of a batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartFacts {
    pub qualified: String,
    pub part_no: u64,
    pub part_rel: String,
    pub part_sha256: String,
    pub part_bytes: u64,
    pub rows: u64,
    pub key_lo: i128,
    pub key_hi: i128,
}

/// A validated txn-batch frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchFacts {
    pub batch_no: u64,
    pub first: u64,
    pub last: u64,
    pub sha256: String,
    pub rows: u64,
    pub parts: Vec<PartFacts>,
    pub txn_records: Vec<Value>,
}

/// A validated `end` frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndFacts {
    pub l_end: String,
    pub l_end_pos: u64,
    pub parts_manifest_sha256: String,
}

// ---------------------------------------------------------------- validation

/// Validates the `open` frame and builds the table specs. `resumed` must match the
/// frame's `resume` flag, so the producer and the worker agree on the run kind.
pub fn validate_open(
    frame: &Value,
    source: &str,
    config_sha256: &str,
    typing: &Typing,
    resumed: bool,
) -> std::result::Result<OpenFacts, Token> {
    if frame["kind"] != "open" || frame["protocol"] != PROTO_CHUNK {
        return Err("frame_kind");
    }
    if text(frame, "connection_config_sha256")? != config_sha256 {
        return Err("baseline_config");
    }
    let uuid = text(frame, "source_uuid")?;
    if !is_uuid(uuid) {
        return Err("baseline_identity");
    }
    let config_digest = hex64(frame, "config_digest")?;
    let schema_digest = hex64(frame, "schema_digest")?;
    let type_digest = hex64(frame, "type_digest")?;
    let input_digest = hex64(frame, "input_digest")?;
    let admitted = frame.get("admission_config").ok_or("baseline_identity")?;
    if admitted["protocol"] != "oo.source-admission.config/v1"
        || admitted["source_id"] != source
        || sha256_hex(&serde_json::to_vec(admitted).map_err(|_| "baseline_identity")?)
            != config_digest
    {
        return Err("baseline_identity");
    }
    let preimage = frame.get("schema_preimage").ok_or("baseline_catalog")?;
    if sha256_hex(&serde_json::to_vec(preimage).map_err(|_| "baseline_catalog")?) != schema_digest {
        return Err("baseline_catalog");
    }
    let l0_pos = uint_text(frame, "l0_pos")?;
    let l0 = text(frame, "l0")?;
    if l0_pos == 0 || l0 != format!("{uuid}:1-{l0_pos}") {
        return Err("baseline_l0");
    }
    let applied = uint_text(frame, "applied_pos")?;
    if frame["resume"].as_bool() != Some(resumed) || applied < l0_pos {
        return Err("baseline_resume");
    }
    let tables = validate_tables(frame, typing)?;
    let mut listed: Vec<String> = frame["tables"]
        .as_array()
        .ok_or("baseline_catalog")?
        .iter()
        .map(|item| item.as_str().map(str::to_owned).ok_or("baseline_catalog"))
        .collect::<std::result::Result<_, _>>()?;
    let mut declared: Vec<String> = tables.iter().map(TableSpec::qualified).collect();
    listed.sort();
    declared.sort();
    if listed != declared {
        return Err("baseline_catalog");
    }
    Ok(OpenFacts {
        uuid: uuid.to_owned(),
        l0: l0.to_owned(),
        l0_pos,
        config_digest,
        schema_digest,
        type_digest,
        input_digest,
        applied,
        tables,
    })
}

fn validate_tables(frame: &Value, typing: &Typing) -> std::result::Result<Vec<TableSpec>, Token> {
    let items = frame["table_schemas"]
        .as_array()
        .filter(|items| !items.is_empty() && items.len() <= MAX_TABLES)
        .ok_or("baseline_catalog")?;
    let mut out = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    for item in items {
        let schema = text(item, "schema")?;
        let table = text(item, "table")?;
        let qualified = format!("{schema}.{table}");
        if !seen.insert(qualified.clone()) {
            return Err("baseline_catalog");
        }
        let schema_digest = hex64(item, "schema_sha256")?;
        let columns_json = item["columns"]
            .as_array()
            .filter(|columns| !columns.is_empty() && columns.len() <= MAX_COLUMNS)
            .ok_or("baseline_catalog")?;
        let mut columns: Vec<String> = Vec::with_capacity(columns_json.len());
        let mut keys: Vec<String> = Vec::new();
        let mut last_ordinal = 0u64;
        for column in columns_json {
            let ordinal = uint(column, "ordinal")?;
            if ordinal <= last_ordinal {
                return Err("baseline_catalog");
            }
            last_ordinal = ordinal;
            let name = text(column, "name")?;
            if columns.iter().any(|known| known == name) {
                return Err("baseline_catalog");
            }
            if column["primary_key"].as_bool() == Some(true) {
                keys.push(name.to_owned());
            }
            columns.push(name.to_owned());
        }
        if keys.len() != 1 {
            return Err("baseline_catalog");
        }
        let typed = match typing {
            Typing::FromFrame => typed_from_frame(item)?,
            Typing::Fixed(map) => map.get(&qualified).cloned().ok_or("baseline_catalog")?,
        };
        let typed_names: Vec<&str> = typed.iter().map(|t| t.name.as_str()).collect();
        if typed_names != columns.iter().map(String::as_str).collect::<Vec<_>>() {
            return Err("baseline_catalog");
        }
        out.push(TableSpec {
            schema: schema.to_owned(),
            table: table.to_owned(),
            schema_digest,
            columns,
            key: keys.remove(0),
            typed,
        });
    }
    Ok(out)
}

fn typed_from_frame(item: &Value) -> std::result::Result<Vec<Typed>, Token> {
    let columns = item["target_columns"]
        .as_array()
        .filter(|columns| !columns.is_empty() && columns.len() <= MAX_COLUMNS)
        .ok_or("baseline_catalog")?;
    columns
        .iter()
        .map(|column| {
            Ok(Typed {
                name: text(column, "name")?.to_owned(),
                oid: column["oid"].as_i64().ok_or("baseline_catalog")?,
                typmod: column["typmod"]
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .ok_or("baseline_catalog")?,
                not_null: column["not_null"].as_bool().ok_or("baseline_catalog")?,
            })
        })
        .collect()
}

/// Validates a chunk frame against the open tables and the acknowledged progress.
pub fn validate_chunk(
    frame: &Value,
    open: &OpenFacts,
    progress: &Progress,
) -> std::result::Result<ChunkFacts, Token> {
    if frame["kind"] != "chunk" || frame["protocol"] != PROTO_CHUNK {
        return Err("frame_kind");
    }
    let qualified = format!("{}.{}", text(frame, "schema")?, text(frame, "table")?);
    if open.spec(&qualified).is_none() {
        return Err("chunk_table");
    }
    let state = progress.tables.get(&qualified).cloned().unwrap_or_default();
    if state.terminal {
        return Err("chunk_after_terminal");
    }
    let chunk_no = uint(frame, "chunk_no")?;
    if chunk_no != state.last_chunk_no + 1 {
        return Err("chunk_sequence");
    }
    let attempt = uint(frame, "attempt")?;
    let terminal = frame["terminal"].as_bool().ok_or("frame_field")?;
    let rows = uint(frame, "rows")?;
    let key_lo = opt_key(frame, "key_lo")?;
    let key_hi = opt_key(frame, "key_hi")?;
    let part_rel = text(frame, "part_path")?;
    check_part_rel(part_rel, "fill-")?;
    let part_sha256 = hex64(frame, "part_sha256")?;
    let part_bytes = uint(frame, "part_bytes")?;
    if attempt == 0 || part_bytes == 0 {
        return Err("frame_field");
    }
    if terminal {
        if rows != 0 || key_lo.is_some() || key_hi.is_some() {
            return Err("chunk_terminal");
        }
    } else {
        if rows == 0 {
            return Err("chunk_rows");
        }
        let (lo, hi) = (key_lo.ok_or("chunk_keys")?, key_hi.ok_or("chunk_keys")?);
        if lo > hi || state.last_key_hi.is_some_and(|prev| lo <= prev) {
            return Err("chunk_keys");
        }
    }
    Ok(ChunkFacts {
        qualified,
        chunk_no,
        attempt,
        terminal,
        key_lo,
        key_hi,
        rows,
        part_rel: part_rel.to_owned(),
        part_sha256,
        part_bytes,
    })
}

/// Validates a txn-batch frame: numbering, the position chain, the transaction list,
/// and the row sums.
pub fn validate_batch(
    frame: &Value,
    open: &OpenFacts,
    progress: &Progress,
) -> std::result::Result<BatchFacts, Token> {
    if frame["kind"] != "txn_batch" || frame["protocol"] != PROTO_TXN_BATCH {
        return Err("frame_kind");
    }
    let batch_no = uint(frame, "batch_no")?;
    if batch_no != progress.batch_no + 1 {
        return Err("batch_sequence");
    }
    let first = uint_text(frame, "first_pos")?;
    let last = uint_text(frame, "last_pos")?;
    if first != progress.applied + 1 || last < first {
        return Err("batch_chain");
    }
    let txns = frame["txns"].as_array().ok_or("batch_txns")?;
    if txns.is_empty()
        || txns.len() > MAX_TXNS
        || u64::try_from(txns.len()).map_err(|_| "batch_txns")? != last - first + 1
    {
        return Err("batch_txns");
    }
    let mut txn_rows = 0u64;
    let mut txn_records = Vec::with_capacity(txns.len());
    for (index, txn) in txns.iter().enumerate() {
        let pos = first + index as u64;
        if uint_text(txn, "applied_pos")? != pos {
            return Err("batch_txns");
        }
        let rows = uint(txn, "rows")?;
        txn_rows = txn_rows.checked_add(rows).ok_or("batch_rows")?;
        txn_records.push(json!({
            "applied_pos": pos.to_string(),
            "txn_id": text(txn, "txn_id")?,
            "payload_sha256": hex64(txn, "payload_sha256")?,
            "source_commit_timestamp": text(txn, "source_commit_timestamp")?,
            "row_count": rows,
        }));
    }
    let items = frame["parts"]
        .as_array()
        .filter(|items| items.len() <= MAX_BATCH_PARTS)
        .ok_or("batch_parts")?;
    let mut parts = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    let mut part_rows = 0u64;
    for item in items {
        let qualified = format!("{}.{}", text(item, "schema")?, text(item, "table")?);
        if open.spec(&qualified).is_none() {
            return Err("batch_table");
        }
        if !seen.insert(qualified.clone()) {
            return Err("batch_parts");
        }
        let part_no = uint(item, "part_no")?;
        let last_part = progress.tables.get(&qualified).and_then(|t| t.cdc_part_no);
        if part_no == 0 || last_part.is_some_and(|prev| part_no != prev + 1) {
            return Err("batch_part_no");
        }
        let part_rel = text(item, "part_path")?;
        check_part_rel(part_rel, "cdc-")?;
        let rows = uint(item, "rows")?;
        let part_bytes = uint(item, "part_bytes")?;
        if rows == 0 || part_bytes == 0 {
            return Err("batch_part_rows");
        }
        let (key_lo, key_hi) = (
            opt_key(item, "key_lo")?.ok_or("batch_keys")?,
            opt_key(item, "key_hi")?.ok_or("batch_keys")?,
        );
        if key_lo > key_hi {
            return Err("batch_keys");
        }
        part_rows = part_rows.checked_add(rows).ok_or("batch_rows")?;
        parts.push(PartFacts {
            qualified,
            part_no,
            part_rel: part_rel.to_owned(),
            part_sha256: hex64(item, "part_sha256")?,
            part_bytes,
            rows,
            key_lo,
            key_hi,
        });
    }
    if part_rows != txn_rows {
        return Err("batch_rows");
    }
    Ok(BatchFacts {
        batch_no,
        first,
        last,
        sha256: hex64(frame, "batch_sha256")?,
        rows: part_rows,
        parts,
        txn_records,
    })
}

/// Validates the `end` frame. All tables must be terminal and the position must be the
/// applied position, so the stream has reached `L_end` exactly.
pub fn validate_end(
    frame: &Value,
    open: &OpenFacts,
    progress: &Progress,
) -> std::result::Result<EndFacts, Token> {
    if frame["kind"] != "end" || frame["protocol"] != PROTO_CHUNK {
        return Err("frame_kind");
    }
    if text(frame, "parts_manifest")? != PARTS_MANIFEST {
        return Err("end_manifest");
    }
    let l_end_pos = uint_text(frame, "l_end_pos")?;
    let l_end = text(frame, "l_end")?;
    if l_end != format!("{}:1-{l_end_pos}", open.uuid) {
        return Err("end_lineage");
    }
    if l_end_pos != progress.applied {
        return Err("end_position");
    }
    let all_terminal = open.tables.iter().all(|t| {
        progress
            .tables
            .get(&t.qualified())
            .is_some_and(|state| state.terminal)
    });
    if !all_terminal {
        return Err("end_tables");
    }
    uint(frame, "fill_rows")?;
    uint(frame, "cdc_rows")?;
    Ok(EndFacts {
        l_end: l_end.to_owned(),
        l_end_pos,
        parts_manifest_sha256: hex64(frame, "parts_manifest_sha256")?,
    })
}

/// Validates a `quiet` frame. Returns the no-change request (v2 shape) for the v1 tail.
pub fn validate_quiet(
    frame: &Value,
    source: &str,
    base: i64,
    config_sha256: &str,
    previous: &Value,
) -> std::result::Result<Value, Token> {
    if frame["kind"] != "quiet" {
        return Err("frame_kind");
    }
    let request = frame
        .get("request")
        .filter(|r| r.is_object())
        .ok_or("baseline_quiet")?;
    if request["protocol"] != PROTO_NOCHANGE
        || request["source_id"] != source
        || request["source_generation"].as_i64() != Some(base)
        || request["previous"] != *previous
        || previous.is_null()
        || request["observed_gtid"] != previous["l_end"]
        || request["connection_config_sha256"] != config_sha256
    {
        return Err("baseline_quiet");
    }
    Ok(request.clone())
}

// ---------------------------------------------------------------- progress and resume

/// Resume state built from the loader's `rebind` output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resume {
    /// The loader has no state for this task. Start from the beginning.
    Fresh,
    /// Continue an open run. `bootstrap` is the producer's `resume` object.
    Open {
        bootstrap: Value,
        progress: Progress,
        l0: String,
        l0_pos: u64,
        config_digest: String,
        schema_digest: String,
    },
    /// The run is already sealed (`awaiting_publish`). Only the post-`end` steps remain.
    Sealed { l_end: String, l_end_pos: u64 },
}

/// Builds the producer's `resume` object and the worker's progress from the loader's
/// `rebind` output (`native_c1_baseline_v2_rebind`).
pub fn resume_from_rebind(rebind: &Value) -> std::result::Result<Resume, Token> {
    if rebind["format"] != FORMAT {
        return Err("format_mismatch");
    }
    let l0 = text(rebind, "l0")?;
    let l0_pos = uint_text(rebind, "l0_pos")?;
    let applied = uint_text(rebind, "applied_pos")?;
    match text(rebind, "state")? {
        "open" => {}
        "awaiting_publish" => {
            let l_end = text(rebind, "l_end")?;
            let l_end_pos = uint_text(rebind, "l_end_pos")?;
            return Ok(Resume::Sealed {
                l_end: l_end.to_owned(),
                l_end_pos,
            });
        }
        _ => return Err("resume_state"),
    }
    let mut progress = Progress {
        applied,
        batch_no: rebind["batches"]["last_batch_no"].as_u64().unwrap_or(0),
        tables: BTreeMap::new(),
    };
    let mut tables = Vec::new();
    for item in rebind["tables"].as_array().ok_or("resume_table")? {
        let qualified = format!("{}.{}", text(item, "schema")?, text(item, "table")?);
        let last_chunk_no = uint(item, "last_chunk_no")?;
        let last_key_hi = opt_key(item, "last_key_hi")?;
        let terminal = item["terminal"].as_bool().ok_or("resume_table")?;
        let rows = uint(item, "rows")?;
        if (last_chunk_no == 0) != last_key_hi.is_none() && !terminal {
            return Err("resume_table");
        }
        tables.push(json!({
            "schema": item["schema"], "table": item["table"], "last_chunk_no": last_chunk_no,
            "last_key_hi": key_text(last_key_hi), "terminal": terminal, "rows": rows,
        }));
        progress.tables.insert(
            qualified,
            TableProgress {
                last_chunk_no,
                last_key_hi,
                terminal,
                rows,
                cdc_part_no: None,
            },
        );
    }
    let bootstrap = json!({
        "state": "open", "l0": l0, "l0_pos": l0_pos.to_string(), "applied_pos": applied.to_string(), "tables": tables,
    });
    Ok(Resume::Open {
        bootstrap,
        progress,
        l0: l0.to_owned(),
        l0_pos,
        config_digest: hex64(rebind, "config_digest")?,
        schema_digest: hex64(rebind, "schema_digest")?,
    })
}

// ---------------------------------------------------------------- loader requests

/// Identity and target of one run, as every loader request carries them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stage {
    pub source: String,
    pub task: String,
    pub generation: i64,
    pub epoch: i64,
    pub target: Value,
    pub shard: String,
    pub owner: String,
}

fn envelope(stage: &Stage, operation: &str, fenced: bool) -> Value {
    let mut value = json!({
        "protocol": PROTO_STAGE, "operation": operation, "source_id": stage.source, "lease_id": stage.task,
        "lease_generation": stage.generation, "source_epoch": stage.epoch, "target": stage.target,
    });
    if fenced {
        value["fence"] = json!({"shard": stage.shard, "owner": stage.owner});
    }
    value
}

/// Loader `open`. The type digest binds the typing of every table.
pub fn map_open(stage: &Stage, open: &OpenFacts, source_generation: i64) -> Value {
    let tables: Vec<Value> = open
        .tables
        .iter()
        .map(|t| {
            json!({
                "schema": t.schema, "table": t.table, "table_schema_digest": t.schema_digest,
                "columns": t.columns, "key": t.key,
                "oids": t.typed.iter().map(|c| c.oid).collect::<Vec<_>>(),
                "typmods": t.typed.iter().map(|c| c.typmod).collect::<Vec<_>>(),
                "not_null": t.typed.iter().map(|c| c.not_null).collect::<Vec<_>>(),
            })
        })
        .collect();
    let typing: Vec<Value> = open
        .tables
        .iter()
        .map(|t| json!({"table": t.qualified(), "columns": t.typed.iter().map(|c| json!({"name": c.name, "oid": c.oid, "typmod": c.typmod, "not_null": c.not_null})).collect::<Vec<_>>()}))
        .collect();
    merge(
        envelope(stage, "open", true),
        json!({
            "measured_id": format!("mysql.{}", open.uuid), "config_digest": open.config_digest,
            "schema_digest": open.schema_digest, "type_digest": open.type_digest, "input_digest": open.input_digest,
            "l0": open.l0, "l0_pos": open.l0_pos.to_string(), "source_generation": source_generation,
            "tables": tables,
        }),
    )
}

/// Loader `chunk`. The loader's `key_lo` is the previous chunk's `key_hi` (exclusive),
/// not the frame's smallest key. `part_path` must be absolute.
pub fn map_chunk(
    stage: &Stage,
    chunk: &ChunkFacts,
    spec: &TableSpec,
    previous_key_hi: Option<i128>,
    part: &Path,
) -> Value {
    merge(
        envelope(stage, "chunk", false),
        json!({
            "schema": spec.schema, "table": spec.table, "chunk_no": chunk.chunk_no, "attempt": chunk.attempt,
            "key_lo": key_text(previous_key_hi), "key_hi": key_text(chunk.key_hi), "terminal": chunk.terminal,
            "row_count": chunk.rows, "part_sha256": chunk.part_sha256, "columns": spec.columns, "key": spec.key,
            "part": {
                "path": part.to_string_lossy(), "bytes": chunk.part_bytes, "sha256": chunk.part_sha256,
                "rows": chunk.rows, "key_lo": key_text(chunk.key_lo), "key_hi": key_text(chunk.key_hi),
            },
        }),
    )
}

/// Loader `txn_batch`. Parts are absolute paths. Row sums are the frame's sums.
pub fn map_batch(
    stage: &Stage,
    batch: &BatchFacts,
    open: &OpenFacts,
    parts_dir: &Path,
) -> std::result::Result<Value, Token> {
    let mut items = Vec::with_capacity(batch.parts.len());
    for part in &batch.parts {
        let spec = open.spec(&part.qualified).ok_or("batch_table")?;
        items.push(json!({
            "schema": spec.schema, "table": spec.table, "columns": spec.columns, "key": spec.key,
            "part": {
                "path": parts_dir.join(&part.part_rel).to_string_lossy(), "bytes": part.part_bytes,
                "sha256": part.part_sha256, "rows": part.rows,
                "key_lo": part.key_lo.to_string(), "key_hi": part.key_hi.to_string(),
            },
        }));
    }
    Ok(merge(
        envelope(stage, "txn_batch", false),
        json!({
            "batch_no": batch.batch_no, "first_pos": batch.first.to_string(), "last_pos": batch.last.to_string(),
            "txns": batch.txn_records.len(), "row_count": batch.rows, "batch_sha256": batch.sha256,
            "txn_records": batch.txn_records, "parts": items,
        }),
    ))
}

/// Loader `rebind` (fenced).
pub fn map_rebind(stage: &Stage) -> Value {
    envelope(stage, "rebind", true)
}

/// Loader `retire` (fenced). Abandons the run.
pub fn map_retire(stage: &Stage) -> Value {
    envelope(stage, "retire", true)
}

// ---------------------------------------------------------------- retry policy

/// What to do after a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Wait, then try again.
    Retry(Duration),
    /// Give up on this run with this closed token. The caller retires the run.
    Stop(Token),
    /// The lease is gone. Stop without retiring.
    Lost,
}

/// Backoff for attempt `n` (1-based): 1 s, 2 s, 4 s, … capped at 30 s.
pub fn backoff(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(16);
    Duration::from_secs((1u64 << shift).min(MAX_BACKOFF_SECS))
}

/// Producer exit policy. Exit 4 (connect or TLS) and 8 (protocol or value) are
/// transient. A killed producer (no exit code) is a crash, and is retried too. Every
/// other code is deterministic and stops the run.
pub fn producer_retry(code: Option<i32>, attempt: u32) -> Next {
    let transient = matches!(code, None | Some(4) | Some(8));
    if !transient {
        Next::Stop("producer_failed")
    } else if attempt < MAX_PRODUCER_ATTEMPTS {
        Next::Retry(backoff(attempt))
    } else {
        Next::Stop("producer_retries")
    }
}

/// What the driver does after a producer exit. A new round is a fresh rebind and a new producer
/// (the loader run stays open). The round pause is the longest back-off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Retry(Duration),
    NewRound(Duration),
    Stop(Token),
}

/// Policy after a producer exit. `round_attempt` counts spawns in this round (1-based),
/// `round` is the round index (0-based). Only when a round's attempts are spent, and no
/// rounds are left, does the claim stop with `producer_retries`.
pub fn producer_step(code: Option<i32>, round_attempt: u32, round: u32) -> Step {
    match producer_retry(code, round_attempt) {
        Next::Retry(delay) => Step::Retry(delay),
        Next::Stop("producer_retries") if round + 1 < MAX_PRODUCER_ROUNDS => {
            Step::NewRound(Duration::from_secs(MAX_BACKOFF_SECS))
        }
        Next::Stop(token) => Step::Stop(token),
        Next::Lost => Step::Stop("producer_failed"),
    }
}

/// Loader state that survives a failed claim. A producer that exhausted its retry budget
/// is transient: the run stays open, so the next claim rebinds and resumes from its
/// ledger. Every other failure retires the run (N1), because a retired run cannot resume.
pub fn keeps_loader_state(token: &str) -> bool {
    token == "producer_retries"
}

/// Loader refusal policy. `target_apply` (a connection or statement error) and a
/// transport failure are replayed, which is safe because every loader write is
/// idempotent. `lease_or_barrier` means the lease is gone. Everything else is final.
pub fn loader_retry(token: &str, attempt: u32) -> Next {
    match token {
        "lease_or_barrier" => Next::Lost,
        "target_apply" | "transport" if attempt < MAX_LOADER_ATTEMPTS => {
            Next::Retry(backoff(attempt))
        }
        other => Next::Stop(loader_token(other)),
    }
}

// ---------------------------------------------------------------- ledger

/// Append-only frame ledger in the producer out directory. One small line per frame:
/// the frame's digest and the loader outcome. The raw frame is not kept. The chain
/// digest links every line, so the receipt can bind the whole stream in 64 bytes.
#[derive(Debug)]
pub struct Ledger {
    file: std::fs::File,
    seq: u64,
    chain: String,
}

/// Opens (or creates) a file for appending, readable by the owner only.
fn private_append(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

impl Ledger {
    pub fn open(out: &Path) -> std::result::Result<Self, Token> {
        let file = private_append(&out.join(LEDGER_FILE)).map_err(|_| "baseline_ledger")?;
        Ok(Self {
            file,
            seq: 0,
            chain: "0".repeat(64),
        })
    }

    /// Records one acknowledged frame. `note` must be small.
    pub fn record(&mut self, raw: &[u8], note: Value) -> std::result::Result<(), Token> {
        let digest = sha256_hex(raw);
        self.chain = sha256_hex(format!("{}{digest}", self.chain).as_bytes());
        self.seq += 1;
        let line = merge(json!({"seq": self.seq, "frame_sha256": digest}), note);
        let mut bytes = serde_json::to_vec(&line).map_err(|_| "baseline_ledger")?;
        bytes.push(b'\n');
        self.file.write_all(&bytes).map_err(|_| "baseline_ledger")
    }

    /// Records one loader refusal: the worker's token and the loader's own reason (bounded).
    /// A refusal is not a frame, so the chain and the frame count do not change.
    pub fn refusal(&mut self, token: &str, reason: &str) -> std::result::Result<(), Token> {
        let line = json!({"kind": "loader_refusal", "after_seq": self.seq, "token": token, "reason": reason});
        let mut bytes = serde_json::to_vec(&line).map_err(|_| "baseline_ledger")?;
        bytes.push(b'\n');
        self.file.write_all(&bytes).map_err(|_| "baseline_ledger")
    }

    pub fn chain(&self) -> &str {
        &self.chain
    }

    pub fn frames(&self) -> u64 {
        self.seq
    }
}

// ---------------------------------------------------------------- driver

/// Everything the driver needs to know about one run, decided before the first attempt.
#[derive(Clone, Debug)]
pub struct Plan {
    pub source: String,
    pub base: i64,
    /// Extra options for the producer's `baseline` object. Production sends none, so the
    /// producer's defaults apply. Tests may set `chunk_rows` and the like.
    pub baseline: Value,
    pub producer: Executable,
    pub out: PathBuf,
    pub task: String,
    pub config_sha256: String,
    pub previous: Value,
    pub typing: Typing,
}

/// The result of a run that reached `end`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub end: EndFacts,
    pub source_epoch: i64,
    pub frames: u64,
    pub chain_sha256: String,
    pub attempts: u32,
    pub chunks: u64,
    pub batches: u64,
    pub tables: Vec<String>,
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Done {
    /// The producer reached `end`, and every frame is loaded.
    Ended(Summary),
    /// The loader was already sealed. Only the post-`end` steps remain.
    Sealed(EndFacts),
    /// Nothing changed since the previous run. The request is the v2 no-change request.
    Quiet(Value),
}

/// Calls the loader and the lease. Production uses [`ProdPort`]. Tests use a local port.
#[allow(async_fn_in_trait)]
pub trait Port {
    /// The loader's `rebind`, or `None` when the task has no loader state.
    async fn rebind(&mut self, stage: &Stage) -> std::result::Result<Option<Value>, Fault>;
    /// Registers the source instance and returns its epoch (≥ 1).
    async fn register(
        &mut self,
        stage: &Stage,
        open: &OpenFacts,
    ) -> std::result::Result<i64, Fault>;
    /// One stage operation. `Ok` carries the loader's receipt (outcome not `refused`).
    async fn call(&mut self, operation: Value) -> std::result::Result<Value, Fault>;
    /// Renews the lease. `Err(Fault::Lost)` when it is gone.
    async fn beat(&mut self) -> std::result::Result<(), Fault>;
    /// Told the producer's pid after each spawn. Tests use it to kill the producer.
    fn producer_started(&mut self, _pid: u32) {}
    /// The loader's own refusal reason for the last refused call (bounded), taken once.
    fn take_refusal(&mut self) -> Option<String> {
        None
    }
}

/// Runs the stream and, when the run fails for good, retires its loader state (N1).
pub async fn run<P: Port>(
    port: &mut P,
    plan: &Plan,
    stage: Stage,
) -> std::result::Result<Done, Fault> {
    let mut driver = Driver {
        port,
        plan,
        stage,
        ledger: None,
        chunks: 0,
        batches: 0,
        attempts: 0,
        refusals: 0,
    };
    let result = driver.stream().await;
    if let Err(Fault::Token(token)) = &result {
        if driver.stage.epoch > 0 && !keeps_loader_state(token) {
            let _ = driver.port.call(map_retire(&driver.stage)).await;
        }
    }
    result
}

enum AttemptError {
    Lost,
    Producer(Option<i32>),
    Fatal(Token),
}

impl From<Fault> for AttemptError {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Lost => AttemptError::Lost,
            Fault::Token(token) => AttemptError::Fatal(token),
        }
    }
}

struct Driver<'a, P: Port> {
    port: &'a mut P,
    plan: &'a Plan,
    stage: Stage,
    ledger: Option<Ledger>,
    chunks: u64,
    batches: u64,
    attempts: u32,
    refusals: u32,
}

/// Per-attempt state. A new attempt starts from a fresh rebind.
struct Session {
    resumed: bool,
    open: Option<OpenFacts>,
    progress: Progress,
    identity: Option<(String, u64, String, String)>,
    started: bool,
}

enum Flow {
    Continue,
    End(EndFacts),
    Quiet(Value),
}

impl<P: Port> Driver<'_, P> {
    async fn stream(&mut self) -> std::result::Result<Done, Fault> {
        // A round is one run of producer attempts. A new round starts from a fresh rebind.
        let mut round = 0u32;
        let mut round_attempt = 0u32;
        loop {
            self.attempts += 1;
            round_attempt += 1;
            let rebind = self.port.rebind(&self.stage).await?;
            let resume = match rebind {
                None => Resume::Fresh,
                Some(value) => resume_from_rebind(&value).map_err(Fault::Token)?,
            };
            if let Resume::Sealed { l_end, l_end_pos } = &resume {
                let end = EndFacts {
                    l_end: l_end.clone(),
                    l_end_pos: *l_end_pos,
                    parts_manifest_sha256: manifest_sha(&self.plan.out)?,
                };
                return Ok(Done::Sealed(end));
            }
            if matches!(resume, Resume::Fresh) {
                prepare_fresh_out(&self.plan.out, &self.plan.task)?;
            }
            let bootstrap = bootstrap_json(self.plan, &resume);
            match self.attempt(bootstrap, resume).await {
                Ok(done) => return Ok(done),
                Err(AttemptError::Lost) => return Err(Fault::Lost),
                Err(AttemptError::Fatal(token)) => return Err(Fault::Token(token)),
                Err(AttemptError::Producer(code)) => match producer_step(code, round_attempt, round) {
                    Step::Retry(delay) => {
                        self.sleep_beating(delay).await.map_err(|_| Fault::Lost)?;
                    }
                    Step::NewRound(pause) => {
                        round += 1;
                        round_attempt = 0;
                        self.sleep_beating(pause).await.map_err(|_| Fault::Lost)?;
                    }
                    Step::Stop(token) => return Err(Fault::Token(token)),
                },
            }
        }
    }

    async fn sleep_beating(&mut self, delay: Duration) -> std::result::Result<(), Fault> {
        let until = Instant::now() + delay;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            tokio::time::sleep(left.min(HEARTBEAT)).await;
            self.port.beat().await?;
        }
    }

    /// One loader call, replayed on transient refusals.
    async fn call_loader(&mut self, operation: Value) -> std::result::Result<Value, AttemptError> {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match self.port.call(operation.clone()).await {
                Ok(value) => return Ok(value),
                Err(Fault::Lost) => return Err(AttemptError::Lost),
                Err(Fault::Token(token)) => {
                    self.record_refusal(token);
                    match loader_retry(token, attempt) {
                        Next::Retry(delay) => self
                            .sleep_beating(delay)
                            .await
                            .map_err(|_| AttemptError::Lost)?,
                        Next::Stop(stop) => return Err(AttemptError::Fatal(stop)),
                        Next::Lost => return Err(AttemptError::Lost),
                    }
                }
            }
        }
    }

    /// Writes one loader refusal to the ledger, bounded per claim. Only a refusal that the loader
    /// itself gave is recorded, with its own reason. Transport and lease failures are not. The
    /// write is best effort: it never changes the outcome.
    fn record_refusal(&mut self, token: Token) {
        let Some(reason) = self.port.take_refusal() else {
            return;
        };
        if self.refusals >= MAX_REFUSAL_RECORDS {
            return;
        }
        self.refusals += 1;
        if self.ledger.is_none() {
            self.ledger = Ledger::open(&self.plan.out).ok();
        }
        if let Some(ledger) = self.ledger.as_mut() {
            let _ = ledger.refusal(token, &reason);
        }
    }

    async fn attempt(
        &mut self,
        bootstrap: Value,
        resume: Resume,
    ) -> std::result::Result<Done, AttemptError> {
        let mut session = match resume {
            Resume::Fresh => Session {
                resumed: false,
                open: None,
                progress: Progress::default(),
                identity: None,
                started: false,
            },
            Resume::Open {
                progress,
                l0,
                l0_pos,
                config_digest,
                schema_digest,
                ..
            } => Session {
                resumed: true,
                open: None,
                progress,
                identity: Some((l0, l0_pos, config_digest, schema_digest)),
                started: false,
            },
            Resume::Sealed { .. } => return Err(AttemptError::Fatal("resume_state")),
        };
        let producer = &self.plan.producer;
        let mut child = Command::new(&producer.path)
            .args(&producer.args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| AttemptError::Fatal("baseline_spawn"))?;
        if let Some(pid) = child.id() {
            self.port.producer_started(pid);
        }
        let mut stdin = child
            .stdin
            .take()
            .ok_or(AttemptError::Fatal("baseline_spawn"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(AttemptError::Fatal("baseline_spawn"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or(AttemptError::Fatal("baseline_spawn"))?;
        let (tx, mut rx) = mpsc::channel::<std::io::Result<Vec<u8>>>(2);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line).await {
                    Ok(0) => break,
                    Ok(_) if line.len() > MAX_FRAME_BYTES => {
                        let _ = tx.send(Err(std::io::Error::other("frame too large"))).await;
                        break;
                    }
                    Ok(_) => {
                        if tx.send(Ok(line)).await.is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(Err(err)).await;
                        break;
                    }
                }
            }
        });
        let stderr_path = self.plan.out.join(PRODUCER_STDERR);
        tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr.take(65537).read_to_end(&mut bytes).await;
            // Human-readable only. The worker never parses it, and it is not stored in the DB.
            if let Ok(mut file) = private_append(&stderr_path) {
                let _ = file.write_all(&bytes);
            }
        });
        let mut line_bytes = bootstrap.to_string().into_bytes();
        line_bytes.push(b'\n');
        if stdin.write_all(&line_bytes).await.is_err() || stdin.flush().await.is_err() {
            let status = child.wait().await.ok().and_then(|s| s.code());
            return Err(AttemptError::Producer(status));
        }

        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + HEARTBEAT, HEARTBEAT);
        let mut finished: Option<Flow> = None;
        loop {
            let item = tokio::select! {
                item = rx.recv() => item,
                _ = tick.tick() => {
                    self.port.beat().await.map_err(|_| AttemptError::Lost)?;
                    continue;
                }
            };
            let raw = match item {
                Some(Ok(raw)) => raw,
                Some(Err(_)) => return Err(AttemptError::Fatal("baseline_stdout")),
                None => break,
            };
            let body = raw.strip_suffix(b"\n").unwrap_or(&raw);
            let frame: Value =
                serde_json::from_slice(body).map_err(|_| AttemptError::Fatal("baseline_frame"))?;
            let (flow, reply) = self.handle(&frame, body, &mut session).await?;
            if let Some(reply) = reply {
                let mut bytes = reply.to_string().into_bytes();
                bytes.push(b'\n');
                if stdin.write_all(&bytes).await.is_err() || stdin.flush().await.is_err() {
                    let status = child.wait().await.ok().and_then(|s| s.code());
                    return Err(AttemptError::Producer(status));
                }
            }
            match flow {
                Flow::Continue => {}
                other => {
                    finished = Some(other);
                    break;
                }
            }
        }
        drop(stdin);
        let status = child
            .wait()
            .await
            .map_err(|_| AttemptError::Fatal("baseline_wait"))?;
        if !status.success() {
            return Err(AttemptError::Producer(status.code()));
        }
        match finished {
            Some(Flow::End(end)) => {
                let open = session
                    .open
                    .as_ref()
                    .ok_or(AttemptError::Fatal("baseline_order"))?;
                let summary = Summary {
                    end,
                    source_epoch: self.stage.epoch,
                    frames: self.ledger.as_ref().map_or(0, Ledger::frames),
                    chain_sha256: self
                        .ledger
                        .as_ref()
                        .map_or_else(|| "0".repeat(64), |l| l.chain().to_owned()),
                    attempts: self.attempts,
                    chunks: self.chunks,
                    batches: self.batches,
                    tables: open.tables.iter().map(TableSpec::qualified).collect(),
                };
                Ok(Done::Ended(summary))
            }
            Some(Flow::Quiet(request)) => Ok(Done::Quiet(request)),
            _ => Err(AttemptError::Fatal("producer_incomplete")),
        }
    }
}

fn ack(stage: &str, extra: Value) -> Value {
    merge(
        json!({"protocol": PROTO_ACK, "stage": stage, "durable": true}),
        extra,
    )
}

fn manifest_sha(out: &Path) -> std::result::Result<String, Token> {
    let path = out.join(PARTS_MANIFEST);
    let size = std::fs::metadata(&path).map_err(|_| "end_manifest")?.len();
    if size > MAX_MANIFEST_BYTES {
        return Err("end_manifest");
    }
    let bytes = std::fs::read(&path).map_err(|_| "end_manifest")?;
    Ok(sha256_hex(&bytes))
}

/// A fresh run may start only when the out directory is absent, or when it lives under
/// this task's id and the loader holds no state for the task. In that case nothing was
/// acknowledged, so the earlier scratch directory is removed.
fn prepare_fresh_out(out: &Path, task: &str) -> std::result::Result<(), Token> {
    if !out.exists() {
        return Ok(());
    }
    let owned = out.is_absolute()
        && out
            .components()
            .any(|c| c.as_os_str().to_str().is_some_and(|s| s.contains(task)));
    if !owned {
        return Err("baseline_out_exists");
    }
    std::fs::remove_dir_all(out).map_err(|_| "baseline_out_exists")
}

fn bootstrap_json(plan: &Plan, resume: &Resume) -> Value {
    let resume = match resume {
        Resume::Open { bootstrap, .. } => bootstrap.clone(),
        _ => Value::Null,
    };
    json!({
        "protocol": PROTO_BOOTSTRAP, "source_id": plan.source, "source_generation": plan.base + 1,
        "baseline": merge(json!({"format": FORMAT}), plan.baseline.clone()), "read_source": {"kind": "primary"},
        "resume": resume, "previous": plan.previous,
    })
}

/// The loader's refusal token, bounded and plain, for the step log. Unknown tokens are
/// otherwise reported only as `loader_refused`.
pub fn refusal_evidence(raw: &str) -> String {
    let plain: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
        .take(64)
        .collect();
    if plain.is_empty() { "empty".to_owned() } else { plain }
}

/// Loader tokens the worker knows. Any other refusal is reported as `loader_refused`.
fn loader_token(reason: &str) -> Token {
    const KNOWN: &[Token] = &[
        "lease_or_barrier",
        "closed",
        "not_ready",
        "permission",
        "target_apply",
        "transport",
        "format_mismatch",
        "chunk_conflict",
        "chunk_sequence",
        "chunk_keys",
        "chunk_part_rows",
        "invalid_chunk",
        "batch_conflict",
        "batch_sequence",
        "batch_rows",
        "batch_txns",
        "invalid_batch",
        "tombstone_overflow",
        "part_sha256",
        "part_too_large",
        "part_keys",
        "part_schema",
        "part_path",
        "part_bytes",
        "open_shape",
        "duplicate_table",
        "fence",
        "request_field",
        "request_json",
        "protocol",
        "operation",
        "target",
        "digest",
        "position",
        "physical",
        "closed_baseline_table",
        "closed_baseline_key",
        "table_identity_conflict",
        // Loader tokens from `native_c1_apply::sql_error`. `run_not_open` is what a retired run
        // answers (OO031), so a resume after a retire shows its real cause, not `loader_refused`.
        "run_not_open",
        "physical_mapping_absent",
        "fence_held",
        "read_budget",
        "table_unregistered",
        "seal_conflict",
        "seal_tables",
        "composed_row_count",
        "snapshot_incomplete",
    ];
    KNOWN
        .iter()
        .copied()
        .find(|known| *known == reason)
        .unwrap_or("loader_refused")
}

impl<P: Port> Driver<'_, P> {
    /// Validates one producer frame, loads it, records it, and returns the ack to write.
    async fn handle(
        &mut self,
        frame: &Value,
        raw: &[u8],
        session: &mut Session,
    ) -> std::result::Result<(Flow, Option<Value>), AttemptError> {
        let kind = frame.get("kind").and_then(Value::as_str).unwrap_or("");
        match kind {
            "open" => {
                if session.open.is_some() {
                    return Err(AttemptError::Fatal("frame_order"));
                }
                let open = validate_open(
                    frame,
                    &self.plan.source,
                    &self.plan.config_sha256,
                    &self.plan.typing,
                    session.resumed,
                )
                .map_err(AttemptError::Fatal)?;
                if let Some((l0, l0_pos, config_digest, schema_digest)) = &session.identity {
                    if open.l0 != *l0
                        || open.l0_pos != *l0_pos
                        || open.config_digest != *config_digest
                        || open.schema_digest != *schema_digest
                    {
                        return Err(AttemptError::Fatal("baseline_resume_identity"));
                    }
                    if open.applied != session.progress.applied {
                        return Err(AttemptError::Fatal("baseline_resume_applied"));
                    }
                }
                let epoch = self.port.register(&self.stage, &open).await?;
                if epoch < 1 {
                    return Err(AttemptError::Fatal("source_epoch"));
                }
                self.stage.epoch = epoch;
                let mut operation = map_open(&self.stage, &open, self.plan.base + 1);
                if !self.plan.previous.is_null() {
                    operation["previous_task"] = self.plan.previous["task_id"].clone();
                }
                let receipt = self.call_loader(operation).await?;
                if !matches!(receipt["outcome"].as_str(), Some("opened" | "replay")) {
                    return Err(AttemptError::Fatal("baseline_target_open"));
                }
                if !session.resumed {
                    session.progress.applied = open.applied;
                }
                session.open = Some(open);
                self.note(raw, json!({"kind": "open", "outcome": receipt["outcome"]}))?;
                Ok((
                    Flow::Continue,
                    Some(ack("stage_open", json!({"source_epoch": epoch}))),
                ))
            }
            "stream_started" => {
                let open = session
                    .open
                    .as_ref()
                    .ok_or(AttemptError::Fatal("frame_order"))?;
                if session.started
                    || text(frame, "l0").ok() != Some(open.l0.as_str())
                    || uint_text(frame, "applied_pos").ok() != Some(session.progress.applied)
                {
                    return Err(AttemptError::Fatal("baseline_stream"));
                }
                session.started = true;
                self.note(raw, json!({"kind": "stream_started"}))?;
                Ok((Flow::Continue, Some(ack("stream_started", json!({})))))
            }
            "chunk" => {
                let open = session
                    .open
                    .clone()
                    .ok_or(AttemptError::Fatal("frame_order"))?;
                if !session.started {
                    return Err(AttemptError::Fatal("frame_order"));
                }
                let facts =
                    validate_chunk(frame, &open, &session.progress).map_err(AttemptError::Fatal)?;
                let spec = open
                    .spec(&facts.qualified)
                    .ok_or(AttemptError::Fatal("chunk_table"))?
                    .clone();
                let previous = session
                    .progress
                    .tables
                    .get(&facts.qualified)
                    .and_then(|t| t.last_key_hi);
                let part = self.plan.out.join(&facts.part_rel);
                let receipt = self
                    .call_loader(map_chunk(&self.stage, &facts, &spec, previous, &part))
                    .await?;
                let outcome = match receipt["outcome"].as_str() {
                    Some(outcome @ ("loaded" | "replay")) => outcome,
                    _ => return Err(AttemptError::Fatal("baseline_chunk_apply")),
                };
                if receipt["chunk_no"]
                    .as_u64()
                    .is_some_and(|n| n != facts.chunk_no)
                {
                    return Err(AttemptError::Fatal("baseline_chunk_apply"));
                }
                let state = session
                    .progress
                    .tables
                    .entry(facts.qualified.clone())
                    .or_default();
                state.last_chunk_no = facts.chunk_no;
                if !facts.terminal {
                    state.last_key_hi = facts.key_hi;
                }
                state.terminal = facts.terminal;
                state.rows += facts.rows;
                self.chunks += 1;
                self.note(raw, json!({"kind": "chunk", "table": facts.qualified, "chunk_no": facts.chunk_no, "outcome": outcome}))?;
                Ok((
                    Flow::Continue,
                    Some(ack(
                        "chunk_apply",
                        json!({"outcome": outcome, "chunk_no": facts.chunk_no,
                        "schema": frame["schema"], "table": frame["table"]}),
                    )),
                ))
            }
            "txn_batch" => {
                let open = session
                    .open
                    .clone()
                    .ok_or(AttemptError::Fatal("frame_order"))?;
                if !session.started {
                    return Err(AttemptError::Fatal("frame_order"));
                }
                let facts =
                    validate_batch(frame, &open, &session.progress).map_err(AttemptError::Fatal)?;
                let operation = map_batch(&self.stage, &facts, &open, &self.plan.out)
                    .map_err(AttemptError::Fatal)?;
                let receipt = self.call_loader(operation).await?;
                let outcome = match receipt["outcome"].as_str() {
                    Some(outcome @ ("applied" | "replay")) => outcome,
                    _ => return Err(AttemptError::Fatal("baseline_batch_apply")),
                };
                let last = facts.last.to_string();
                if receipt["applied_pos"].as_str() != Some(last.as_str()) {
                    return Err(AttemptError::Fatal("baseline_batch_apply"));
                }
                session.progress.applied = facts.last;
                session.progress.batch_no = facts.batch_no;
                for part in &facts.parts {
                    session
                        .progress
                        .tables
                        .entry(part.qualified.clone())
                        .or_default()
                        .cdc_part_no = Some(part.part_no);
                }
                self.batches += 1;
                self.note(
                    raw,
                    json!({"kind": "txn_batch", "batch_no": facts.batch_no, "outcome": outcome}),
                )?;
                Ok((
                    Flow::Continue,
                    Some(ack(
                        "txn_batch",
                        json!({"outcome": outcome, "batch_no": facts.batch_no, "last_pos": last}),
                    )),
                ))
            }
            "end" => {
                let open = session
                    .open
                    .clone()
                    .ok_or(AttemptError::Fatal("frame_order"))?;
                if !session.started {
                    return Err(AttemptError::Fatal("frame_order"));
                }
                let end =
                    validate_end(frame, &open, &session.progress).map_err(AttemptError::Fatal)?;
                if manifest_sha(&self.plan.out).map_err(AttemptError::Fatal)?
                    != end.parts_manifest_sha256
                {
                    return Err(AttemptError::Fatal("end_manifest_sha"));
                }
                self.note(raw, json!({"kind": "end", "l_end_pos": end.l_end_pos}))?;
                Ok((
                    Flow::End(end),
                    Some(ack("end", json!({"outcome": "applied"}))),
                ))
            }
            "quiet" => {
                if session.open.is_some() || self.ledger.is_some() {
                    return Err(AttemptError::Fatal("frame_order"));
                }
                let request = validate_quiet(
                    frame,
                    &self.plan.source,
                    self.plan.base,
                    &self.plan.config_sha256,
                    &self.plan.previous,
                )
                .map_err(AttemptError::Fatal)?;
                Ok((Flow::Quiet(request), None))
            }
            _ => Err(AttemptError::Fatal("frame_kind")),
        }
    }

    /// Appends one ledger line. The ledger exists from the `open` frame onwards.
    fn note(&mut self, raw: &[u8], note: Value) -> std::result::Result<(), AttemptError> {
        if self.ledger.is_none() {
            self.ledger = Some(Ledger::open(&self.plan.out).map_err(AttemptError::Fatal)?);
        }
        let ledger = self
            .ledger
            .as_mut()
            .ok_or(AttemptError::Fatal("baseline_ledger"))?;
        ledger.record(raw, note).map_err(AttemptError::Fatal)
    }
}

// ---------------------------------------------------------------- production port

/// The production port: loader subprocesses, the target connection, and the lease.
struct ProdPort<'a> {
    control: &'a tokio_postgres::Client,
    target: tokio_postgres::Client,
    admin: tokio_postgres::Client,
    project: &'a str,
    worker: &'a str,
    task: &'a str,
    generation: i64,
    loader: Executable,
    refusal: Option<String>,
}

impl Port for ProdPort<'_> {
    async fn rebind(&mut self, stage: &Stage) -> std::result::Result<Option<Value>, Fault> {
        // Every loader request needs a positive epoch. An existing run has one stored, and
        // the rebind carries it. A task with no format row has no loader state yet.
        let row = self
            .admin
            .query_opt(
                "SELECT r.source_epoch::bigint FROM _oo.native_c1_baseline_v2_format f \
                 JOIN _oo.native_c1_baseline_run r ON r.source=f.source AND r.task_id=f.task_id \
                 WHERE f.source=$1 AND f.task_id=$2::text::uuid",
                &[&stage.source, &stage.task],
            )
            .await
            .map_err(|_| Fault::Token("sql"))?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut current = stage.clone();
        current.epoch = row.get::<_, i64>(0);
        self.call(map_rebind(&current)).await.map(Some)
    }

    async fn register(
        &mut self,
        stage: &Stage,
        open: &OpenFacts,
    ) -> std::result::Result<i64, Fault> {
        let text: String = self
            .target
            .query_one(
                "SELECT _oo.native_source_instance_register($1,$2::text::uuid,$3,$4,$5,$6,$7,$8,$9::text::uuid)::text",
                &[&stage.source, &stage.owner, &stage.generation, &stage.source, &"mysql", &format!("mysql.{}", open.uuid),
                  &open.config_digest, &open.schema_digest, &stage.task],
            )
            .await
            .map_err(|_| Fault::Token("source_instance"))?
            .get(0);
        let instance: Value =
            serde_json::from_str(&text).map_err(|_| Fault::Token("source_instance"))?;
        instance["source_epoch"]
            .as_i64()
            .filter(|epoch| *epoch > 0)
            .ok_or(Fault::Token("source_epoch"))
    }

    async fn call(&mut self, operation: Value) -> std::result::Result<Value, Fault> {
        self.refusal = None;
        let input = serde_json::to_vec(&operation).map_err(|_| Fault::Token("request_json"))?;
        let mut child = Command::new(&self.loader.path)
            .args(&self.loader.args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| Fault::Token("transport"))?;
        let mut stdin = child.stdin.take().ok_or(Fault::Token("transport"))?;
        let stdout = child.stdout.take().ok_or(Fault::Token("transport"))?;
        let stderr = child.stderr.take().ok_or(Fault::Token("transport"))?;
        let writer = tokio::spawn(async move {
            stdin.write_all(&input).await?;
            stdin.shutdown().await
        });
        let reader = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stdout
                .take(MAX_LOADER_OUTPUT as u64 + 1)
                .read_to_end(&mut bytes)
                .await
                .map(|_| bytes)
        });
        tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr.take(65537).read_to_end(&mut bytes).await;
        });
        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + HEARTBEAT, HEARTBEAT);
        let status = loop {
            tokio::select! {
                status = child.wait() => break status.map_err(|_| Fault::Token("transport"))?,
                _ = tick.tick() => self.beat().await?,
            }
        };
        let _ = writer.await;
        let bytes = reader
            .await
            .map_err(|_| Fault::Token("transport"))?
            .map_err(|_| Fault::Token("transport"))?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| Fault::Token("transport"))?;
        if value["outcome"].as_str() == Some("refused") {
            let raw = value["reason"].as_str().unwrap_or("");
            let evidence = refusal_evidence(raw);
            println!("loader_refusal reason={evidence}");
            self.refusal = Some(evidence);
            return Err(Fault::Token(loader_token(raw)));
        }
        if !status.success() {
            return Err(Fault::Token("transport"));
        }
        Ok(value)
    }

    fn take_refusal(&mut self) -> Option<String> {
        self.refusal.take()
    }

    async fn beat(&mut self) -> std::result::Result<(), Fault> {
        let control = lease::heartbeat(
            self.control,
            self.project,
            self.task,
            self.worker,
            self.generation,
        )
        .await
        .map_err(|_| Fault::Token("heartbeat"))?;
        if control.status != LeaseStatus::Extended {
            return Err(Fault::Lost);
        }
        let target = lease::heartbeat(
            &self.target,
            self.project,
            self.task,
            self.worker,
            self.generation,
        )
        .await
        .map_err(|_| Fault::Token("heartbeat"))?;
        if target.status != LeaseStatus::Extended {
            return Err(Fault::Lost);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- post-end steps

/// The steps after the producer's `end`: materialize v2 and admission v2 (T10), which
/// produce the final-load request (T8). The final load itself runs through the v1
/// publication tail, `apply_cdc_baseline`.
#[allow(async_fn_in_trait)]
pub trait PostEnd {
    async fn request(&mut self, out: &Path, end: &EndFacts) -> std::result::Result<Value, Token>;
}

/// Stops the run in the seal-ready state. Not used in production. Tests use it to stop
/// before the final load.
pub struct Pending;

impl PostEnd for Pending {
    async fn request(&mut self, _out: &Path, _end: &EndFacts) -> std::result::Result<Value, Token> {
        Err("final_load_pending")
    }
}

/// File in the producer out directory that carries the worker's facts to the materializer.
const MATERIALIZE_CONTEXT: &str = "materialize-context.json";

/// Production post-`end` step. It runs the producer binary's `c1-materialize-v2`
/// subcommand (T10). The subcommand seals each table's admission and prints the
/// final-load request (the T8 document). This step checks that the request names the same
/// L_end as the producer's `end`, and adds the lease and fence fields.
pub struct Materialize {
    /// The producer binary, which carries the subcommand. Its digest was checked at claim.
    pub producer: Executable,
    /// The worker's facts: source id and generation, admission binary and digest, binding,
    /// pin, target, and source epoch. Written to the out directory with mode 0600.
    pub context: Value,
    /// Lease and fence fields that the final load checks (`lease_id`, `lease_generation`, `fence`).
    pub lease: Value,
}

impl PostEnd for Materialize {
    async fn request(&mut self, out: &Path, end: &EndFacts) -> std::result::Result<Value, Token> {
        let context_path = out.join(MATERIALIZE_CONTEXT);
        let bytes = serde_json::to_vec(&self.context).map_err(|_| "materialize_context")?;
        std::fs::write(&context_path, bytes).map_err(|_| "materialize_context")?;
        private_mode(&context_path).map_err(|_| "materialize_context")?;
        let output = Command::new(&self.producer.path)
            .arg("c1-materialize-v2")
            .arg("--out")
            .arg(out)
            .arg("--context")
            .arg(&context_path)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|_| "materialize_spawn")?;
        if !output.status.success() {
            return Err("materialize_failed");
        }
        if output.stdout.len() > MAX_LOADER_OUTPUT {
            return Err("materialize_output");
        }
        let mut request: Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "materialize_output")?;
        if request["protocol"] != REQUEST_PROTOCOL
            || request["l_end"].as_str() != Some(end.l_end.as_str())
            || request["l_end_pos"].as_str() != Some(end.l_end_pos.to_string().as_str())
        {
            return Err("materialize_end");
        }
        for (key, value) in self.lease.as_object().ok_or("materialize_context")? {
            request[key] = value.clone();
        }
        Ok(request)
    }
}

/// The production post-`end` step for one run. The admission binary is the one the claim
/// registered. The target is the loader's target, so the request names the same shard and owner.
#[allow(clippy::too_many_arguments)]
fn post_end_for(
    m: &Manifest,
    task: &str,
    generation: i64,
    binding: &str,
    pin: i64,
    target: &Value,
    epoch: i64,
    base: i64,
) -> Result<Materialize> {
    let registration = m.registration.as_ref().ok_or_else(|| Error::new("registration"))?;
    if epoch <= 0 {
        return Err(Error::new("source_epoch"));
    }
    Ok(Materialize {
        producer: m.producer.clone(),
        context: json!({
            "source_id": m.source_id, "source_generation": base + 1, "source_epoch": epoch,
            "admission_binary": registration.admission_binary, "admission_sha256": registration.admission_sha256,
            "binding": binding, "pin": pin, "target": target,
        }),
        lease: json!({
            "lease_id": task, "lease_generation": generation,
            "fence": {"shard": target["shard"], "owner": target["owner"]},
        }),
    })
}

/// Owner-only permissions on a file the worker wrote.
fn private_mode(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

fn validate_request(
    request: &Value,
    source: &str,
    task: &str,
    generation: i64,
    base: i64,
    database: &str,
) -> Result<()> {
    let ok = request["protocol"] == REQUEST_PROTOCOL
        && request["source_id"] == source
        && request["source_generation"].as_i64() == Some(base + 1)
        && request["lease_id"] == task
        && request["lease_generation"].as_i64() == Some(generation)
        && request.pointer("/target/database").and_then(Value::as_str) == Some(database);
    if ok {
        Ok(())
    } else {
        Err(Error::new("request"))
    }
}

/// Checks the producer arguments and resolves the config and out paths.
fn producer_paths(m: &Manifest, task: &str) -> Result<(PathBuf, PathBuf, Vec<String>)> {
    let args = &m.producer.args;
    if args.len() != 5
        || args[0] != PRODUCER_SUBCOMMAND
        || args[1] != "--config"
        || args[3] != "--out"
    {
        return Err(Error::new("baseline_producer"));
    }
    let config = PathBuf::from(&args[2]);
    if !config.is_absolute() || args[2].contains("..") {
        return Err(Error::new("baseline_config"));
    }
    let template = &args[4];
    if !template.contains("{task}") || template.contains("{generation}") {
        return Err(Error::new("baseline_out"));
    }
    let out = PathBuf::from(template.replace("{task}", task));
    if !out.is_absolute() || out.to_string_lossy().contains("..") {
        return Err(Error::new("baseline_out"));
    }
    let resolved = vec![
        args[0].clone(),
        args[1].clone(),
        args[2].clone(),
        args[3].clone(),
        out.to_string_lossy().into_owned(),
    ];
    Ok((config, out, resolved))
}

async fn record_evidence(
    c: &tokio_postgres::Client,
    task: &str,
    generation: i64,
    producer_sha: &str,
    receipt: &Value,
) -> Result<()> {
    let bytes = receipt.to_string().into_bytes();
    c.execute(
        "INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) \
         VALUES($1::text::uuid,$2,'c1_baseline_stream',$3,0,$4,''::bytea) \
         ON CONFLICT (task_id,generation,stage) DO UPDATE SET stdout=EXCLUDED.stdout, observed_at=clock_timestamp()",
        &[&task, &generation, &producer_sha, &bytes],
    )
    .await
    .map_err(Error::sql)?;
    Ok(())
}

/// Claimed-task entry for `chunked-typed/v2`. Called by `native_schedule::apply` after the
/// claim (`adopt_claim`) and the manifest checks. Returns after the publication tail.
pub(super) async fn apply_v2(
    config: &Config,
    c: &mut tokio_postgres::Client,
    project: &str,
    worker: &str,
    task: &str,
    generation: i64,
    m: &Manifest,
    binding: &str,
    pin: i64,
    database: &str,
    stored: Option<String>,
) -> Result<()> {
    let base = confirmed_generation(config, c, &m.source_id, database, task, &stored).await?;
    if let Some(text) = stored {
        let mut request: Value = serde_json::from_str(&text).map_err(|_| Error::new("request"))?;
        // A re-claim of the same task carries a new lease generation; the stored request
        // was built under an earlier one. Every other binding (task, source, base, target)
        // is still validated below.
        request["lease_generation"] = json!(generation);
        validate_request(&request, &m.source_id, task, generation, base, database)?;
        return apply_cdc_baseline(
            config, c, project, worker, task, generation, binding, pin, m, m, database, base,
            &request,
        )
        .await;
    }
    let override_row = c
        .query_opt(
            "SELECT 1 FROM jw_native.loader_overrides WHERE binding_id=$1::text::uuid",
            &[&binding],
        )
        .await
        .map_err(Error::sql)?;
    if override_row.is_some() {
        return Err(Error::new("v2_loader_override"));
    }
    executable_check(&m.producer)?;
    executable_check(&m.loader)?;
    let (config_path, out, producer_args) = producer_paths(m, task)?;
    let config_bytes = std::fs::read(&config_path).map_err(|_| Error::new("baseline_config"))?;
    if serde_json::from_slice::<Value>(&config_bytes).ok()
        .is_some_and(|config| config.get("c1_initial_resume").is_some()) {
        return Err(Error::new("mysql_initial_requires_v3"));
    }
    let config_sha256 = sha256_hex(&config_bytes);

    let admin = session::admin_db(config, database).await?;
    let owner: String = admin
        .query_opt(
            "SELECT owner_token::text FROM _oo.full_import_run WHERE source_id=$1 ORDER BY target_generation DESC LIMIT 1",
            &[&m.source_id],
        )
        .await
        .map_err(Error::sql)?
        .map(|row| row.get(0))
        .unwrap_or_else(|| binding.to_owned());
    if !is_uuid(&owner) {
        return Err(Error::new("baseline_owner"));
    }
    let previous = if base > 0 {
        let row = admin
            .query_opt(
                "SELECT jsonb_build_object('task_id',s.task_id,'l_end',substring(s.measured_id from 7)||':1-'||_oo.native_pos_text(s.l_end),'measured_id',s.measured_id,'config_digest',s.config_digest,'schema_digest',s.schema_digest,'source_epoch',s.source_epoch,'source_generation',$2::bigint)::text FROM _oo.native_c1_baseline_run s JOIN _oo.native_c1_baseline_typed t USING(source,task_id) JOIN _oo.source_watermark w ON w.source=s.source JOIN jobs.tasks j ON j.id=s.task_id WHERE s.source=$1 AND (t.receipt->>'source_generation')::bigint=$2 AND t.acknowledged_at IS NOT NULL AND j.state='succeeded' AND w.source_generation=$2 AND w.applied_pos=s.l_end AND w.source_epoch=s.source_epoch",
                &[&m.source_id, &base],
            )
            .await
            .map_err(Error::sql)?
            .ok_or_else(|| Error::new("unconfirmed_baseline"))?;
        serde_json::from_str::<Value>(&row.get::<_, String>(0))
            .map_err(|_| Error::new("baseline_previous"))?
    } else {
        Value::Null
    };
    let target = target_owner(config, database).await?;
    let ca_file = config
        .ca_file
        .as_ref()
        .ok_or_else(|| Error::new("baseline_target"))?;
    let stage = Stage {
        source: m.source_id.clone(),
        task: task.to_owned(),
        generation,
        epoch: 0,
        target: json!({
            "host": config.host, "port": config.port, "database": database,
            "runtime_principal": config.runtime_user, "password_file": config.runtime_password_file,
            "ca_file": ca_file, "shard": m.source_id, "owner": owner,
        }),
        shard: m.source_id.clone(),
        owner,
    };
    let plan = Plan {
        source: m.source_id.clone(),
        base,
        baseline: json!({}),
        producer: Executable {
            path: m.producer.path.clone(),
            sha256: m.producer.sha256.clone(),
            args: producer_args,
        },
        out: out.clone(),
        task: task.to_owned(),
        config_sha256,
        previous,
        typing: Typing::FromFrame,
    };
    let target_desc = stage.target.clone();
    let done = {
        let mut port = ProdPort {
            control: c,
            target,
            admin,
            project,
            worker,
            task,
            generation,
            loader: Executable {
                path: m.loader.path.clone(),
                sha256: m.loader.sha256.clone(),
                args: vec![STAGE_SUBCOMMAND.to_owned()],
            },
            refusal: None,
        };
        run(&mut port, &plan, stage).await.map_err(Error::from)?
    };
    match done {
        Done::Quiet(request) => {
            finish_c1_quiet(
                c,
                project,
                worker,
                task,
                generation,
                binding,
                pin,
                &m.source_id,
                database,
                base,
                &request,
            )
            .await
        }
        Done::Ended(summary) => {
            let receipt = json!({
                "protocol": PROTO_EVIDENCE, "frames": summary.frames, "chain_sha256": summary.chain_sha256,
                "attempts": summary.attempts, "chunks": summary.chunks, "batches": summary.batches,
                "source_epoch": summary.source_epoch, "l_end": summary.end.l_end,
                "l_end_pos": summary.end.l_end_pos, "parts_manifest_sha256": summary.end.parts_manifest_sha256,
                "tables": summary.tables,
            });
            record_evidence(c, task, generation, &m.producer.sha256, &receipt).await?;
            let mut post = post_end_for(m, task, generation, binding, pin, &target_desc, summary.source_epoch, base)?;
            finish_post_end(
                config, c, project, worker, task, generation, binding, pin, m, database, base,
                &out, &summary.end, &mut post,
            )
            .await
        }
        Done::Sealed(end) => {
            let admin = session::admin_db(config, database).await?;
            let epoch: i64 = admin
                .query_one(
                    "SELECT source_epoch::bigint FROM _oo.native_c1_baseline_run WHERE source=$1 AND task_id=$2::text::uuid",
                    &[&m.source_id, &task],
                )
                .await
                .map_err(Error::sql)?
                .get(0);
            let mut post = post_end_for(m, task, generation, binding, pin, &target_desc, epoch, base)?;
            finish_post_end(
                config, c, project, worker, task, generation, binding, pin, m, database, base,
                &out, &end, &mut post,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn finish_post_end<P: PostEnd>(
    config: &Config,
    c: &mut tokio_postgres::Client,
    project: &str,
    worker: &str,
    task: &str,
    generation: i64,
    binding: &str,
    pin: i64,
    m: &Manifest,
    database: &str,
    base: i64,
    out: &Path,
    end: &EndFacts,
    post: &mut P,
) -> Result<()> {
    let beat_control = session::runtime(config).await?;
    let beat_target = target_owner(config, database).await?;
    let request = {
        let work = post.request(out, end);
        tokio::pin!(work);
        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + HEARTBEAT, HEARTBEAT);
        loop {
            tokio::select! {
                r = &mut work => break r.map_err(Error::new)?,
                _ = tick.tick() => {
                    let a = lease::heartbeat(&beat_control, project, task, worker, generation).await?;
                    if a.status != LeaseStatus::Extended { return Err(Error::new("lost_lease")); }
                    let b = lease::heartbeat(&beat_target, project, task, worker, generation).await?;
                    if b.status != LeaseStatus::Extended { return Err(Error::new("target_lease")); }
                }
            }
        }
    };
    validate_request(&request, &m.source_id, task, generation, base, database)?;
    let text = request.to_string();
    let n = c
        .execute(
            "UPDATE jw_native.occurrences o SET request=$2::text::jsonb FROM jobs.tasks t WHERE o.task_id=t.id AND t.id=$1::text::uuid AND t.state='running' AND t.lease_owner=$3 AND t.generation=$4 AND t.lease_until>clock_timestamp() AND o.request IS NULL",
            &[&task, &text, &worker, &generation],
        )
        .await
        .map_err(Error::sql)?;
    if n != 1 {
        return Err(Error::new("lost_lease"));
    }
    apply_cdc_baseline(
        config, c, project, worker, task, generation, binding, pin, m, m, database, base, &request,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "mysql_t9";
    const UUID: &str = "11111111-2222-3333-4444-555555555555";
    const TASK: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    const CONFIG_SHA: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const H: &str = "abababababababababababababababababababababababababababababababab";

    fn admission() -> Value {
        json!({"protocol": "oo.source-admission.config/v1", "source_id": SOURCE, "target_id": SOURCE})
    }

    fn preimage() -> Value {
        json!([{"table": "items", "columns": [{"name": "id"}, {"name": "payload"}]}])
    }

    fn config_digest() -> String {
        sha256_hex(&serde_json::to_vec(&admission()).unwrap())
    }

    fn schema_digest() -> String {
        sha256_hex(&serde_json::to_vec(&preimage()).unwrap())
    }

    /// The `open` frame of a fresh run (or a resumed one, which carries `resume: true`).
    fn open_frame(resume: bool, applied: &str) -> Value {
        json!({
            "kind": "open", "protocol": PROTO_CHUNK, "l0": format!("{UUID}:1-100"), "l0_pos": "100",
            "source_uuid": UUID, "config_digest": config_digest(), "connection_config_sha256": CONFIG_SHA,
            "admission_config": admission(), "schema_digest": schema_digest(), "type_digest": H, "input_digest": H, "schema_preimage": preimage(),
            "tables": ["bench.items"],
            "table_schemas": [{
                "schema": "bench", "table": "items", "schema_sha256": H,
                "columns": [
                    {"ordinal": 1, "name": "id", "data_type": "bigint", "primary_key": true},
                    {"ordinal": 2, "name": "payload", "data_type": "text", "primary_key": false}
                ],
                "target_columns": [
                    {"name": "id", "oid": 20, "typmod": -1, "not_null": true},
                    {"name": "payload", "oid": 25, "typmod": -1, "not_null": false}
                ]
            }],
            "resume": resume, "applied_pos": applied,
        })
    }

    fn chunk_frame(
        no: u64,
        lo: Option<&str>,
        hi: Option<&str>,
        rows: u64,
        terminal: bool,
    ) -> Value {
        json!({
            "kind": "chunk", "protocol": PROTO_CHUNK, "schema": "bench", "table": "items", "chunk_no": no,
            "attempt": 1, "terminal": terminal, "key_lo": lo, "key_hi": hi, "rows": rows, "bytes": 512,
            "avg_row_bytes": 100, "part_path": format!("parts/bench.items/fill-{no:08}.arrow"),
            "part_sha256": H, "part_bytes": 900,
            "read": {"source": "primary", "statement_sha256": H, "wait_ms": 0, "rejects": {"count": 0, "by_reason": {}}},
        })
    }

    fn batch_frame(batch_no: u64, first: &str, last: &str, txns: u64, part_no: u64) -> Value {
        let records: Vec<Value> = (0..txns)
            .map(|i| {
                let pos = first.parse::<u64>().unwrap() + i;
                json!({"txn_id": format!("{UUID}:{pos}"), "applied_pos": pos.to_string(), "rows": 1,
                       "payload_sha256": H, "source_commit_timestamp": "2026-10-17T10:00:00Z"})
            })
            .collect();
        json!({
            "kind": "txn_batch", "protocol": PROTO_TXN_BATCH, "batch_no": batch_no,
            "first_pos": first, "last_pos": last, "batch_sha256": H,
            "parts": [{"schema": "bench", "table": "items", "part_no": part_no,
                       "part_path": format!("parts/bench.items/cdc-{part_no:08}.arrow"),
                       "part_sha256": H, "part_bytes": 400, "rows": txns, "key_lo": "7", "key_hi": "8"}],
            "txns": records, "rejects": 0,
        })
    }

    fn end_frame(manifest_sha: &str) -> Value {
        json!({"kind": "end", "protocol": PROTO_CHUNK, "l_end": format!("{UUID}:1-102"), "l_end_pos": "102",
               "applied_pos": "102", "parts_manifest": PARTS_MANIFEST, "parts_manifest_sha256": manifest_sha,
               "fill_rows": 4, "cdc_rows": 2})
    }

    fn open_table() -> OpenFacts {
        validate_open(
            &open_frame(false, "100"),
            SOURCE,
            CONFIG_SHA,
            &Typing::FromFrame,
            false,
        )
        .unwrap()
    }

    fn progress_after_snapshot() -> Progress {
        let mut progress = Progress {
            applied: 100,
            batch_no: 0,
            tables: BTreeMap::new(),
        };
        progress.tables.insert(
            "bench.items".into(),
            TableProgress {
                last_chunk_no: 2,
                last_key_hi: Some(9),
                terminal: false,
                rows: 4,
                cdc_part_no: None,
            },
        );
        progress
    }

    fn expect_token(result: std::result::Result<impl std::fmt::Debug, Token>, token: Token) {
        match result {
            Err(found) => assert_eq!(found, token),
            Ok(value) => panic!("expected {token}, got {value:?}"),
        }
    }

    // ------------------------------------------------------------ open

    #[test]
    fn open_is_bound_to_the_connection_digest_identity_and_schema() {
        let open = open_table();
        assert_eq!(open.spec("bench.items").unwrap().key, "id");
        let mut wrong_connection = open_frame(false, "100");
        wrong_connection["connection_config_sha256"] = json!(H);
        expect_token(
            validate_open(
                &wrong_connection,
                SOURCE,
                CONFIG_SHA,
                &Typing::FromFrame,
                false,
            ),
            "baseline_config",
        );
        expect_token(
            validate_open(
                &open_frame(false, "100"),
                "other",
                CONFIG_SHA,
                &Typing::FromFrame,
                false,
            ),
            "baseline_identity",
        );
        let mut bad_preimage = open_frame(false, "100");
        bad_preimage["schema_preimage"] = json!([]);
        expect_token(
            validate_open(&bad_preimage, SOURCE, CONFIG_SHA, &Typing::FromFrame, false),
            "baseline_catalog",
        );
        let mut bad_l0 = open_frame(false, "100");
        bad_l0["l0"] = json!(format!("{UUID}:1-99"));
        expect_token(
            validate_open(&bad_l0, SOURCE, CONFIG_SHA, &Typing::FromFrame, false),
            "baseline_l0",
        );
    }

    #[test]
    fn open_needs_exactly_one_primary_key_and_matching_typing() {
        let mut no_key = open_frame(false, "100");
        no_key["table_schemas"][0]["columns"][0]["primary_key"] = json!(false);
        expect_token(
            validate_open(&no_key, SOURCE, CONFIG_SHA, &Typing::FromFrame, false),
            "baseline_catalog",
        );
        let mut renamed = open_frame(false, "100");
        renamed["table_schemas"][0]["target_columns"][1]["name"] = json!("other");
        expect_token(
            validate_open(&renamed, SOURCE, CONFIG_SHA, &Typing::FromFrame, false),
            "baseline_catalog",
        );
    }

    #[test]
    fn open_refuses_a_missing_target_typing_so_the_loader_never_guesses_types() {
        let mut frame = open_frame(false, "100");
        frame["table_schemas"][0]
            .as_object_mut()
            .unwrap()
            .remove("target_columns");
        expect_token(
            validate_open(&frame, SOURCE, CONFIG_SHA, &Typing::FromFrame, false),
            "baseline_catalog",
        );
        let mut fixed = BTreeMap::new();
        fixed.insert(
            "bench.items".to_owned(),
            vec![
                Typed {
                    name: "id".into(),
                    oid: 20,
                    typmod: -1,
                    not_null: true,
                },
                Typed {
                    name: "payload".into(),
                    oid: 25,
                    typmod: -1,
                    not_null: false,
                },
            ],
        );
        assert!(validate_open(&frame, SOURCE, CONFIG_SHA, &Typing::Fixed(fixed), false).is_ok());
    }

    #[test]
    fn open_resume_flag_must_match_the_run_kind() {
        expect_token(
            validate_open(
                &open_frame(true, "100"),
                SOURCE,
                CONFIG_SHA,
                &Typing::FromFrame,
                false,
            ),
            "baseline_resume",
        );
        expect_token(
            validate_open(
                &open_frame(false, "100"),
                SOURCE,
                CONFIG_SHA,
                &Typing::FromFrame,
                true,
            ),
            "baseline_resume",
        );
    }

    // ------------------------------------------------------------ chunk

    #[test]
    fn chunk_numbers_are_contiguous_and_nothing_follows_the_terminal_chunk() {
        let open = open_table();
        let mut progress = Progress {
            applied: 100,
            ..Progress::default()
        };
        expect_token(
            validate_chunk(
                &chunk_frame(2, Some("1"), Some("5"), 2, false),
                &open,
                &progress,
            ),
            "chunk_sequence",
        );
        let first = validate_chunk(
            &chunk_frame(1, Some("1"), Some("5"), 2, false),
            &open,
            &progress,
        )
        .unwrap();
        assert_eq!((first.chunk_no, first.rows, first.key_hi), (1, 2, Some(5)));
        progress.tables.insert(
            "bench.items".into(),
            TableProgress {
                last_chunk_no: 1,
                last_key_hi: Some(5),
                ..Default::default()
            },
        );
        expect_token(
            validate_chunk(
                &chunk_frame(2, Some("6"), Some("9"), 0, true),
                &open,
                &progress,
            ),
            "chunk_terminal",
        );
        assert!(validate_chunk(&chunk_frame(2, None, None, 0, true), &open, &progress).is_ok());
        progress.tables.get_mut("bench.items").unwrap().terminal = true;
        expect_token(
            validate_chunk(&chunk_frame(3, None, None, 0, true), &open, &progress),
            "chunk_after_terminal",
        );
    }

    #[test]
    fn chunk_keys_follow_the_previous_chunk_and_stay_ordered() {
        let open = open_table();
        let progress = progress_after_snapshot();
        expect_token(
            validate_chunk(
                &chunk_frame(3, Some("9"), Some("12"), 2, false),
                &open,
                &progress,
            ),
            "chunk_keys",
        );
        expect_token(
            validate_chunk(
                &chunk_frame(3, Some("12"), Some("10"), 2, false),
                &open,
                &progress,
            ),
            "chunk_keys",
        );
        assert!(
            validate_chunk(
                &chunk_frame(3, Some("10"), Some("12"), 2, false),
                &open,
                &progress
            )
            .is_ok()
        );
        expect_token(
            validate_chunk(
                &chunk_frame(3, None, Some("12"), 2, false),
                &open,
                &progress,
            ),
            "chunk_keys",
        );
    }

    #[test]
    fn chunk_tables_and_part_paths_are_confined() {
        let open = open_table();
        let progress = Progress {
            applied: 100,
            ..Progress::default()
        };
        let mut other = chunk_frame(1, Some("1"), Some("5"), 2, false);
        other["table"] = json!("missing");
        expect_token(validate_chunk(&other, &open, &progress), "chunk_table");
        for bad in [
            "/etc/fill-00000001.arrow",
            "parts/../fill-00000001.arrow",
            "other/fill-00000001.arrow",
            "parts/bench.items/cdc-00000001.arrow",
            "parts/bench.items/fill-1.arrow",
        ] {
            let mut frame = chunk_frame(1, Some("1"), Some("5"), 2, false);
            frame["part_path"] = json!(bad);
            expect_token(validate_chunk(&frame, &open, &progress), "part_path");
        }
    }

    // ------------------------------------------------------------ batch

    #[test]
    fn batch_positions_chain_from_applied_plus_one() {
        let open = open_table();
        let progress = progress_after_snapshot();
        expect_token(
            validate_batch(
                &batch_frame(1, "101", "102", 2, 1),
                &open,
                &Progress {
                    applied: 99,
                    ..progress.clone()
                },
            ),
            "batch_chain",
        );
        let facts = validate_batch(&batch_frame(1, "101", "102", 2, 1), &open, &progress).unwrap();
        assert_eq!((facts.first, facts.last, facts.rows), (101, 102, 2));
        let mut gap = batch_frame(1, "101", "102", 2, 1);
        gap["txns"][1]["applied_pos"] = json!("103");
        expect_token(validate_batch(&gap, &open, &progress), "batch_txns");
    }

    #[test]
    fn batch_numbers_and_row_sums_must_agree() {
        let open = open_table();
        let progress = progress_after_snapshot();
        expect_token(
            validate_batch(&batch_frame(2, "101", "102", 2, 1), &open, &progress),
            "batch_sequence",
        );
        let mut rows = batch_frame(1, "101", "102", 2, 1);
        rows["txns"][0]["rows"] = json!(3);
        expect_token(validate_batch(&rows, &open, &progress), "batch_rows");
        let mut empty = batch_frame(1, "101", "102", 2, 1);
        empty["parts"][0]["rows"] = json!(0);
        expect_token(validate_batch(&empty, &open, &progress), "batch_part_rows");
    }

    #[test]
    fn batch_part_numbers_are_contiguous_per_table_within_a_run() {
        let open = open_table();
        let mut progress = progress_after_snapshot();
        progress.tables.get_mut("bench.items").unwrap().cdc_part_no = Some(4);
        expect_token(
            validate_batch(&batch_frame(1, "101", "102", 2, 4), &open, &progress),
            "batch_part_no",
        );
        assert!(validate_batch(&batch_frame(1, "101", "102", 2, 5), &open, &progress).is_ok());
        // After a resume the loader does not report the last part number, so the first one is not checked.
        assert!(
            validate_batch(
                &batch_frame(1, "101", "102", 2, 9),
                &open,
                &progress_after_snapshot()
            )
            .is_ok()
        );
    }

    #[test]
    fn end_requires_terminal_tables_the_applied_position_and_the_same_lineage() {
        let open = open_table();
        let mut progress = progress_after_snapshot();
        progress.applied = 102;
        progress.tables.get_mut("bench.items").unwrap().terminal = true;
        let end = validate_end(&end_frame(H), &open, &progress).unwrap();
        assert_eq!(
            (end.l_end_pos, end.parts_manifest_sha256.as_str()),
            (102, H)
        );
        let mut not_terminal = progress.clone();
        not_terminal.tables.get_mut("bench.items").unwrap().terminal = false;
        expect_token(
            validate_end(&end_frame(H), &open, &not_terminal),
            "end_tables",
        );
        let mut behind = progress.clone();
        behind.applied = 101;
        expect_token(validate_end(&end_frame(H), &open, &behind), "end_position");
        let mut lineage = end_frame(H);
        lineage["l_end"] = json!("22222222-2222-3333-4444-555555555555:1-102");
        expect_token(validate_end(&lineage, &open, &progress), "end_lineage");
    }

    #[test]
    fn quiet_must_name_the_previous_run_exactly() {
        let previous =
            json!({"task_id": TASK, "l_end": format!("{UUID}:1-100"), "config_digest": H});
        let request = json!({"protocol": PROTO_NOCHANGE, "source_id": SOURCE, "source_generation": 4,
            "previous": previous, "observed_gtid": format!("{UUID}:1-100"), "connection_config_sha256": CONFIG_SHA});
        let frame = json!({"kind": "quiet", "protocol": PROTO_CHUNK, "request": request});
        assert!(validate_quiet(&frame, SOURCE, 4, CONFIG_SHA, &previous).is_ok());
        expect_token(
            validate_quiet(&frame, SOURCE, 5, CONFIG_SHA, &previous),
            "baseline_quiet",
        );
        expect_token(
            validate_quiet(&frame, SOURCE, 4, H, &previous),
            "baseline_quiet",
        );
        expect_token(
            validate_quiet(&frame, SOURCE, 4, CONFIG_SHA, &Value::Null),
            "baseline_quiet",
        );
    }

    // ------------------------------------------------------------ mapping

    fn stage() -> Stage {
        Stage {
            source: SOURCE.into(),
            task: TASK.into(),
            generation: 3,
            epoch: 7,
            target: json!({"host": "db", "database": "oo"}),
            shard: SOURCE.into(),
            owner: "ffffffff-ffff-ffff-ffff-ffffffffffff".into(),
        }
    }

    #[test]
    fn chunk_maps_the_previous_key_as_the_exclusive_lower_bound() {
        let open = open_table();
        let spec = open.spec("bench.items").unwrap().clone();
        let facts = validate_chunk(
            &chunk_frame(3, Some("10"), Some("12"), 2, false),
            &open,
            &progress_after_snapshot(),
        )
        .unwrap();
        let op = map_chunk(
            &stage(),
            &facts,
            &spec,
            Some(9),
            Path::new("/out/parts/bench.items/fill-00000003.arrow"),
        );
        assert_eq!(op["operation"], "chunk");
        assert_eq!(
            op["key_lo"], "9",
            "the loader requires the previous key_hi, not the frame's smallest key"
        );
        assert_eq!(op["key_hi"], "12");
        assert_eq!(op["part"]["key_lo"], "10");
        assert_eq!(
            op["part"]["path"],
            "/out/parts/bench.items/fill-00000003.arrow"
        );
        assert_eq!(op["columns"], json!(["id", "payload"]));
        assert_eq!(op["key"], "id");
        assert!(op.get("fence").is_none(), "chunk carries no fence");
    }

    #[test]
    fn first_chunk_and_terminal_chunk_map_keys_as_the_loader_expects() {
        let open = open_table();
        let spec = open.spec("bench.items").unwrap().clone();
        let first = validate_chunk(
            &chunk_frame(1, Some("1"), Some("5"), 2, false),
            &open,
            &Progress {
                applied: 100,
                ..Progress::default()
            },
        )
        .unwrap();
        let op = map_chunk(&stage(), &first, &spec, None, Path::new("/out/x.arrow"));
        assert!(op["key_lo"].is_null(), "the first chunk has no lower bound");
        let mut progress = progress_after_snapshot();
        progress
            .tables
            .get_mut("bench.items")
            .unwrap()
            .last_chunk_no = 2;
        let terminal =
            validate_chunk(&chunk_frame(3, None, None, 0, true), &open, &progress).unwrap();
        let op = map_chunk(
            &stage(),
            &terminal,
            &spec,
            Some(9),
            Path::new("/out/y.arrow"),
        );
        assert_eq!(op["key_lo"], "9");
        assert!(op["key_hi"].is_null());
        assert_eq!(op["row_count"], 0);
        assert_eq!(op["terminal"], true);
    }

    #[test]
    fn batch_maps_transaction_records_and_absolute_part_paths() {
        let open = open_table();
        let facts = validate_batch(
            &batch_frame(1, "101", "102", 2, 1),
            &open,
            &progress_after_snapshot(),
        )
        .unwrap();
        let op = map_batch(&stage(), &facts, &open, Path::new("/out")).unwrap();
        assert_eq!(
            (
                op["first_pos"].as_str(),
                op["last_pos"].as_str(),
                op["txns"].as_u64()
            ),
            (Some("101"), Some("102"), Some(2))
        );
        assert_eq!(op["row_count"], 2);
        assert_eq!(op["txn_records"][1]["applied_pos"], "102");
        assert_eq!(op["txn_records"][1]["row_count"], 1);
        assert_eq!(
            op["parts"][0]["part"]["path"],
            "/out/parts/bench.items/cdc-00000001.arrow"
        );
        assert_eq!(op["parts"][0]["part"]["key_hi"], "8");
    }

    #[test]
    #[test]
    fn open_binds_the_producer_digests_without_recomputing_them() {
        // FRAMES-V2 §3.1: type and input digests are the producer's values from the open frame.
        let open = open_table();
        let first = map_open(&stage(), &open, 5);
        assert_eq!(first["type_digest"], json!(open.type_digest));
        assert_eq!(first["input_digest"], json!(open.input_digest));
        assert_eq!(first["schema_digest"], json!(open.schema_digest));
        assert_eq!(first["tables"][0]["oids"], json!([20, 25]));
        assert_eq!(first["tables"][0]["not_null"], json!([true, false]));
        assert_eq!(first["source_generation"], 5);
        assert_eq!(first["fence"]["shard"], SOURCE);
        // Changing the typing does not change the bound digest: the worker never derives it.
        let mut changed = open.clone();
        changed.tables[0].typed[1].oid = 1043;
        assert_eq!(map_open(&stage(), &changed, 5)["type_digest"], first["type_digest"]);
    }

    // ------------------------------------------------------------ resume

    fn rebind_open() -> Value {
        json!({
            "format": FORMAT, "state": "open", "l0": format!("{UUID}:1-100"), "l0_pos": "100", "applied_pos": "100",
            "config_digest": config_digest(), "schema_digest": schema_digest(), "source_epoch": 7,
            "tables": [{"schema": "bench", "table": "items", "physical": "pt_x", "run_id": TASK,
                        "last_chunk_no": 2, "last_key_hi": "9", "terminal": false, "rows": 4}],
            "batches": {"last_batch_no": 0, "last_pos": null},
        })
    }

    #[test]
    fn resume_builds_the_producer_resume_and_the_progress_from_rebind() {
        let Resume::Open {
            bootstrap,
            progress,
            l0_pos,
            ..
        } = resume_from_rebind(&rebind_open()).unwrap()
        else {
            panic!("open run");
        };
        assert_eq!(bootstrap["state"], "open");
        assert_eq!(bootstrap["applied_pos"], "100");
        assert_eq!(bootstrap["tables"][0]["last_chunk_no"], 2);
        assert_eq!(bootstrap["tables"][0]["last_key_hi"], "9");
        assert_eq!(bootstrap["tables"][0]["terminal"], false);
        assert_eq!(l0_pos, 100);
        assert_eq!(progress, progress_after_snapshot());
    }

    #[test]
    fn resume_refuses_a_foreign_format_and_an_unknown_state() {
        let mut foreign = rebind_open();
        foreign["format"] = json!("chunked-typed/v1");
        expect_token(resume_from_rebind(&foreign), "format_mismatch");
        let mut unknown = rebind_open();
        unknown["state"] = json!("retired");
        expect_token(resume_from_rebind(&unknown), "resume_state");
        let mut sealed = rebind_open();
        sealed["state"] = json!("awaiting_publish");
        sealed["l_end"] = json!(format!("{UUID}:1-102"));
        sealed["l_end_pos"] = json!("102");
        assert_eq!(
            resume_from_rebind(&sealed).unwrap(),
            Resume::Sealed {
                l_end: format!("{UUID}:1-102"),
                l_end_pos: 102
            }
        );
    }

    // ------------------------------------------------------------ retry policy

    #[test]
    fn producer_retries_only_transient_exits_and_is_bounded() {
        for code in [None, Some(4), Some(8)] {
            assert!(
                matches!(producer_retry(code, 1), Next::Retry(_)),
                "{code:?} is transient"
            );
        }
        for code in [Some(1), Some(3), Some(5), Some(6), Some(7), Some(9)] {
            assert_eq!(
                producer_retry(code, 1),
                Next::Stop("producer_failed"),
                "{code:?} is deterministic"
            );
        }
        assert_eq!(
            producer_retry(Some(4), MAX_PRODUCER_ATTEMPTS),
            Next::Stop("producer_retries")
        );
    }

    #[test]
    fn producer_retry_budget_covers_five_minutes() {
        let total: u64 = (1..MAX_PRODUCER_ATTEMPTS).map(|n| backoff(n).as_secs()).sum();
        assert!(total >= 300, "the producer retry budget is {total} s");
    }

    #[test]
    fn producer_exhaustion_keeps_the_loader_run_open() {
        assert!(keeps_loader_state("producer_retries"));
        for token in ["producer_failed", "chunk_conflict", "lease_or_barrier", "end_manifest_sha"] {
            assert!(!keeps_loader_state(token), "{token} retires the run");
        }
    }

    #[test]
    fn exhausted_producer_starts_a_new_round_until_the_rounds_are_spent() {
        assert_eq!(producer_step(Some(4), 1, 0), Step::Retry(Duration::from_secs(1)));
        assert_eq!(
            producer_step(Some(4), MAX_PRODUCER_ATTEMPTS, 0),
            Step::NewRound(Duration::from_secs(MAX_BACKOFF_SECS)),
            "a spent round restarts the producer from a fresh rebind"
        );
        assert_eq!(
            producer_step(Some(4), MAX_PRODUCER_ATTEMPTS, MAX_PRODUCER_ROUNDS - 2),
            Step::NewRound(Duration::from_secs(MAX_BACKOFF_SECS)),
        );
        assert_eq!(
            producer_step(Some(4), MAX_PRODUCER_ATTEMPTS, MAX_PRODUCER_ROUNDS - 1),
            Step::Stop("producer_retries"),
            "the last round ends the claim, and the loader run stays open"
        );
        assert_eq!(producer_step(Some(1), 1, 0), Step::Stop("producer_failed"));
        assert_eq!(producer_step(Some(1), 1, 3), Step::Stop("producer_failed"));
    }

    #[test]
    fn loader_refusals_keep_their_real_reason_in_the_token_list() {
        for token in ["run_not_open", "physical_mapping_absent", "seal_conflict", "table_unregistered"] {
            assert_eq!(loader_token(token), token, "{token} is named, not loader_refused");
        }
    }

    #[test]
    fn refusal_evidence_is_bounded_and_plain() {
        assert_eq!(refusal_evidence("closed"), "closed");
        assert_eq!(refusal_evidence("a b\n<x>"), "abx");
        assert_eq!(refusal_evidence(&"z".repeat(100)).len(), 64);
        assert_eq!(refusal_evidence(" "), "empty");
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let seconds: Vec<u64> = (1..=7).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(seconds, vec![1, 2, 4, 8, 16, 30, 30]);
    }

    #[test]
    fn loader_replays_transient_refusals_and_stops_on_the_rest() {
        assert!(matches!(loader_retry("target_apply", 1), Next::Retry(_)));
        assert!(matches!(loader_retry("transport", 2), Next::Retry(_)));
        assert_eq!(
            loader_retry("target_apply", MAX_LOADER_ATTEMPTS),
            Next::Stop("target_apply")
        );
        assert_eq!(
            loader_retry("chunk_conflict", 1),
            Next::Stop("chunk_conflict")
        );
        assert_eq!(loader_retry("unlisted", 1), Next::Stop("loader_refused"));
        assert_eq!(loader_retry("closed", 1), Next::Stop("closed"));
        assert_eq!(loader_retry("lease_or_barrier", 1), Next::Lost);
        assert_eq!(loader_token("chunk_conflict"), "chunk_conflict");
        assert_eq!(loader_token("something new"), "loader_refused");
    }

    // ------------------------------------------------------------ ledger

    #[test]
    fn ledger_chains_frame_digests_and_keeps_each_line_small() {
        let dir = std::env::temp_dir().join(format!("oo-cb-t9-ledger-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut ledger = Ledger::open(&dir).unwrap();
        let start = ledger.chain().to_owned();
        ledger
            .record(
                b"{\"kind\":\"chunk\"}",
                json!({"kind": "chunk", "chunk_no": 1}),
            )
            .unwrap();
        let after_one = ledger.chain().to_owned();
        ledger
            .record(
                b"{\"kind\":\"chunk\"}",
                json!({"kind": "chunk", "chunk_no": 1}),
            )
            .unwrap();
        assert_ne!(start, after_one);
        assert_ne!(after_one, ledger.chain());
        assert_eq!(ledger.frames(), 2);
        let text = std::fs::read_to_string(dir.join(LEDGER_FILE)).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(
            text.lines()
                .all(|line| line.len() < 256 && !line.contains("\"kind\":\"chunk\"}")),
            "no raw frame is kept"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ------------------------------------------------------------ manifest and paths

    fn manifest(loader_args: &[&str], format: Option<&str>, producer_subcommand: &str) -> Manifest {
        let mut value = json!({
            "protocol": "oo.jobs.native-schedule/v1",
            "binding": "11111111-2222-3333-4444-555555555555",
            "project_id": "66666666-7777-8888-9999-aaaaaaaaaaaa",
            "source_id": SOURCE, "method": "C1", "cycle": "pull", "period_ms": 60000,
            "producer": {"path": "/bin/sh", "sha256": H, "args": [producer_subcommand, "--config", "/etc/p.json", "--out", "/var/oo/{task}/capture"]},
            "loader": {"path": "/bin/sh", "sha256": H, "args": loader_args},
            "registration": {"admission_binary": "/bin/sh", "admission_sha256": H},
        });
        if let Some(format) = format {
            value["c1_baseline_format"] = json!(format);
        }
        serde_json::from_value(value).expect("manifest shape")
    }

    #[test]
    fn manifest_accepts_v2_only_with_its_format_and_subcommand() {
        let v2 = manifest(
            &["import-native-c1-baseline-v2"],
            Some(FORMAT),
            PRODUCER_SUBCOMMAND,
        );
        assert!(v2.shape().is_ok(), "the v2 manifest is accepted");
        expect_manifest_refused(manifest(
            &["import-native-c1-baseline-v2"],
            None,
            PRODUCER_SUBCOMMAND,
        ));
        expect_manifest_refused(manifest(
            &["import-native-c1-baseline-v2"],
            Some("v1"),
            PRODUCER_SUBCOMMAND,
        ));
        expect_manifest_refused(manifest(
            &["import-native-c1-baseline-v2"],
            Some(FORMAT),
            "c1-baseline-stream",
        ));
        expect_manifest_refused(manifest(
            &["import-native-c1-baseline"],
            Some(FORMAT),
            "c1-baseline-stream",
        ));
    }

    #[test]
    fn v1_manifests_are_unchanged_and_never_carry_the_format() {
        let v1 = manifest(&["import-native-c1-baseline"], None, "c1-baseline-stream");
        assert!(v1.shape().is_ok());
        expect_manifest_refused(manifest(
            &["import-native-c1-baseline"],
            Some(FORMAT),
            "c1-baseline-stream",
        ));
        let text = serde_json::to_string(&v1).expect("json");
        assert!(
            !text.contains("c1_baseline_format"),
            "an absent format is not written, so the v1 canonical text is unchanged"
        );
    }

    fn expect_manifest_refused(manifest: Manifest) {
        assert!(manifest.shape().is_err(), "{:?}", manifest.loader.args);
    }

    #[test]
    fn the_out_path_is_per_task_so_a_resume_finds_its_journal() {
        let v2 = manifest(
            &["import-native-c1-baseline-v2"],
            Some(FORMAT),
            PRODUCER_SUBCOMMAND,
        );
        let (config, out, args) = producer_paths(&v2, TASK).expect("paths");
        assert_eq!(config, PathBuf::from("/etc/p.json"));
        assert_eq!(out, PathBuf::from(format!("/var/oo/{TASK}/capture")));
        assert_eq!(args[4], out.to_string_lossy());
        let mut generation_dir = v2.clone();
        generation_dir.producer.args[4] = "/var/oo/{task}/{generation}/capture".into();
        assert!(
            producer_paths(&generation_dir, TASK).is_err(),
            "a generation in the path would hide the journal on resume"
        );
        let mut unscoped = v2.clone();
        unscoped.producer.args[4] = "/var/oo/capture".into();
        assert!(
            producer_paths(&unscoped, TASK).is_err(),
            "the out path must name the task"
        );
        let mut relative = v2.clone();
        relative.producer.args[4] = "relative/{task}".into();
        assert!(producer_paths(&relative, TASK).is_err());
    }

    #[test]
    fn the_final_request_must_name_this_run() {
        let request = json!({"protocol": REQUEST_PROTOCOL, "source_id": SOURCE, "source_generation": 5,
            "lease_id": TASK, "lease_generation": 3, "target": {"database": "oo"}});
        assert!(validate_request(&request, SOURCE, TASK, 3, 4, "oo").is_ok());
        assert!(
            validate_request(&request, SOURCE, TASK, 4, 4, "oo").is_err(),
            "another generation"
        );
        assert!(
            validate_request(&request, SOURCE, TASK, 3, 4, "other").is_err(),
            "another database"
        );
        assert!(
            validate_request(&request, SOURCE, TASK, 3, 3, "oo").is_err(),
            "another source generation"
        );
    }

    // ------------------------------------------------------------ driver (fake port, fake producer)

    /// A producer stand-in. It prints frames from `$2` after a crash, `$1` before. It
    /// reads one ack after each frame. A crash is `exit 4` before frame `$8`.
    const PRODUCER_SH: &str = r#"#!/bin/sh
frames_first="$1"; frames_resume="$2"; out="$3"; manifest="$4"; acks="$5"; boots="$6"; state="$7"; fail_after="$8"
mkdir -p "$out/parts" || exit 3
cp "$manifest" "$out/parts-manifest.json" || exit 3
read -r boot || exit 2
printf '%s\n' "$boot" >> "$boots"
if [ -e "$state" ]; then src="$frames_resume"; else src="$frames_first"; fi
n=0
while IFS= read -r frame <&3; do
  if [ "$fail_after" != "-" ] && [ ! -e "$state" ] && [ "$n" -ge "$fail_after" ]; then : > "$state"; exit 4; fi
  printf '%s\n' "$frame"
  case "$frame" in *'"kind":"quiet"'*) exit 0;; esac
  read -r ack || exit 1
  printf '%s\n' "$ack" >> "$acks"
  n=$((n+1))
done 3< "$src"
exit 0
"#;

    #[derive(Default)]
    struct FakePort {
        rebinds: Vec<Option<Value>>,
        rebind_calls: usize,
        ops: Vec<Value>,
        fail_once: Option<(&'static str, &'static str)>,
        pids: Vec<u32>,
        refused: Option<String>,
    }

    impl Port for FakePort {
        async fn rebind(&mut self, _stage: &Stage) -> std::result::Result<Option<Value>, Fault> {
            let index = self.rebind_calls.min(self.rebinds.len() - 1);
            self.rebind_calls += 1;
            Ok(self.rebinds[index].clone())
        }

        async fn register(
            &mut self,
            _stage: &Stage,
            _open: &OpenFacts,
        ) -> std::result::Result<i64, Fault> {
            Ok(7)
        }

        async fn call(&mut self, operation: Value) -> std::result::Result<Value, Fault> {
            self.ops.push(operation.clone());
            let name = operation["operation"].as_str().unwrap_or("").to_owned();
            if let Some((op, token)) = self.fail_once {
                if op == name {
                    self.fail_once = None;
                    self.refused = Some(token.to_owned());
                    return Err(Fault::Token(token));
                }
            }
            Ok(match name.as_str() {
                "open" => json!({"outcome": "opened"}),
                "chunk" => json!({"outcome": "loaded", "chunk_no": operation["chunk_no"]}),
                "txn_batch" => json!({"outcome": "applied", "applied_pos": operation["last_pos"]}),
                "retire" => json!({"outcome": "retired"}),
                _ => json!({"outcome": "refused", "reason": "unexpected"}),
            })
        }

        async fn beat(&mut self) -> std::result::Result<(), Fault> {
            Ok(())
        }

        fn producer_started(&mut self, pid: u32) {
            self.pids.push(pid);
        }

        fn take_refusal(&mut self) -> Option<String> {
            self.refused.take()
        }
    }

    fn fresh_frames() -> Vec<Value> {
        vec![
            open_frame(false, "100"),
            json!({"kind": "stream_started", "protocol": PROTO_CHUNK, "l0": format!("{UUID}:1-100"), "applied_pos": "100", "position_lineage": format!("mysql.{UUID}")}),
            chunk_frame(1, Some("1"), Some("5"), 2, false),
            chunk_frame(2, Some("6"), Some("9"), 2, false),
            chunk_frame(3, None, None, 0, true),
            batch_frame(1, "101", "102", 2, 1),
            end_frame(&manifest_sha()),
        ]
    }

    fn resumed_frames() -> Vec<Value> {
        vec![
            open_frame(true, "100"),
            json!({"kind": "stream_started", "protocol": PROTO_CHUNK, "l0": format!("{UUID}:1-100"), "applied_pos": "100", "position_lineage": format!("mysql.{UUID}")}),
            chunk_frame(3, None, None, 0, true),
            batch_frame(1, "101", "102", 2, 1),
            end_frame(&manifest_sha()),
        ]
    }

    fn manifest_bytes() -> Vec<u8> {
        br#"{"protocol":"oo.mysql.c1-parts-manifest/v2","tables":[]}"#.to_vec()
    }

    fn manifest_sha() -> String {
        sha256_hex(&manifest_bytes())
    }

    /// A materializer stand-in. It checks its arguments, copies the context file it is
    /// given into the out directory, and prints `request` as its one output line.
    fn fake_materializer(scratch: &Scratch, request: &Value) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = scratch.path("materialize.sh");
        let body = format!(
            "#!/bin/sh\n[ \"$1\" = c1-materialize-v2 ] || exit 2\n[ \"$2\" = --out ] || exit 2\n\
             [ \"$4\" = --context ] || exit 2\ncp \"$5\" \"$3/seen-context.json\" || exit 3\n\
             printf '%s\\n' '{request}'\n"
        );
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn end_facts() -> EndFacts {
        EndFacts {
            l_end: format!("{UUID}:1-120"),
            l_end_pos: 120,
            parts_manifest_sha256: H.into(),
        }
    }

    fn post_end(path: PathBuf) -> Materialize {
        Materialize {
            producer: Executable {
                path: path.to_string_lossy().into_owned(),
                sha256: H.into(),
                args: Vec::new(),
            },
            context: json!({"source_id": SOURCE, "source_generation": 6, "source_epoch": 2}),
            lease: json!({"lease_id": TASK, "lease_generation": 7,
                          "fence": {"shard": SOURCE, "owner": "11111111-1111-1111-1111-111111111111"}}),
        }
    }

    /// The materializer's request is the T8 document. The worker adds the lease and fence,
    /// and the context it passes is written with mode 0600.
    #[tokio::test]
    async fn post_end_runs_the_materializer_and_adds_the_lease_and_fence() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("postend-ok");
        let out = scratch.path("out");
        std::fs::create_dir_all(&out).unwrap();
        let end = end_facts();
        let request = json!({"protocol": REQUEST_PROTOCOL, "source_id": SOURCE, "source_generation": 6,
            "l_end": end.l_end, "l_end_pos": "120", "target": {"database": "oo_t"}, "entries": []});
        let fake = fake_materializer(&scratch, &request);
        let got = post_end(fake).request(&out, &end).await.expect("request");
        assert_eq!(got["lease_id"], json!(TASK));
        assert_eq!(got["lease_generation"], json!(7));
        assert_eq!(got["fence"]["shard"], json!(SOURCE));
        assert_eq!(got["source_generation"], json!(6));
        let seen: Value = serde_json::from_slice(&std::fs::read(out.join("seen-context.json")).unwrap()).unwrap();
        assert_eq!(seen["source_epoch"], json!(2));
        let mode = std::fs::metadata(out.join(MATERIALIZE_CONTEXT)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    /// A request for another L_end is refused. The loader would seal a different position.
    #[tokio::test]
    async fn post_end_refuses_a_request_for_another_l_end() {
        let scratch = Scratch::new("postend-end");
        let out = scratch.path("out");
        std::fs::create_dir_all(&out).unwrap();
        let end = end_facts();
        let request = json!({"protocol": REQUEST_PROTOCOL, "source_id": SOURCE, "source_generation": 6,
            "l_end": format!("{UUID}:1-119"), "l_end_pos": "119"});
        let fake = fake_materializer(&scratch, &request);
        assert_eq!(post_end(fake).request(&out, &end).await.err(), Some("materialize_end"));
    }

    /// A materializer that fails, or prints something that is not the request, stops the run.
    #[tokio::test]
    async fn post_end_refuses_a_failing_or_malformed_materializer() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("postend-bad");
        let out = scratch.path("out");
        std::fs::create_dir_all(&out).unwrap();
        let end = end_facts();
        let failing = scratch.path("failing.sh");
        std::fs::write(&failing, "#!/bin/sh\nexit 3\n").unwrap();
        std::fs::set_permissions(&failing, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(post_end(failing).request(&out, &end).await.err(), Some("materialize_failed"));
        let malformed = scratch.path("malformed.sh");
        std::fs::write(&malformed, "#!/bin/sh\necho not-json\n").unwrap();
        std::fs::set_permissions(&malformed, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(post_end(malformed).request(&out, &end).await.err(), Some("materialize_output"));
    }

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("oo-cb-t9-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
        fn write_frames(&self, name: &str, frames: &[Value]) -> PathBuf {
            let mut text = String::new();
            for frame in frames {
                text.push_str(&frame.to_string());
                text.push('\n');
            }
            let path = self.path(name);
            std::fs::write(&path, text).unwrap();
            path
        }
        fn script(&self) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let path = self.path("producer.sh");
            std::fs::write(&path, PRODUCER_SH).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Harness {
        plan: Plan,
        acks: PathBuf,
        boots: PathBuf,
        out: PathBuf,
    }

    fn harness(
        scratch: &Scratch,
        first: &[Value],
        resume: &[Value],
        fail_after: Option<usize>,
        script: Option<PathBuf>,
    ) -> Harness {
        let out = scratch.path(TASK).join("capture");
        let manifest = scratch.path("manifest.json");
        std::fs::write(&manifest, manifest_bytes()).unwrap();
        let acks = scratch.path("acks.ndjson");
        let boots = scratch.path("boots.ndjson");
        let state = scratch.path("crashed");
        let plan = Plan {
            source: SOURCE.into(),
            base: 4,
            baseline: json!({}),
            producer: Executable {
                path: script
                    .unwrap_or_else(|| scratch.script())
                    .to_string_lossy()
                    .into_owned(),
                sha256: H.into(),
                args: vec![
                    scratch
                        .write_frames("first.ndjson", first)
                        .to_string_lossy()
                        .into_owned(),
                    scratch
                        .write_frames("resume.ndjson", resume)
                        .to_string_lossy()
                        .into_owned(),
                    out.to_string_lossy().into_owned(),
                    manifest.to_string_lossy().into_owned(),
                    acks.to_string_lossy().into_owned(),
                    boots.to_string_lossy().into_owned(),
                    state.to_string_lossy().into_owned(),
                    fail_after.map_or("-".to_owned(), |n| n.to_string()),
                ],
            },
            out: out.clone(),
            task: TASK.into(),
            config_sha256: CONFIG_SHA.into(),
            previous: Value::Null,
            typing: Typing::FromFrame,
        };
        Harness {
            plan,
            acks,
            boots,
            out,
        }
    }

    fn lines(path: &Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn ops_named(port: &FakePort, name: &str) -> Vec<Value> {
        port.ops
            .iter()
            .filter(|op| op["operation"] == name)
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn a_run_acks_each_frame_only_after_the_loader_commits_it() {
        let scratch = Scratch::new("ordered");
        let h = harness(&scratch, &fresh_frames(), &resumed_frames(), None, None);
        let mut port = FakePort {
            rebinds: vec![None],
            ..Default::default()
        };
        let done = run(&mut port, &h.plan, stage()).await.expect("run");
        let Done::Ended(summary) = done else {
            panic!("expected end")
        };
        assert_eq!(
            (summary.chunks, summary.batches, summary.end.l_end_pos),
            (3, 1, 102)
        );
        assert_eq!(summary.source_epoch, 7);
        let names: Vec<&str> = port
            .ops
            .iter()
            .map(|op| op["operation"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["open", "chunk", "chunk", "chunk", "txn_batch"]);
        let acks = lines(&h.acks);
        let stages: Vec<&str> = acks.iter().map(|a| a["stage"].as_str().unwrap()).collect();
        assert_eq!(
            stages,
            vec![
                "stage_open",
                "stream_started",
                "chunk_apply",
                "chunk_apply",
                "chunk_apply",
                "txn_batch",
                "end"
            ]
        );
        assert_eq!(acks[0]["source_epoch"], 7);
        assert!(
            acks.iter()
                .all(|a| a["durable"] == true && a["protocol"] == PROTO_ACK)
        );
        assert_eq!(
            lines(&h.out.join(LEDGER_FILE)).len(),
            7,
            "one ledger line per acknowledged frame"
        );
    }

    #[tokio::test]
    async fn a_crashed_producer_restarts_from_a_fresh_rebind_and_loads_only_the_unfinished_chunks()
    {
        let scratch = Scratch::new("crash");
        let h = harness(&scratch, &fresh_frames(), &resumed_frames(), Some(4), None);
        let mut port = FakePort {
            rebinds: vec![None, Some(rebind_open())],
            ..Default::default()
        };
        let done = run(&mut port, &h.plan, stage())
            .await
            .expect("run after restart");
        assert!(matches!(done, Done::Ended(_)));
        let chunk_numbers: Vec<u64> = ops_named(&port, "chunk")
            .iter()
            .map(|op| op["chunk_no"].as_u64().unwrap())
            .collect();
        assert_eq!(
            chunk_numbers,
            vec![1, 2, 3],
            "chunks 1 and 2 are not read again"
        );
        let boots = lines(&h.boots);
        assert_eq!(boots.len(), 2, "one bootstrap per producer attempt");
        assert!(
            boots[0]["resume"].is_null(),
            "the first attempt starts fresh"
        );
        assert_eq!(boots[1]["resume"]["tables"][0]["last_chunk_no"], 2);
        assert_eq!(boots[1]["resume"]["tables"][0]["last_key_hi"], "9");
        assert_eq!(
            ops_named(&port, "chunk")[2]["key_lo"],
            "9",
            "chunk 3 starts after chunk 2's key_hi"
        );
        assert_eq!(port.pids.len(), 2);
    }

    #[tokio::test]
    async fn a_transient_loader_refusal_is_replayed_and_acked_once() {
        let scratch = Scratch::new("replay");
        let h = harness(&scratch, &fresh_frames(), &resumed_frames(), None, None);
        let mut port = FakePort {
            rebinds: vec![None],
            fail_once: Some(("chunk", "target_apply")),
            ..Default::default()
        };
        let done = run(&mut port, &h.plan, stage()).await.expect("run");
        assert!(matches!(done, Done::Ended(_)));
        assert_eq!(
            ops_named(&port, "chunk").len(),
            4,
            "the refused chunk was sent again"
        );
        let chunk_acks = lines(&h.acks)
            .iter()
            .filter(|a| a["stage"] == "chunk_apply")
            .count();
        assert_eq!(chunk_acks, 3, "the producer sees one ack per frame");
    }

    #[tokio::test]
    async fn a_successor_open_passes_the_acknowledged_predecessor_to_the_loader() {
        let scratch = Scratch::new("successor-open");
        let mut h = harness(&scratch, &fresh_frames(), &resumed_frames(), None, None);
        h.plan.previous = json!({"task_id": "bbbbbbbb-cccc-dddd-eeee-ffffffffffff"});
        let mut port = FakePort { rebinds: vec![None], ..Default::default() };
        run(&mut port, &h.plan, stage()).await.expect("run");
        assert_eq!(
            ops_named(&port, "open")[0]["previous_task"],
            "bbbbbbbb-cccc-dddd-eeee-ffffffffffff",
        );
    }

    #[tokio::test]
    async fn a_refused_frame_fails_the_run_and_retires_its_loader_state() {
        let scratch = Scratch::new("refused");
        let h = harness(&scratch, &fresh_frames(), &resumed_frames(), None, None);
        let mut port = FakePort {
            rebinds: vec![None],
            fail_once: Some(("chunk", "chunk_conflict")),
            ..Default::default()
        };
        let result = run(&mut port, &h.plan, stage()).await;
        assert_eq!(result.err(), Some(Fault::Token("chunk_conflict")));
        assert_eq!(
            ops_named(&port, "retire").len(),
            1,
            "a failed run is retired (N1)"
        );
        assert_eq!(
            lines(&h.acks).len(),
            2,
            "nothing past the refused chunk is acknowledged"
        );
    }

    #[tokio::test]
    async fn a_loader_refusal_is_recorded_in_the_ledger_without_changing_the_frames() {
        let scratch = Scratch::new("refusal-ledger");
        let h = harness(&scratch, &fresh_frames(), &resumed_frames(), None, None);
        let mut port = FakePort {
            rebinds: vec![None],
            fail_once: Some(("chunk", "run_not_open")),
            ..Default::default()
        };
        let result = run(&mut port, &h.plan, stage()).await;
        assert_eq!(result.err(), Some(Fault::Token("run_not_open")));
        let all = lines(&h.out.join(LEDGER_FILE));
        let refusals: Vec<&Value> = all.iter().filter(|l| l["kind"] == "loader_refusal").collect();
        assert_eq!(refusals.len(), 1, "one refusal, one record");
        assert_eq!(refusals[0]["token"], "run_not_open");
        assert_eq!(refusals[0]["after_seq"], 2, "written after the two acknowledged frames");
        let frames = all.iter().filter(|l| l["kind"] != "loader_refusal").count();
        assert_eq!(frames, 2, "a refusal is not a frame");
    }

    #[tokio::test]
    async fn a_chunk_before_stream_started_is_refused_and_the_run_is_retired() {
        let scratch = Scratch::new("order");
        let frames = vec![
            open_frame(false, "100"),
            chunk_frame(1, Some("1"), Some("5"), 2, false),
        ];
        let h = harness(&scratch, &frames, &resumed_frames(), None, None);
        let mut port = FakePort {
            rebinds: vec![None],
            ..Default::default()
        };
        let result = run(&mut port, &h.plan, stage()).await;
        assert_eq!(result.err(), Some(Fault::Token("frame_order")));
        assert!(
            ops_named(&port, "chunk").is_empty(),
            "no chunk reaches the loader before stream_started"
        );
        assert_eq!(ops_named(&port, "retire").len(), 1);
    }

    #[tokio::test]
    async fn a_lost_lease_stops_the_run_without_retiring_it() {
        let scratch = Scratch::new("lost");
        let h = harness(&scratch, &fresh_frames(), &resumed_frames(), None, None);
        let mut port = FakePort {
            rebinds: vec![None],
            fail_once: Some(("chunk", "lease_or_barrier")),
            ..Default::default()
        };
        let result = run(&mut port, &h.plan, stage()).await;
        assert_eq!(result.err(), Some(Fault::Lost));
        assert!(
            ops_named(&port, "retire").is_empty(),
            "a lost lease may not write"
        );
    }

    #[tokio::test]
    async fn a_manifest_that_does_not_match_the_end_frame_is_refused() {
        let scratch = Scratch::new("manifest");
        let mut frames = fresh_frames();
        frames[6] = end_frame(H);
        let h = harness(&scratch, &frames, &resumed_frames(), None, None);
        let mut port = FakePort {
            rebinds: vec![None],
            ..Default::default()
        };
        let result = run(&mut port, &h.plan, stage()).await;
        assert_eq!(result.err(), Some(Fault::Token("end_manifest_sha")));
        assert_eq!(ops_named(&port, "retire").len(), 1);
    }

    #[tokio::test]
    async fn a_quiet_run_returns_the_no_change_request_and_loads_nothing() {
        let scratch = Scratch::new("quiet");
        let previous = json!({"task_id": TASK, "l_end": format!("{UUID}:1-100"), "config_digest": config_digest()});
        let quiet = json!({"kind": "quiet", "protocol": PROTO_CHUNK, "request": {
            "protocol": PROTO_NOCHANGE, "source_id": SOURCE, "source_generation": 4, "previous": previous,
            "observed_gtid": format!("{UUID}:1-100"), "connection_config_sha256": CONFIG_SHA}});
        let mut h = harness(&scratch, std::slice::from_ref(&quiet), &[], None, None);
        h.plan.previous = previous;
        let mut port = FakePort {
            rebinds: vec![None],
            ..Default::default()
        };
        let done = run(&mut port, &h.plan, stage()).await.expect("quiet");
        assert!(matches!(done, Done::Quiet(ref request) if request["protocol"] == PROTO_NOCHANGE));
        assert!(port.ops.is_empty());
    }

    #[tokio::test]
    async fn a_sealed_run_skips_the_producer_and_hands_over_the_manifest_digest() {
        let scratch = Scratch::new("sealed");
        let h = harness(
            &scratch,
            &fresh_frames(),
            &resumed_frames(),
            None,
            Some(PathBuf::from("/nonexistent/producer")),
        );
        std::fs::create_dir_all(&h.out).unwrap();
        std::fs::write(h.out.join(PARTS_MANIFEST), manifest_bytes()).unwrap();
        let mut sealed = rebind_open();
        sealed["state"] = json!("awaiting_publish");
        sealed["l_end"] = json!(format!("{UUID}:1-102"));
        sealed["l_end_pos"] = json!("102");
        let mut port = FakePort {
            rebinds: vec![Some(sealed)],
            ..Default::default()
        };
        let done = run(&mut port, &h.plan, stage()).await.expect("sealed");
        assert_eq!(
            done,
            Done::Sealed(EndFacts {
                l_end: format!("{UUID}:1-102"),
                l_end_pos: 102,
                parts_manifest_sha256: manifest_sha()
            })
        );
        assert!(
            port.pids.is_empty(),
            "no producer is started for a sealed run"
        );
    }
}
