//! Restart recovery. A committed iterator is returned only after the seal,
//! manifest, and chunk files match. This module does not delete or rewrite.

use std::fs::{self, File};
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::Serialize;

use crate::jsonline::{parse_wire, read_line_capped};
use crate::model::{
    canonical_preimage, event_hash, event_hash_hex, is_chunk_name, is_hex64, validate_key,
    validate_payload, validate_timestamp, CanonicalData, Cell, Fault, FaultCode, Limits, Op, Wire,
    CONTRACT, MAX_CHUNK_BYTES, MAX_EVENTS, PROVENANCE,
};
use crate::sha256::{hex, sha256};

const MANIFEST_CAP: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestLimits {
    pub max_events: u64,
    pub max_chunk_events: u64,
    pub max_chunk_bytes: u64,
    pub max_disk_bytes: u64,
    pub max_files: u64,
    pub max_line_bytes: u64,
}

impl ManifestLimits {
    pub(crate) fn from_limits(limits: &Limits) -> Self {
        Self {
            max_events: limits.max_events,
            max_chunk_events: limits.max_chunk_events,
            max_chunk_bytes: limits.max_chunk_bytes,
            max_disk_bytes: limits.max_disk_bytes,
            max_files: limits.max_files,
            max_line_bytes: u64::try_from(limits.max_line_bytes).unwrap_or(u64::MAX),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkRec {
    pub name: String,
    pub events: u64,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub contract: String,
    pub kind: String,
    pub source: String,
    pub lineage: String,
    pub epoch: u32,
    pub schema_hash: String,
    pub config_hash: String,
    pub type_hash: String,
    pub hook_hashes: Vec<String>,
    pub selected_tables: Vec<String>,
    pub commit_position_provenance: String,
    pub txn_id: String,
    pub count: u64,
    pub per_collection_counts: std::collections::BTreeMap<String, u64>,
    pub first_sequence: Option<u64>,
    pub last_sequence: Option<u64>,
    pub commit_lsn: String,
    pub commit_position: String,
    pub source_commit_timestamp: String,
    pub selected_row_count: u64,
    pub chunks: Vec<ChunkRec>,
    pub chunk_set_sha256: String,
    pub limits: ManifestLimits,
    pub ack_allowed: bool,
    pub target_applied: bool,
    pub cross_table_consistent: bool,
    pub applied: serde_json::Value,
    pub upstream_commit_lsn_verified: bool,
    pub debezium_normalizer: bool,
    pub p2_verified: bool,
    pub p11_verified: bool,
}

#[derive(Debug)]
pub struct Committed {
    pub manifest: Manifest,
    dir: PathBuf,
}

impl Committed {
    pub fn ack_allowed(&self) -> bool {
        false
    }

    pub fn iter(&self) -> Result<ChunkIter, Fault> {
        ChunkIter::open(&self.dir, &self.manifest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub durable_chunks: u64,
    pub durable_events: u64,
    pub ack_allowed: bool,
    pub target_applied: bool,
    pub cross_table_consistent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolledBack {
    pub txn_id: Option<String>,
    pub ack_allowed: bool,
    pub target_applied: bool,
    pub cross_table_consistent: bool,
}

#[derive(Debug)]
pub enum Recovery {
    Committed(Committed),
    RolledBack(RolledBack),
    Incomplete(Pending),
    Unknown { reason: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub global_txn_sequence: u64,
    pub data_collection_sequence: u64,
    pub table: String,
    pub key: Vec<Cell>,
    pub op: Op,
    pub payload: Vec<Cell>,
    pub unavailable_ordinals: Vec<u16>,
    pub preimage: String,
    pub event_hash: String,
}

pub struct ChunkIter {
    dir: PathBuf,
    chunks: Vec<ChunkRec>,
    chunk_index: usize,
    lines: Vec<String>,
    line_index: usize,
    loaded: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredLine {
    data_collection_sequence: u64,
    global_txn_sequence: u64,
    key: Vec<Cell>,
    op: Op,
    payload: Vec<Cell>,
    table: String,
    txn_id: String,
    unavailable_ordinals: Vec<crate::model::Ordinal>,
}

enum Marker {
    Missing,
    Exact,
    Torn,
    Symlink,
}

pub fn recover(dir: &Path) -> Result<Recovery, Fault> {
    let meta = fs::symlink_metadata(dir).map_err(|_| fault(FaultCode::Io, None))?;
    if meta.file_type().is_symlink() {
        return Err(fault(FaultCode::SymlinkRefused, None));
    }
    if !meta.is_dir() {
        return Err(fault(FaultCode::Io, None));
    }
    let seal = marker(dir, "SEAL", None)?;
    let rolled = marker(dir, "ROLLED_BACK", Some(b"rolled_back\n"))?;
    match seal {
        Marker::Symlink => Err(fault(FaultCode::SymlinkRefused, None)),
        Marker::Torn => Ok(Recovery::Unknown { reason: "seal_torn" }),
        Marker::Exact if matches!(rolled, Marker::Exact | Marker::Torn) => {
            Ok(Recovery::Unknown { reason: "seal_and_rollback" })
        }
        Marker::Exact => open_committed(dir),
        Marker::Missing => match rolled {
            Marker::Exact => Ok(Recovery::RolledBack(RolledBack {
                txn_id: progress_txn(dir),
                ack_allowed: false,
                target_applied: false,
                cross_table_consistent: false,
            })),
            Marker::Symlink => Err(fault(FaultCode::SymlinkRefused, None)),
            Marker::Torn => Ok(Recovery::Unknown {
                reason: "rollback_torn",
            }),
            Marker::Missing => Ok(incomplete_or_unknown(dir)),
        },
    }
}

pub fn verify_replay<R: BufRead>(dir: &Path, mut reader: R) -> Result<(), Fault> {
    let Recovery::Committed(committed) = recover(dir)? else {
        return Err(fault(FaultCode::NotCommitted, None));
    };
    let count = committed.manifest.count;
    let slots = usize::try_from(count)
        .map_err(|_| fault(FaultCode::CountOverflow, Some(committed.manifest.txn_id.clone())))?
        .checked_add(1)
        .ok_or_else(|| fault(FaultCode::CountOverflow, Some(committed.manifest.txn_id.clone())))?;
    if slots > usize::try_from(MAX_EVENTS).unwrap_or(usize::MAX).saturating_add(1) {
        return Err(fault(FaultCode::LimitRaised, None));
    }
    let mut stored = vec![[0u8; 32]; slots];
    let mut have = vec![false; slots];
    let txn_id = committed.manifest.txn_id.clone();
    {
        let mut iter = committed.iter()?;
        while let Some(item) = iter.next() {
            let event = item?;
            let idx = usize::try_from(event.global_txn_sequence)
                .map_err(|_| fault(FaultCode::OutputMismatch, Some(txn_id.clone())))?;
            if idx == 0 || idx >= slots {
                return Err(fault(FaultCode::OutputMismatch, Some(txn_id.clone())));
            }
            stored[idx] = event_hash(&event.preimage)
                .map_err(|code| fault(code, Some(txn_id.clone())))?;
            have[idx] = true;
        }
    }
    let mut body = Vec::new();
    let mut seen_bind = false;
    let mut seen_begin = false;
    let mut seen_end = false;
    let mut feed_seen = vec![false; slots];
    let max_line = usize::try_from(committed.manifest.limits.max_line_bytes).unwrap_or(usize::MAX);
    loop {
        let Some(bytes) = read_line_capped(&mut reader, max_line, &mut body)
            .map_err(|code| fault(code, Some(txn_id.clone())))?
        else {
            break;
        };
        let _ = bytes;
        if body.contains(&b'\r') {
            return Err(fault(FaultCode::Malformed, Some(txn_id)));
        }
        let line = std::str::from_utf8(&body).map_err(|_| fault(FaultCode::Malformed, Some(txn_id.clone())))?;
        let wire = parse_wire(line).map_err(|code| fault(code, Some(txn_id.clone())))?;
        match wire {
            Wire::Bind {
                source,
                lineage,
                epoch,
                selected_tables,
                schema_hash,
                config_hash,
                type_hash,
                hook_hashes,
                commit_position_provenance,
            } => {
                if seen_bind {
                    return Err(fault(FaultCode::BindDuplicate, Some(txn_id)));
                }
                let manifest = &committed.manifest;
                if source != manifest.source
                    || lineage != manifest.lineage
                    || epoch != manifest.epoch
                    || selected_tables != manifest.selected_tables
                    || schema_hash != manifest.schema_hash
                    || config_hash != manifest.config_hash
                    || type_hash != manifest.type_hash
                    || hook_hashes != manifest.hook_hashes
                    || commit_position_provenance != manifest.commit_position_provenance
                {
                    return Err(fault(FaultCode::PayloadConflict, Some(txn_id)));
                }
                seen_bind = true;
            }
            Wire::Begin { txn_id: id } => {
                if !seen_bind || seen_end {
                    return Err(fault(FaultCode::SingleStream, Some(txn_id)));
                }
                if id != txn_id {
                    return Err(fault(FaultCode::PayloadConflict, Some(id)));
                }
                seen_begin = true;
            }
            Wire::Data {
                txn_id: id,
                global_txn_sequence,
                data_collection_sequence,
                table,
                key,
                op,
                payload,
                unavailable_ordinals,
            } => {
                if !seen_begin || seen_end || id != txn_id {
                    return Err(fault(FaultCode::PayloadConflict, Some(id)));
                }
                let unavailable: Vec<u16> = unavailable_ordinals.iter().map(|item| item.0).collect();
                let preimage = canonical_preimage(&CanonicalData {
                    txn_id: &id,
                    global_txn_sequence,
                    data_collection_sequence,
                    table: &table,
                    key: &key,
                    op,
                    payload: &payload,
                    unavailable_ordinals: &unavailable,
                })
                .map_err(|code| fault(code, Some(txn_id.clone())))?;
                let digest = event_hash(&preimage).map_err(|code| fault(code, Some(txn_id.clone())))?;
                let idx = usize::try_from(global_txn_sequence)
                    .map_err(|_| fault(FaultCode::PayloadConflict, Some(txn_id.clone())))?;
                if idx == 0 || idx >= slots || !have[idx] || stored[idx] != digest {
                    return Err(fault(FaultCode::PayloadConflict, Some(txn_id)));
                }
                feed_seen[idx] = true;
            }
            Wire::End {
                txn_id: id,
                count: end_count,
                per_collection_counts,
                commit_lsn,
                source_commit_timestamp,
            } => {
                if seen_end || id != txn_id {
                    return Err(fault(FaultCode::EndConflict, Some(id)));
                }
                let manifest = &committed.manifest;
                if end_count != manifest.count
                    || per_collection_counts != manifest.per_collection_counts
                    || commit_lsn != manifest.commit_lsn
                    || source_commit_timestamp != manifest.source_commit_timestamp
                {
                    return Err(fault(FaultCode::PayloadConflict, Some(txn_id)));
                }
                seen_end = true;
            }
            Wire::Rollback { txn_id: id } => {
                return Err(fault(FaultCode::PayloadConflict, Some(id)));
            }
            Wire::Barrier { .. } => return Err(fault(FaultCode::SingleStream, Some(txn_id))),
        }
    }
    if !seen_end {
        return Err(fault(FaultCode::NotCommitted, Some(txn_id)));
    }
    for (idx, present) in have.iter().enumerate().skip(1) {
        if *present && !feed_seen[idx] {
            return Err(fault(FaultCode::PayloadConflict, Some(txn_id)));
        }
    }
    Ok(())
}

fn open_committed(dir: &Path) -> Result<Recovery, Fault> {
    let seal_bytes = read_regular(dir, "SEAL", 65)?;
    if seal_bytes.len() != 65 || seal_bytes[64] != b'\n' || !is_lower_hex(&seal_bytes[..64]) {
        return Ok(Recovery::Unknown { reason: "seal_torn" });
    }
    let manifest_bytes = read_regular(dir, "manifest.json", MANIFEST_CAP)?;
    let digest = sha256(&manifest_bytes).ok_or_else(|| fault(FaultCode::Io, None))?;
    if hex(&digest).as_bytes() != &seal_bytes[..64] {
        return Err(fault(FaultCode::ChecksumFault, None));
    }
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| fault(FaultCode::OutputMismatch, None))?;
    check_manifest(&manifest)?;
    let index = read_regular(dir, "chunks.idx", MANIFEST_CAP)?;
    let indexed = parse_index(&index).map_err(|code| fault(code, Some(manifest.txn_id.clone())))?;
    if indexed != manifest.chunks {
        return Err(fault(FaultCode::ChecksumFault, Some(manifest.txn_id.clone())));
    }
    let mut events = 0u64;
    let mut set = String::new();
    for chunk in &manifest.chunks {
        if !is_chunk_name(&chunk.name) || chunk.bytes > MAX_CHUNK_BYTES {
            return Err(fault(FaultCode::ChecksumFault, Some(manifest.txn_id.clone())));
        }
        let bytes = read_regular(dir, &chunk.name, chunk.bytes)?;
        if bytes.len() as u64 != chunk.bytes {
            return Err(fault(FaultCode::Truncated, Some(manifest.txn_id.clone())));
        }
        let got = sha256(&bytes).ok_or_else(|| fault(FaultCode::Io, Some(manifest.txn_id.clone())))?;
        if hex(&got) != chunk.sha256 || !is_hex64(&chunk.sha256) {
            return Err(fault(FaultCode::ChecksumFault, Some(manifest.txn_id.clone())));
        }
        events = events
            .checked_add(chunk.events)
            .ok_or_else(|| fault(FaultCode::CountOverflow, Some(manifest.txn_id.clone())))?;
        set.push_str(&chunk.sha256);
        set.push('\n');
    }
    if events != manifest.count {
        return Err(fault(FaultCode::OutputMismatch, Some(manifest.txn_id.clone())));
    }
    let set_hash = hex(&sha256(set.as_bytes()).ok_or_else(|| fault(FaultCode::Io, None))?);
    if set_hash != manifest.chunk_set_sha256 {
        return Err(fault(FaultCode::ChecksumFault, Some(manifest.txn_id.clone())));
    }
    Ok(Recovery::Committed(Committed {
        manifest,
        dir: dir.to_path_buf(),
    }))
}

fn check_manifest(manifest: &Manifest) -> Result<(), Fault> {
    let txn = Some(manifest.txn_id.clone());
    if manifest.contract != CONTRACT || manifest.kind != "committed_manifest" {
        return Err(fault(FaultCode::OutputMismatch, txn));
    }
    if manifest.commit_position_provenance != PROVENANCE {
        return Err(fault(FaultCode::OutputMismatch, txn));
    }
    if manifest.ack_allowed
        || manifest.target_applied
        || manifest.cross_table_consistent
        || manifest.upstream_commit_lsn_verified
        || manifest.debezium_normalizer
        || manifest.p2_verified
        || manifest.p11_verified
        || !manifest.applied.is_null()
    {
        return Err(fault(FaultCode::AttestationRefused, txn));
    }
    let product = Limits::product();
    if manifest.limits.max_events > product.max_events
        || manifest.limits.max_chunk_events > product.max_chunk_events
        || manifest.limits.max_chunk_bytes > product.max_chunk_bytes
        || manifest.limits.max_disk_bytes > product.max_disk_bytes
        || manifest.limits.max_files > product.max_files
        || manifest.limits.max_line_bytes > u64::try_from(product.max_line_bytes).unwrap_or(u64::MAX)
    {
        return Err(fault(FaultCode::LimitRaised, txn));
    }
    validate_timestamp(&manifest.source_commit_timestamp).map_err(|code| fault(code, txn.clone()))?;
    let lsn = crate::lsn::parse_commit_lsn(&manifest.commit_lsn).map_err(|code| fault(code, txn.clone()))?;
    let position =
        crate::lsn::encode_position(manifest.epoch, lsn).map_err(|code| fault(code, txn.clone()))?;
    if position != manifest.commit_position {
        return Err(fault(FaultCode::OutputMismatch, txn));
    }
    if manifest.count == 0 {
        if manifest.first_sequence.is_some() || manifest.last_sequence.is_some() {
            return Err(fault(FaultCode::OutputMismatch, txn));
        }
    } else if manifest.first_sequence != Some(1) || manifest.last_sequence != Some(manifest.count) {
        return Err(fault(FaultCode::OutputMismatch, txn));
    }
    if manifest.count > manifest.limits.max_events {
        return Err(fault(FaultCode::EventCap, txn));
    }
    Ok(())
}

fn incomplete_or_unknown(dir: &Path) -> Recovery {
    let names = ["chunks.idx", "progress.json", "manifest.json", "chunk-000001.partial"];
    let mut owned = names.iter().any(|name| fs::symlink_metadata(dir.join(name)).is_ok());
    if !owned {
        owned = (1..=96u64).any(|index| {
            fs::symlink_metadata(dir.join(format!("chunk-{index:06}"))).is_ok()
                || fs::symlink_metadata(dir.join(format!("chunk-{index:06}.partial"))).is_ok()
        });
    }
    if !owned {
        return Recovery::Unknown {
            reason: "no_spool_files",
        };
    }
    let (chunks, events) = index_durable(dir);
    Recovery::Incomplete(Pending {
        durable_chunks: chunks,
        durable_events: events,
        ack_allowed: false,
        target_applied: false,
        cross_table_consistent: false,
    })
}

fn index_durable(dir: &Path) -> (u64, u64) {
    let Ok(bytes) = read_regular(dir, "chunks.idx", MANIFEST_CAP) else {
        return (0, 0);
    };
    let Ok(rows) = parse_index(&bytes) else {
        return (0, 0);
    };
    let mut chunks = 0u64;
    let mut events = 0u64;
    for row in rows {
        let Ok(file) = read_regular(dir, &row.name, row.bytes) else {
            continue;
        };
        if file.len() as u64 != row.bytes {
            continue;
        }
        let Some(digest) = sha256(&file) else {
            continue;
        };
        if hex(&digest) != row.sha256 {
            continue;
        }
        chunks += 1;
        events = events.saturating_add(row.events);
    }
    (chunks, events)
}

fn progress_txn(dir: &Path) -> Option<String> {
    let bytes = read_regular(dir, "progress.json", 65_536).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.get("txn_id")?.as_str().map(str::to_owned)
}

fn parse_index(bytes: &[u8]) -> Result<Vec<ChunkRec>, FaultCode> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(b"\n") {
        return Err(FaultCode::ChecksumFault);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| FaultCode::ChecksumFault)?;
    let mut rows = Vec::new();
    for line in text.lines() {
        let mut parts = line.split(' ');
        let name = parts.next().ok_or(FaultCode::ChecksumFault)?;
        let bytes_text = parts.next().ok_or(FaultCode::ChecksumFault)?;
        let events_text = parts.next().ok_or(FaultCode::ChecksumFault)?;
        let hash = parts.next().ok_or(FaultCode::ChecksumFault)?;
        if parts.next().is_some() || !is_chunk_name(name) || !is_hex64(hash) {
            return Err(FaultCode::ChecksumFault);
        }
        let bytes = parse_u64(bytes_text)?;
        let events = parse_u64(events_text)?;
        rows.push(ChunkRec {
            name: name.to_owned(),
            events,
            bytes,
            sha256: hash.to_owned(),
        });
    }
    Ok(rows)
}

fn parse_u64(text: &str) -> Result<u64, FaultCode> {
    if text.is_empty() || (text.len() > 1 && text.starts_with('0')) || !text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(FaultCode::ChecksumFault);
    }
    text.parse().map_err(|_| FaultCode::ChecksumFault)
}

fn marker(dir: &Path, name: &str, expect: Option<&[u8]>) -> Result<Marker, Fault> {
    let path = dir.join(name);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Marker::Missing),
        Err(_) => return Err(fault(FaultCode::Io, None)),
    };
    if meta.file_type().is_symlink() {
        return Ok(Marker::Symlink);
    }
    if !meta.is_file() {
        return Ok(Marker::Torn);
    }
    let Some(expect) = expect else {
        return Ok(Marker::Exact);
    };
    if meta.len() != expect.len() as u64 {
        return Ok(Marker::Torn);
    }
    let bytes = fs::read(&path).map_err(|_| fault(FaultCode::Io, None))?;
    if bytes == expect {
        Ok(Marker::Exact)
    } else {
        Ok(Marker::Torn)
    }
}

fn read_regular(dir: &Path, name: &str, cap: u64) -> Result<Vec<u8>, Fault> {
    if name.contains('/') || name.contains('\\') || name == ".." || name == "." {
        return Err(fault(FaultCode::ChecksumFault, None));
    }
    let path = dir.join(name);
    let meta = fs::symlink_metadata(&path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            fault(FaultCode::MissingChunk, None)
        } else {
            fault(FaultCode::Io, None)
        }
    })?;
    if meta.file_type().is_symlink() {
        return Err(fault(FaultCode::SymlinkRefused, None));
    }
    if !meta.is_file() {
        return Err(fault(FaultCode::MissingChunk, None));
    }
    if meta.len() > cap {
        return Err(fault(FaultCode::Truncated, None));
    }
    let mut file = File::open(&path).map_err(|_| fault(FaultCode::Io, None))?;
    let mut bytes = Vec::new();
    file.take(cap)
        .read_to_end(&mut bytes)
        .map_err(|_| fault(FaultCode::Io, None))?;
    if bytes.len() as u64 != meta.len() {
        return Err(fault(FaultCode::Truncated, None));
    }
    Ok(bytes)
}

fn is_lower_hex(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn fault(code: FaultCode, txn_id: Option<String>) -> Fault {
    Fault::new(code, txn_id)
}

impl ChunkIter {
    fn open(dir: &Path, manifest: &Manifest) -> Result<Self, Fault> {
        Ok(Self {
            dir: dir.to_path_buf(),
            chunks: manifest.chunks.clone(),
            chunk_index: 0,
            lines: Vec::new(),
            line_index: 0,
            loaded: false,
        })
    }

    fn load(&mut self) -> Result<(), Fault> {
        if self.chunk_index >= self.chunks.len() {
            self.loaded = true;
            self.lines.clear();
            return Ok(());
        }
        let chunk = self.chunks[self.chunk_index].clone();
        let bytes = read_regular(&self.dir, &chunk.name, chunk.bytes)?;
        if bytes.len() as u64 != chunk.bytes {
            return Err(fault(FaultCode::OutputMismatch, None));
        }
        let digest = sha256(&bytes).ok_or_else(|| fault(FaultCode::Io, None))?;
        if hex(&digest) != chunk.sha256 {
            return Err(fault(FaultCode::OutputMismatch, None));
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| fault(FaultCode::OutputMismatch, None))?;
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            return Err(fault(FaultCode::OutputMismatch, None));
        }
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        if lines.len() as u64 != chunk.events {
            return Err(fault(FaultCode::OutputMismatch, None));
        }
        self.lines = lines;
        self.line_index = 0;
        self.loaded = true;
        Ok(())
    }
}

impl Iterator for ChunkIter {
    type Item = Result<Event, Fault>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if !self.loaded {
                if let Err(err) = self.load() {
                    return Some(Err(err));
                }
            }
            if self.line_index < self.lines.len() {
                let line = self.lines[self.line_index].clone();
                self.line_index += 1;
                return Some(parse_event(&line));
            }
            if self.chunk_index + 1 >= self.chunks.len() && self.loaded {
                return None;
            }
            if self.chunk_index >= self.chunks.len() {
                return None;
            }
            self.chunk_index += 1;
            self.loaded = false;
        }
    }
}

fn parse_event(line: &str) -> Result<Event, Fault> {
    let parsed: StoredLine = match serde_json::from_str(line) {
        Ok(parsed) => parsed,
        Err(err) => {
            let code = if err.to_string().contains("ordinal_range") {
                FaultCode::OrdinalRange
            } else {
                FaultCode::OutputMismatch
            };
            return Err(fault(code, None));
        }
    };
    let unavailable: Vec<u16> = parsed.unavailable_ordinals.iter().map(|item| item.0).collect();
    validate_key(&parsed.key).map_err(|code| fault(code, Some(parsed.txn_id.clone())))?;
    validate_payload(&parsed.payload, &unavailable)
        .map_err(|code| fault(code, Some(parsed.txn_id.clone())))?;
    let preimage = canonical_preimage(&CanonicalData {
        txn_id: &parsed.txn_id,
        global_txn_sequence: parsed.global_txn_sequence,
        data_collection_sequence: parsed.data_collection_sequence,
        table: &parsed.table,
        key: &parsed.key,
        op: parsed.op,
        payload: &parsed.payload,
        unavailable_ordinals: &unavailable,
    })
    .map_err(|code| fault(code, Some(parsed.txn_id.clone())))?;
    if preimage != line {
        return Err(fault(FaultCode::OutputMismatch, Some(parsed.txn_id)));
    }
    let event_hash = event_hash_hex(&preimage).map_err(|code| fault(code, Some(parsed.txn_id.clone())))?;
    Ok(Event {
        global_txn_sequence: parsed.global_txn_sequence,
        data_collection_sequence: parsed.data_collection_sequence,
        table: parsed.table,
        key: parsed.key,
        op: parsed.op,
        payload: parsed.payload,
        unavailable_ordinals: unavailable,
        preimage,
        event_hash,
    })
}
