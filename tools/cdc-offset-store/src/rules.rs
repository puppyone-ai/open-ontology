//! Closed receipt rules from target-fence-receipts/3 and the checkpoint decision.
//!
//! The decision never takes the maximum position. A hole stays a hole until
//! the caller-declared previous and next bindings name this transaction.

use std::cmp::Ordering;

use crate::model::{CheckpointRef, OrderBinding, PROVENANCE};
use crate::sha256::{hex, sha256};

pub const IDENT: &str = "ident";
pub const TXN: &str = "txn";
pub const LINEAGE: &str = "lineage";

pub const BOOTSTRAP_SHA256: &str =
    "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208";
pub const FENCE_SHA256: &str =
    "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9";
pub const PREFLIGHT_SHA256: &str =
    "d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3";
pub const HISTORICAL_V2_SHA256: &str =
    "59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512";

const CODECS: [&str; 5] = [
    "pg-commit-lsn/v1",
    "mysql-binlog-pos/v1",
    "batch-cursor-us/v1",
    "snapshot-zero/v1",
    "mariadb-gtid/v1",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub source: String,
    pub table_generation: String,
    pub txn_or_batch_id: String,
    pub config_version: String,
    pub config_digest: String,
    pub type_policy_version: String,
    pub type_digest: String,
    pub hook_digest: String,
    pub schema_digest: String,
    pub payload_digest: String,
    pub protocol_version: String,
    pub position_codec: String,
    pub position_lineage: String,
    pub source_epoch: i32,
    pub applied_pos: String,
    pub partial_txn: bool,
    pub cross_table_consistent: bool,
    pub txn_complete: bool,
    pub segment_index: Option<i32>,
    pub segment_count: Option<i32>,
    pub segment_bitmap: Vec<u8>,
    pub fence_shard: String,
    pub fence_generation: i64,
    pub target_commit_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watermark {
    pub position_codec: String,
    pub position_lineage: String,
    pub source_epoch: i32,
    pub applied_pos: String,
    pub applied_txn_id: String,
    pub declared_next_txn: String,
    pub declared_next_pos: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Checkpoint {
    Skip(&'static str),
    InsertFirst,
    CompareAndSwap,
}

pub fn codec_ok(value: &str) -> bool {
    CODECS.contains(&value)
}

pub fn pos_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=38).contains(&bytes.len())
        && bytes.iter().all(|b| b.is_ascii_digit())
        && (bytes.len() == 1 || bytes[0] != b'0')
}

pub fn cmp_pos(left: &str, right: &str) -> Ordering {
    let left = trim_zero(left);
    let right = trim_zero(right);
    left.len().cmp(&right.len()).then(left.cmp(right))
}

/// One MariaDB GTID stored as lineage `d{domain}.s{server}` and sequence
/// `applied_pos`. A set, a file position, and a MySQL `UUID:seq` are rejected.
fn maria_stored(lineage: &str, pos: &str) -> bool {
    maria_lineage(lineage).is_some() && maria_seq(pos).is_some()
}

fn maria_lineage(lineage: &str) -> Option<(u32, u32)> {
    let rest = lineage.strip_prefix('d')?;
    let (domain, server) = rest.split_once(".s")?;
    let domain = canonical_u32(domain)?;
    let server = canonical_u32(server)?;
    if server == 0 {
        return None;
    }
    Some((domain, server))
}

fn maria_seq(pos: &str) -> Option<u64> {
    if !pos_ok(pos) || pos.len() > 20 {
        return None;
    }
    pos.parse().ok()
}

fn canonical_u32(text: &str) -> Option<u32> {
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

fn position_closed(codec: &str, lineage: &str, pos: &str) -> bool {
    if codec == "mariadb-gtid/v1" {
        return maria_stored(lineage, pos);
    }
    codec_ok(codec) && token_lineage(lineage) && pos_ok(pos)
}

fn trim_zero(value: &str) -> &str {
    let trimmed = value.trim_start_matches('0');
    if trimmed.is_empty() { "0" } else { trimmed }
}

pub fn receipt_closed(receipt: &Receipt) -> bool {
    token_ident(&receipt.source)
        && token_ident(&receipt.table_generation)
        && token_txn(&receipt.txn_or_batch_id)
        && receipt.config_version == "config/v1"
        && receipt.type_policy_version == "type-policy/v1"
        && receipt.protocol_version == "batch/v1"
        && hex64(&receipt.config_digest)
        && hex64(&receipt.type_digest)
        && hex64(&receipt.hook_digest)
        && hex64(&receipt.schema_digest)
        && hex64(&receipt.payload_digest)
        && position_closed(
            &receipt.position_codec,
            &receipt.position_lineage,
            &receipt.applied_pos,
        )
        && receipt.source_epoch >= 0
        && completion_ok(receipt)
        && segment_shape(receipt)
        && token_lineage(&receipt.fence_shard)
        && receipt.fence_generation >= 1
        && timestamp_ok(&receipt.target_commit_at)
}

pub fn receipt_document(receipt: &Receipt) -> String {
    let mut text = String::new();
    text.push_str("cdc-offset-receipt/v1\n");
    push(&mut text, "source", &receipt.source);
    push(&mut text, "table_generation", &receipt.table_generation);
    push(&mut text, "txn_or_batch_id", &receipt.txn_or_batch_id);
    push(&mut text, "config_version", &receipt.config_version);
    push(&mut text, "config_digest", &receipt.config_digest);
    push(&mut text, "type_policy_version", &receipt.type_policy_version);
    push(&mut text, "type_digest", &receipt.type_digest);
    push(&mut text, "hook_digest", &receipt.hook_digest);
    push(&mut text, "schema_digest", &receipt.schema_digest);
    push(&mut text, "payload_digest", &receipt.payload_digest);
    push(&mut text, "protocol_version", &receipt.protocol_version);
    push(&mut text, "position_codec", &receipt.position_codec);
    push(&mut text, "position_lineage", &receipt.position_lineage);
    push(&mut text, "source_epoch", &receipt.source_epoch.to_string());
    push(&mut text, "applied_pos", &receipt.applied_pos);
    push(&mut text, "partial_txn", bit(receipt.partial_txn));
    push(&mut text, "cross_table_consistent", bit(receipt.cross_table_consistent));
    push(&mut text, "txn_complete", bit(receipt.txn_complete));
    push(&mut text, "segment_index", &opt_i32(receipt.segment_index));
    push(&mut text, "segment_count", &opt_i32(receipt.segment_count));
    push(&mut text, "segment_bitmap", &bitmap_text(&receipt.segment_bitmap));
    push(&mut text, "fence_shard", &receipt.fence_shard);
    push(&mut text, "fence_generation", &receipt.fence_generation.to_string());
    push(&mut text, "target_commit_at", &receipt.target_commit_at);
    text
}

pub fn receipt_hash(receipt: &Receipt) -> Option<String> {
    sha256(receipt_document(receipt).as_bytes()).map(|bytes| hex(&bytes))
}

pub fn same_batch(existing: &str, fresh: &str) -> bool {
    existing == fresh
}

pub fn migrations_ok(rows: &[(String, String, String, String)]) -> bool {
    let required = [
        ("target-bootstrap/1", BOOTSTRAP_SHA256, "0000_bootstrap_roles.sql"),
        ("target-fence-receipts/1", FENCE_SHA256, "0001_target_fence_expand.sql"),
        ("target-fence-receipts/3", PREFLIGHT_SHA256, "0003_target_segment_preflight_expand.sql"),
    ];
    for (version, checksum, filename) in required {
        if !rows.iter().any(|row| {
            row.0 == version && row.1 == checksum && row.2 == "expand" && row.3 == filename
        }) {
            return false;
        }
    }
    match rows.iter().find(|row| row.0 == "target-fence-receipts/2") {
        None => true,
        Some(row) => {
            row.1 == HISTORICAL_V2_SHA256
                && row.2 == "expand"
                && row.3 == "0002_target_segment_null_expand.sql"
        }
    }
}

/// `Err` means do not record the receipt. `Ok(Skip)` records it and leaves
/// the watermark. `InsertFirst` and `CompareAndSwap` record it and move the
/// watermark for this codec, lineage, and epoch only.
pub fn checkpoint(
    receipt: &Receipt,
    siblings: &[Receipt],
    watermark: Option<&Watermark>,
    binding: &OrderBinding,
) -> Result<Checkpoint, &'static str> {
    if !receipt_closed(receipt) {
        return Err("receipt_rule");
    }
    for sibling in siblings {
        if sibling.table_generation != receipt.table_generation && !receipt_closed(sibling) {
            return Err("receipt_rule");
        }
    }
    if binding.provenance != PROVENANCE {
        return Err("input");
    }
    if let Some(reason) = hold_reason(receipt, siblings, binding) {
        return Ok(Checkpoint::Skip(reason));
    }
    if binding.source_gap {
        return Ok(Checkpoint::Skip("source_gap"));
    }
    if binding.epoch_barrier {
        return Ok(Checkpoint::Skip("epoch_barrier"));
    }
    if let Some(previous) = &binding.expected_previous {
        if previous.source_epoch != receipt.source_epoch {
            return Ok(Checkpoint::Skip("epoch_barrier"));
        }
        if previous.position_codec != receipt.position_codec
            || previous.position_lineage != receipt.position_lineage
        {
            return Ok(Checkpoint::Skip("lineage_barrier"));
        }
    }
    match watermark {
        None => {
            if binding.expected_previous.is_some() {
                Ok(Checkpoint::Skip("expected_previous_mismatch"))
            } else {
                Ok(Checkpoint::InsertFirst)
            }
        }
        Some(mark) => bound_next(receipt, mark, binding),
    }
}

fn bound_next(receipt: &Receipt, mark: &Watermark, binding: &OrderBinding) -> Result<Checkpoint, &'static str> {
    if mark.position_codec != receipt.position_codec
        || mark.position_lineage != receipt.position_lineage
        || mark.source_epoch != receipt.source_epoch
    {
        return Ok(Checkpoint::Skip("lineage_barrier"));
    }
    if mark.applied_txn_id == receipt.txn_or_batch_id && mark.applied_pos == receipt.applied_pos {
        return Ok(Checkpoint::Skip("already_checkpoint"));
    }
    let Some(previous) = &binding.expected_previous else {
        return Ok(Checkpoint::Skip("expected_previous_mismatch"));
    };
    if !previous_matches(previous, mark) {
        return Ok(Checkpoint::Skip("expected_previous_mismatch"));
    }
    if receipt.txn_or_batch_id != mark.declared_next_txn || receipt.applied_pos != mark.declared_next_pos {
        return Ok(Checkpoint::Skip("source_gap"));
    }
    if cmp_pos(&receipt.applied_pos, &mark.applied_pos) != Ordering::Greater {
        return Ok(Checkpoint::Skip("position_regression"));
    }
    Ok(Checkpoint::CompareAndSwap)
}

fn previous_matches(previous: &CheckpointRef, mark: &Watermark) -> bool {
    previous.position_codec == mark.position_codec
        && previous.position_lineage == mark.position_lineage
        && previous.source_epoch == mark.source_epoch
        && previous.applied_pos == mark.applied_pos
        && previous.txn_or_batch_id == mark.applied_txn_id
}

fn hold_reason(receipt: &Receipt, siblings: &[Receipt], binding: &OrderBinding) -> Option<&'static str> {
    if binding.declared_transaction.applied_pos != receipt.applied_pos {
        return Some("position_unbound");
    }
    if receipt.partial_txn || !receipt.txn_complete {
        return Some("partial_segment");
    }
    if segment_incomplete(receipt) {
        return Some("segment_incomplete");
    }
    if !receipt.cross_table_consistent {
        return Some("cross_table_inconsistent");
    }
    for table in &binding.declared_transaction.required_table_generations {
        if table == &receipt.table_generation {
            continue;
        }
        let Some(sibling) = siblings.iter().find(|row| &row.table_generation == table) else {
            return Some("required_incomplete");
        };
        if sibling.partial_txn || !sibling.txn_complete || segment_incomplete(sibling) {
            return Some("partial_segment");
        }
        if !sibling.cross_table_consistent {
            return Some("cross_table_inconsistent");
        }
        if sibling.applied_pos != receipt.applied_pos
            || sibling.position_codec != receipt.position_codec
            || sibling.position_lineage != receipt.position_lineage
            || sibling.source_epoch != receipt.source_epoch
        {
            return Some("position_unbound");
        }
    }
    None
}

fn completion_ok(receipt: &Receipt) -> bool {
    (receipt.partial_txn && !receipt.txn_complete && !receipt.cross_table_consistent)
        || (!receipt.partial_txn && receipt.txn_complete && receipt.cross_table_consistent)
        || (!receipt.partial_txn && receipt.txn_complete && !receipt.cross_table_consistent)
}

fn segment_shape(receipt: &Receipt) -> bool {
    match (receipt.segment_index, receipt.segment_count) {
        (None, None) => receipt.segment_bitmap.is_empty(),
        (Some(index), Some(count)) => {
            count >= 1
                && index >= 0
                && index < count
                && (1..=8192).contains(&receipt.segment_bitmap.len())
        }
        _ => false,
    }
}

fn segment_incomplete(receipt: &Receipt) -> bool {
    let Some(count) = receipt.segment_count else {
        return false;
    };
    if count < 1 {
        return true;
    }
    (0..count).any(|bit| !bit_set(&receipt.segment_bitmap, bit))
}

fn bit_set(bitmap: &[u8], bit: i32) -> bool {
    let bit = usize::try_from(bit).unwrap_or(usize::MAX);
    let Some(byte) = bitmap.get(bit / 8) else {
        return false;
    };
    byte & (1_u8 << (bit % 8)) != 0
}

fn timestamp_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 27
        && &bytes[4..5] == b"-"
        && &bytes[7..8] == b"-"
        && &bytes[10..11] == b"T"
        && &bytes[13..14] == b":"
        && &bytes[16..17] == b":"
        && &bytes[19..20] == b"."
        && bytes[26] == b'Z'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[11..13].iter().all(u8::is_ascii_digit)
        && bytes[14..16].iter().all(u8::is_ascii_digit)
        && bytes[17..19].iter().all(u8::is_ascii_digit)
        && bytes[20..26].iter().all(u8::is_ascii_digit)
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn token_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

fn token_txn(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes.iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
}

fn token_lineage(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b':' | b'-'))
}

fn push(text: &mut String, key: &str, value: &str) {
    text.push_str(key);
    text.push('=');
    text.push_str(value);
    text.push('\n');
}

fn bit(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

fn opt_i32(value: Option<i32>) -> String {
    value.map(|item| item.to_string()).unwrap_or_else(|| "-".to_owned())
}

fn bitmap_text(bitmap: &[u8]) -> String {
    if bitmap.is_empty() {
        "-".to_owned()
    } else {
        hex(bitmap)
    }
}
