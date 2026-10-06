//! One transaction directory. Payloads are written through and not retained.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::jsonline::{parse_wire, read_line_capped};
use crate::lsn::{encode_position, parse_commit_lsn};
use crate::model::{
    canonical_preimage, chunk_name, event_hash, validate_bind, validate_key, validate_payload,
    validate_table, validate_timestamp, validate_txn_id, Bind, CanonicalData, Fault, FaultCode,
    Limits, Wire, CONTRACT, INDEX_ALLOWANCE, PROGRESS_ALLOWANCE, PROVENANCE,
};
use crate::recover::{ChunkRec, Manifest, ManifestLimits};
use crate::sha256::{hex, sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkpoint {
    BeforeChunkSync,
    BeforeEnd,
    BeforeFinalSeal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealSummary {
    pub txn_id: String,
    pub count: u64,
    pub commit_lsn: String,
    pub commit_position: String,
    pub durable_events: u64,
    pub decoded_events: u64,
    pub ack_allowed: bool,
    pub target_applied: bool,
    pub cross_table_consistent: bool,
    pub resident_hwm_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub kind: OutcomeKind,
    pub progress: ProgressView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutcomeKind {
    Sealed(SealSummary),
    RolledBack { txn_id: String },
    Incomplete { txn_id: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressView {
    pub read_lines: u64,
    pub read_bytes: u64,
    pub durable_chunks: u64,
    pub durable_events: u64,
    pub durable_bytes: u64,
    pub decoded_events: u64,
    pub state: &'static str,
    pub applied: Option<()>,
    pub ack_allowed: bool,
    pub target_applied: bool,
    pub cross_table_consistent: bool,
    pub seen_begin: u64,
    pub seen_data: u64,
    pub seen_end: u64,
    pub seen_rollback: u64,
    pub duplicate_data: u64,
    pub duplicate_begin: u64,
    pub duplicate_end: u64,
    pub duplicate_rollback: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    NeedBind,
    NeedBegin,
    Open,
    Sealed,
    RolledBack,
    Failed,
}

struct Bits {
    words: Vec<u64>,
    count: u64,
    min: u64,
    max: u64,
}

impl Bits {
    fn new() -> Self {
        Self {
            words: Vec::new(),
            count: 0,
            min: 0,
            max: 0,
        }
    }

    fn insert(&mut self, value: u64, cap: u64) -> Result<bool, FaultCode> {
        if value == 0 || value > cap {
            return Err(FaultCode::SequenceOutOfRange);
        }
        let idx = usize::try_from(value / 64).map_err(|_| FaultCode::SequenceOutOfRange)?;
        let bit = u32::try_from(value & 63).map_err(|_| FaultCode::SequenceOutOfRange)?;
        let word_cap = usize::try_from(cap / 64 + 1).map_err(|_| FaultCode::SequenceOutOfRange)?;
        if idx >= word_cap {
            return Err(FaultCode::SequenceOutOfRange);
        }
        if self.words.len() <= idx {
            let mut new_len = self.words.len().max(1);
            while new_len <= idx {
                let grown = new_len.saturating_mul(2).min(word_cap);
                new_len = if grown <= idx { idx + 1 } else { grown };
            }
            self.words.resize(new_len, 0);
        }
        let mask = 1u64 << bit;
        if self.words[idx] & mask != 0 {
            return Ok(false);
        }
        self.words[idx] |= mask;
        self.count += 1;
        if self.count == 1 {
            self.min = value;
            self.max = value;
        } else {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        Ok(true)
    }
}

struct EndIdentity {
    count: u64,
    per_collection: BTreeMap<String, u64>,
    commit_lsn: String,
    timestamp: String,
}

pub struct Spool {
    dir: PathBuf,
    limits: Limits,
    phase: Phase,
    checkpoint: Option<Box<dyn FnMut(Checkpoint) -> Result<(), Fault>>>,
    bind: Option<Bind>,
    selected: BTreeSet<String>,
    txn_id: Option<String>,
    end: Option<EndIdentity>,
    buffer: Vec<u8>,
    buffer_events: u64,
    chunks: Vec<ChunkRec>,
    chunk_set_body: String,
    files: u64,
    disk: u64,
    synced_payload: u64,
    index_created: bool,
    progress_created: bool,
    progress_reserved: bool,
    progress_accounted: u64,
    admitted: u64,
    decoded_events: u64,
    selected_rows: u64,
    global: Bits,
    hashes: Vec<[u8; 32]>,
    collections: BTreeMap<String, Bits>,
    read_lines: u64,
    read_bytes: u64,
    seen_begin: u64,
    seen_data: u64,
    seen_end: u64,
    seen_rollback: u64,
    duplicate_data: u64,
    duplicate_begin: u64,
    duplicate_end: u64,
    duplicate_rollback: u64,
}

impl Spool {
    pub fn create_new(dir: &Path, limits: Limits) -> Result<Self, Fault> {
        let limits = limits.checked().map_err(|code| Fault::new(code, None))?;
        let parent = dir
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| Fault::new(FaultCode::Io, None))?;
        let parent_meta = fs::symlink_metadata(parent).map_err(|_| Fault::new(FaultCode::Io, None))?;
        if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
            return Err(Fault::new(FaultCode::SymlinkRefused, None));
        }
        if fs::symlink_metadata(dir).is_ok() {
            return Err(Fault::new(FaultCode::PathExists, None));
        }
        if let Err(err) = fs::create_dir(dir) {
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                return Err(Fault::new(FaultCode::PathExists, None));
            }
            return Err(Fault::new(FaultCode::Io, None));
        }
        let created = fs::symlink_metadata(dir).map_err(|_| Fault::new(FaultCode::Io, None))?;
        if created.file_type().is_symlink() || !created.is_dir() {
            return Err(Fault::new(FaultCode::PathExists, None));
        }
        sync_dir(parent).map_err(|code| Fault::new(code, None))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            limits,
            phase: Phase::NeedBind,
            checkpoint: None,
            bind: None,
            selected: BTreeSet::new(),
            txn_id: None,
            end: None,
            buffer: Vec::new(),
            buffer_events: 0,
            chunks: Vec::new(),
            chunk_set_body: String::new(),
            files: 0,
            disk: 0,
            synced_payload: 0,
            index_created: false,
            progress_created: false,
            progress_reserved: false,
            progress_accounted: 0,
            admitted: 0,
            decoded_events: 0,
            selected_rows: 0,
            global: Bits::new(),
            hashes: Vec::new(),
            collections: BTreeMap::new(),
            read_lines: 0,
            read_bytes: 0,
            seen_begin: 0,
            seen_data: 0,
            seen_end: 0,
            seen_rollback: 0,
            duplicate_data: 0,
            duplicate_begin: 0,
            duplicate_end: 0,
            duplicate_rollback: 0,
        })
    }

    pub fn set_checkpoint<F>(&mut self, hook: F)
    where
        F: FnMut(Checkpoint) -> Result<(), Fault> + 'static,
    {
        self.checkpoint = Some(Box::new(hook));
    }

    pub fn progress(&self) -> ProgressView {
        ProgressView {
            read_lines: self.read_lines,
            read_bytes: self.read_bytes,
            durable_chunks: u64::try_from(self.chunks.len()).unwrap_or(u64::MAX),
            durable_events: self.chunks.iter().fold(0u64, |total, chunk| {
                total.saturating_add(chunk.events)
            }),
            durable_bytes: self.synced_payload,
            decoded_events: self.decoded_events,
            state: match self.phase {
                Phase::Sealed => "sealed",
                Phase::RolledBack => "rolled_back",
                Phase::Failed => "failed",
                _ => "open",
            },
            applied: None,
            ack_allowed: false,
            target_applied: false,
            cross_table_consistent: false,
            seen_begin: self.seen_begin,
            seen_data: self.seen_data,
            seen_end: self.seen_end,
            seen_rollback: self.seen_rollback,
            duplicate_data: self.duplicate_data,
            duplicate_begin: self.duplicate_begin,
            duplicate_end: self.duplicate_end,
            duplicate_rollback: self.duplicate_rollback,
        }
    }

    pub fn ingest_reader<R: BufRead>(&mut self, mut reader: R) -> Result<Outcome, Fault> {
        let mut body = Vec::new();
        loop {
            let Some(presented) = read_line_capped(&mut reader, self.limits.max_line_bytes, &mut body)
                .map_err(|code| self.fail(code))?
            else {
                break;
            };
            self.read_lines = self
                .read_lines
                .checked_add(1)
                .ok_or_else(|| self.fail(FaultCode::CountOverflow))?;
            self.read_bytes = self
                .read_bytes
                .checked_add(u64::try_from(presented).unwrap_or(u64::MAX))
                .ok_or_else(|| self.fail(FaultCode::CountOverflow))?;
            if body.contains(&b'\r') {
                return Err(self.fail(FaultCode::Malformed));
            }
            let line = std::str::from_utf8(&body).map_err(|_| self.fail(FaultCode::Malformed))?;
            self.ingest_line(line)?;
        }
        self.finish()
    }

    fn ingest_line(&mut self, line: &str) -> Result<(), Fault> {
        if self.phase == Phase::Failed {
            return Err(self.fail(FaultCode::StreamClosed));
        }
        let wire = parse_wire(line).map_err(|code| self.fail(code))?;
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
            } => self.apply_bind(Bind {
                source,
                lineage,
                epoch,
                selected_tables,
                schema_hash,
                config_hash,
                type_hash,
                hook_hashes,
            }, commit_position_provenance),
            Wire::Barrier { .. } => Err(self.fail(FaultCode::SingleStream)),
            Wire::Begin { txn_id } => self.apply_begin(txn_id),
            Wire::Data {
                txn_id,
                global_txn_sequence,
                data_collection_sequence,
                table,
                key,
                op,
                payload,
                unavailable_ordinals,
            } => {
                let unavailable: Vec<u16> = unavailable_ordinals.into_iter().map(|item| item.0).collect();
                self.apply_data(
                    txn_id,
                    global_txn_sequence,
                    data_collection_sequence,
                    table,
                    key,
                    op,
                    payload,
                    unavailable,
                )
            }
            Wire::End {
                txn_id,
                count,
                per_collection_counts,
                commit_lsn,
                source_commit_timestamp,
            } => self.apply_end(
                txn_id,
                count,
                per_collection_counts,
                commit_lsn,
                source_commit_timestamp,
            ),
            Wire::Rollback { txn_id } => self.apply_rollback(txn_id),
        }
    }

    fn apply_bind(&mut self, bind: Bind, provenance: String) -> Result<(), Fault> {
        if self.phase != Phase::NeedBind {
            return Err(self.fail(FaultCode::BindDuplicate));
        }
        validate_bind(&bind, &provenance).map_err(|code| self.fail(code))?;
        self.selected = bind.selected_tables.iter().cloned().collect();
        self.bind = Some(bind);
        self.phase = Phase::NeedBegin;
        Ok(())
    }

    fn apply_begin(&mut self, txn_id: String) -> Result<(), Fault> {
        validate_txn_id(&txn_id).map_err(|code| self.fail(code))?;
        match self.phase {
            Phase::NeedBind => return Err(self.fail(FaultCode::BindRequired)),
            Phase::Sealed | Phase::RolledBack => return Err(self.fail(FaultCode::SingleStream)),
            Phase::Open => {
                if self.txn_id.as_deref() == Some(txn_id.as_str()) {
                    self.bump_duplicate_begin()?;
                    return Ok(());
                }
                return Err(self.fail(FaultCode::ActiveTxnExceeded));
            }
            Phase::NeedBegin => {}
            Phase::Failed => return Err(self.fail(FaultCode::StreamClosed)),
        }
        self.txn_id = Some(txn_id);
        self.phase = Phase::Open;
        self.seen_begin = self
            .seen_begin
            .checked_add(1)
            .ok_or_else(|| self.fail(FaultCode::CountOverflow))?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments, clippy::cognitive_complexity)]
    fn apply_data(
        &mut self,
        txn_id: String,
        global: u64,
        collection_seq: u64,
        table: String,
        key: Vec<crate::model::Cell>,
        op: crate::model::Op,
        payload: Vec<crate::model::Cell>,
        unavailable: Vec<u16>,
    ) -> Result<(), Fault> {
        match self.phase {
            Phase::Open => {}
            Phase::NeedBind => return Err(self.fail(FaultCode::BindRequired)),
            Phase::NeedBegin => return Err(self.fail(FaultCode::UnknownTxn)),
            Phase::Sealed | Phase::RolledBack | Phase::Failed => {
                return Err(self.fail(FaultCode::StreamClosed));
            }
        }
        if self.txn_id.as_deref() != Some(txn_id.as_str()) {
            return Err(self.fail(FaultCode::UnknownTxn));
        }
        validate_txn_id(&txn_id).map_err(|code| self.fail(code))?;
        if global == 0 || collection_seq == 0 {
            return Err(self.fail(FaultCode::SequenceInvalid));
        }
        if global > self.limits.max_events {
            return Err(self.fail(FaultCode::EventCap));
        }
        if collection_seq > self.limits.max_events {
            return Err(self.fail(FaultCode::SequenceOutOfRange));
        }
        validate_table(&table).map_err(|code| self.fail(code))?;
        validate_key(&key).map_err(|code| self.fail(code))?;
        validate_payload(&payload, &unavailable).map_err(|code| self.fail(code))?;
        let preimage = canonical_preimage(&CanonicalData {
            txn_id: &txn_id,
            global_txn_sequence: global,
            data_collection_sequence: collection_seq,
            table: &table,
            key: &key,
            op,
            payload: &payload,
            unavailable_ordinals: &unavailable,
        })
        .map_err(|code| self.fail(code))?;
        let digest = event_hash(&preimage).map_err(|code| self.fail(code))?;
        let fresh_global = {
            let max_events = self.limits.max_events;
            self.global.insert(global, max_events)
        }
        .map_err(|code| self.fail(code))?;
        if !fresh_global {
            let previous = self.hash_at(global);
            if previous == digest {
                self.duplicate_data = self
                    .duplicate_data
                    .checked_add(1)
                    .ok_or_else(|| self.fail(FaultCode::CountOverflow))?;
                return Ok(());
            }
            return Err(self.fail(FaultCode::DuplicateConflict));
        }
        if !self.collections.contains_key(&table)
            && self.collections.len() >= self.limits.max_collections
        {
            return Err(self.fail(FaultCode::CollectionInvalid));
        }
        let fresh_collection = {
            let max_events = self.limits.max_events;
            let collection = self.collections.entry(table.clone()).or_insert_with(Bits::new);
            collection.insert(collection_seq, max_events)
        }
        .map_err(|code| self.fail(code))?;
        if !fresh_collection {
            return Err(self.fail(FaultCode::DuplicateConflict));
        }
        if self.admitted >= self.limits.max_events {
            return Err(self.fail(FaultCode::EventCap));
        }
        self.remember_hash(global, digest)?;
        self.push_event(&preimage)?;
        self.admitted += 1;
        self.decoded_events += 1;
        if self.selected.contains(&table) {
            self.selected_rows += 1;
        }
        self.seen_data += 1;
        Ok(())
    }

    #[allow(clippy::cognitive_complexity)]
    fn apply_end(
        &mut self,
        txn_id: String,
        count: u64,
        per_collection: BTreeMap<String, u64>,
        commit_lsn: String,
        timestamp: String,
    ) -> Result<(), Fault> {
        validate_txn_id(&txn_id).map_err(|code| self.fail(code))?;
        if self.phase == Phase::Sealed {
            let same = self.end.as_ref().is_some_and(|end| {
                end.count == count
                    && end.per_collection == per_collection
                    && end.commit_lsn == commit_lsn
                    && end.timestamp == timestamp
                    && self.txn_id.as_deref() == Some(txn_id.as_str())
            });
            if same {
                self.duplicate_end += 1;
                return Ok(());
            }
            return Err(self.fail(FaultCode::EndConflict));
        }
        if self.phase == Phase::RolledBack {
            return Err(self.fail(FaultCode::TxnIdReuse));
        }
        if self.phase != Phase::Open || self.txn_id.as_deref() != Some(txn_id.as_str()) {
            return Err(self.fail(FaultCode::UnknownTxn));
        }
        self.hit(Checkpoint::BeforeEnd)?;
        if let Err(code) = self.check_end(count, &per_collection) {
            if code == FaultCode::EndTooEarly {
                return Err(Fault::new(code, Some(txn_id)));
            }
            return Err(self.fail(code));
        }
        validate_timestamp(&timestamp).map_err(|code| self.fail(code))?;
        let lsn = parse_commit_lsn(&commit_lsn).map_err(|code| self.fail(code))?;
        let epoch = match self.bind.as_ref() {
            Some(bind) => bind.epoch,
            None => return Err(self.fail(FaultCode::BindRequired)),
        };
        let position = encode_position(epoch, lsn).map_err(|code| self.fail(code))?;
        self.seal_chunk()?;
        if !self.index_created {
            self.append_index("")?;
        }
        let manifest = self.manifest(
            &txn_id,
            count,
            &per_collection,
            &commit_lsn,
            &timestamp,
            &position,
        )?;
        let body = serde_json::to_vec(&manifest).map_err(|_| self.fail(FaultCode::Io))?;
        let manifest_len = u64::try_from(body.len()).unwrap_or(u64::MAX);
        self.charge(manifest_len.saturating_add(65))?;
        let partial = self.dir.join("manifest.json.partial");
        let final_path = self.dir.join("manifest.json");
        {
            let mut file = self.create_exclusive(&partial)?;
            file.write_all(&body).map_err(|_| self.fail(FaultCode::Io))?;
            file.flush().map_err(|_| self.fail(FaultCode::Io))?;
            file.sync_all().map_err(|_| self.fail(FaultCode::Io))?;
        }
        fs::rename(&partial, &final_path).map_err(|_| self.fail(FaultCode::Io))?;
        sync_dir(&self.dir).map_err(|code| self.fail(code))?;
        self.hit(Checkpoint::BeforeFinalSeal)?;
        let digest = sha256(&body).ok_or_else(|| self.fail(FaultCode::Io))?;
        let seal = format!("{}\n", hex(&digest));
        {
            let mut file = self.create_exclusive(&self.dir.join("SEAL"))?;
            file.write_all(seal.as_bytes()).map_err(|_| self.fail(FaultCode::Io))?;
            file.flush().map_err(|_| self.fail(FaultCode::Io))?;
            file.sync_all().map_err(|_| self.fail(FaultCode::Io))?;
        }
        sync_dir(&self.dir).map_err(|code| self.fail(code))?;
        self.end = Some(EndIdentity {
            count,
            per_collection,
            commit_lsn,
            timestamp,
        });
        self.phase = Phase::Sealed;
        self.seen_end += 1;
        self.write_progress("sealed")?;
        Ok(())
    }

    fn apply_rollback(&mut self, txn_id: String) -> Result<(), Fault> {
        validate_txn_id(&txn_id).map_err(|code| self.fail(code))?;
        if self.phase == Phase::Sealed {
            return Err(self.fail(FaultCode::RollbackAfterComplete));
        }
        if self.phase == Phase::RolledBack && self.txn_id.as_deref() == Some(txn_id.as_str()) {
            self.duplicate_rollback += 1;
            return Ok(());
        }
        if self.phase != Phase::Open || self.txn_id.as_deref() != Some(txn_id.as_str()) {
            return Err(self.fail(FaultCode::UnknownTxn));
        }
        self.buffer.clear();
        self.buffer_events = 0;
        let body = b"rolled_back\n";
        self.charge(u64::try_from(body.len()).unwrap_or(u64::MAX))?;
        {
            let mut file = self.create_exclusive(&self.dir.join("ROLLED_BACK"))?;
            file.write_all(body).map_err(|_| self.fail(FaultCode::Io))?;
            file.flush().map_err(|_| self.fail(FaultCode::Io))?;
            file.sync_all().map_err(|_| self.fail(FaultCode::Io))?;
        }
        sync_dir(&self.dir).map_err(|code| self.fail(code))?;
        self.phase = Phase::RolledBack;
        self.seen_rollback += 1;
        self.write_progress("rolled_back")?;
        Ok(())
    }

    fn finish(&mut self) -> Result<Outcome, Fault> {
        let progress = self.progress();
        let kind = match self.phase {
            Phase::Sealed => {
                let txn_for_fault = self.txn_id.clone();
                let (txn_id, count, commit_lsn) = {
                    let end = self
                        .end
                        .as_ref()
                        .ok_or_else(|| Fault::new(FaultCode::Io, txn_for_fault))?;
                    (
                        self.txn_id.clone().unwrap_or_default(),
                        end.count,
                        end.commit_lsn.clone(),
                    )
                };
                OutcomeKind::Sealed(SealSummary {
                    txn_id,
                    count,
                    commit_lsn,
                    commit_position: self.sealed_position()?,
                    durable_events: progress.durable_events,
                    decoded_events: self.decoded_events,
                    ack_allowed: false,
                    target_applied: false,
                    cross_table_consistent: false,
                    resident_hwm_bytes: resident_hwm_bytes(),
                })
            }
            Phase::RolledBack => OutcomeKind::RolledBack {
                txn_id: self.txn_id.clone().unwrap_or_default(),
            },
            Phase::NeedBind => return Err(self.fail(FaultCode::BindRequired)),
            Phase::Failed => return Err(self.fail(FaultCode::StreamClosed)),
            Phase::NeedBegin | Phase::Open => OutcomeKind::Incomplete {
                txn_id: self.txn_id.clone(),
            },
        };
        Ok(Outcome { kind, progress })
    }

    fn sealed_position(&self) -> Result<String, Fault> {
        let txn_id = self.txn_id.clone();
        let end = self
            .end
            .as_ref()
            .ok_or_else(|| Fault::new(FaultCode::Io, txn_id.clone()))?;
        let epoch = self
            .bind
            .as_ref()
            .ok_or_else(|| Fault::new(FaultCode::BindRequired, txn_id.clone()))?
            .epoch;
        let lsn = parse_commit_lsn(&end.commit_lsn)
            .map_err(|code| Fault::new(code, txn_id.clone()))?;
        encode_position(epoch, lsn).map_err(|code| Fault::new(code, txn_id))
    }

    fn check_end(&self, count: u64, declared: &BTreeMap<String, u64>) -> Result<(), FaultCode> {
        let mut sum = 0u64;
        for value in declared.values() {
            sum = sum.checked_add(*value).ok_or(FaultCode::CountOverflow)?;
        }
        if sum != count {
            return Err(FaultCode::CollectionCountGap);
        }
        for name in declared.keys() {
            validate_table(name)?;
        }
        if self.collections.len() != declared.len()
            || self.collections.keys().any(|name| !declared.contains_key(name))
        {
            return Err(FaultCode::CollectionUnknown);
        }
        if self.admitted < count {
            return Err(FaultCode::EndTooEarly);
        }
        if self.admitted > count {
            return Err(FaultCode::CountMismatch);
        }
        self.check_bits(&self.global, count)?;
        for (name, bits) in &self.collections {
            let declared_count = declared[name];
            if bits.count < declared_count {
                return Err(FaultCode::EndTooEarly);
            }
            if bits.count > declared_count {
                return Err(FaultCode::CollectionCountGap);
            }
            self.check_bits(bits, declared_count)?;
        }
        Ok(())
    }

    fn check_bits(&self, bits: &Bits, count: u64) -> Result<(), FaultCode> {
        if count == 0 {
            return if bits.count == 0 {
                Ok(())
            } else {
                Err(FaultCode::CollectionCountGap)
            };
        }
        if bits.max > count || bits.min == 0 {
            return Err(FaultCode::SequenceOutOfRange);
        }
        if bits.count != count {
            Err(FaultCode::SequenceGap)
        } else {
            Ok(())
        }
    }

    fn manifest(
        &mut self,
        txn_id: &str,
        count: u64,
        per_collection: &BTreeMap<String, u64>,
        commit_lsn: &str,
        timestamp: &str,
        position: &str,
    ) -> Result<Manifest, Fault> {
        let Some(bind) = self.bind.clone() else {
            return Err(self.fail(FaultCode::BindRequired));
        };
        let set_hash = sha256(self.chunk_set_body.as_bytes())
            .ok_or_else(|| Fault::new(FaultCode::Io, self.txn_id.clone()))?;
        Ok(Manifest {
            contract: CONTRACT.to_owned(),
            kind: "committed_manifest".to_owned(),
            source: bind.source.clone(),
            lineage: bind.lineage.clone(),
            epoch: bind.epoch,
            schema_hash: bind.schema_hash.clone(),
            config_hash: bind.config_hash.clone(),
            type_hash: bind.type_hash.clone(),
            hook_hashes: bind.hook_hashes.clone(),
            selected_tables: bind.selected_tables.clone(),
            commit_position_provenance: PROVENANCE.to_owned(),
            txn_id: txn_id.to_owned(),
            count,
            per_collection_counts: per_collection.clone(),
            first_sequence: if count == 0 { None } else { Some(1) },
            last_sequence: if count == 0 { None } else { Some(count) },
            commit_lsn: commit_lsn.to_owned(),
            commit_position: position.to_owned(),
            source_commit_timestamp: timestamp.to_owned(),
            selected_row_count: self.selected_rows,
            chunks: self.chunks.clone(),
            chunk_set_sha256: hex(&set_hash),
            limits: ManifestLimits::from_limits(&self.limits),
            ack_allowed: false,
            target_applied: false,
            cross_table_consistent: false,
            applied: serde_json::Value::Null,
            upstream_commit_lsn_verified: false,
            debezium_normalizer: false,
            p2_verified: false,
            p11_verified: false,
        })
    }

    fn push_event(&mut self, preimage: &str) -> Result<(), Fault> {
        let line_len = u64::try_from(preimage.len()).unwrap_or(u64::MAX).saturating_add(1);
        let buffered = u64::try_from(self.buffer.len()).unwrap_or(u64::MAX);
        if !self.buffer.is_empty()
            && (self.buffer_events + 1 > self.limits.max_chunk_events
                || buffered.saturating_add(line_len) > self.limits.max_chunk_bytes)
        {
            self.seal_chunk()?;
        }
        if self.buffer.is_empty() && line_len > self.limits.max_chunk_bytes {
            return Err(self.fail(FaultCode::ChunkBound));
        }
        let mut extra = line_len;
        if self.buffer.is_empty() {
            extra = extra.saturating_add(INDEX_ALLOWANCE);
            if !self.progress_reserved {
                extra = extra.saturating_add(PROGRESS_ALLOWANCE);
            }
            let needed = 1
                + u64::from(!self.index_created)
                + u64::from(!self.progress_created);
            if self.files.saturating_add(needed) > self.limits.max_files {
                return Err(self.fail(FaultCode::FileCap));
            }
        }
        self.charge(extra)?;
        if self.buffer.is_empty() && !self.progress_reserved {
            self.progress_reserved = true;
            self.progress_accounted = PROGRESS_ALLOWANCE;
        }
        self.buffer.extend_from_slice(preimage.as_bytes());
        self.buffer.push(b'\n');
        self.buffer_events += 1;
        if self.buffer_events >= self.limits.max_chunk_events
            || u64::try_from(self.buffer.len()).unwrap_or(u64::MAX) >= self.limits.max_chunk_bytes
        {
            self.seal_chunk()?;
        }
        Ok(())
    }

    fn seal_chunk(&mut self) -> Result<(), Fault> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let next = u64::try_from(self.chunks.len()).unwrap_or(u64::MAX).saturating_add(1);
        let name = chunk_name(next).map_err(|code| self.fail(code))?;
        let digest = match sha256(&self.buffer) {
            Some(digest) => digest,
            None => return Err(self.fail(FaultCode::Io)),
        };
        let digest_hex = hex(&digest);
        let partial = self.dir.join(format!("{name}.partial"));
        let final_path = self.dir.join(&name);
        {
            let mut file = self.create_exclusive(&partial)?;
            file.write_all(&self.buffer).map_err(|_| self.fail(FaultCode::Io))?;
            file.flush().map_err(|_| self.fail(FaultCode::Io))?;
            self.hit(Checkpoint::BeforeChunkSync)?;
            file.sync_all().map_err(|_| self.fail(FaultCode::Io))?;
        }
        confirm_file(&partial, self.buffer.len() as u64, &digest_hex).map_err(|code| self.fail(code))?;
        fs::rename(&partial, &final_path).map_err(|_| self.fail(FaultCode::Io))?;
        sync_dir(&self.dir).map_err(|code| self.fail(code))?;
        confirm_file(&final_path, self.buffer.len() as u64, &digest_hex).map_err(|code| self.fail(code))?;
        let events = self.buffer_events;
        let bytes = u64::try_from(self.buffer.len()).unwrap_or(u64::MAX);
        let line = format!("{name} {bytes} {events} {digest_hex}\n");
        self.append_index(&line)?;
        self.chunk_set_body.push_str(&digest_hex);
        self.chunk_set_body.push('\n');
        self.chunks.push(ChunkRec {
            name,
            events,
            bytes,
            sha256: digest_hex,
        });
        self.synced_payload = self.synced_payload.saturating_add(bytes);
        self.buffer.clear();
        self.buffer_events = 0;
        self.write_progress("open")?;
        Ok(())
    }

    fn append_index(&mut self, line: &str) -> Result<(), Fault> {
        let path = self.dir.join("chunks.idx");
        let mut file = if self.index_created {
            OpenOptions::new()
                .append(true)
                .open(&path)
                .map_err(|_| self.fail(FaultCode::Io))?
        } else {
            let created = self.create_exclusive(&path)?;
            self.index_created = true;
            created
        };
        file.write_all(line.as_bytes()).map_err(|_| self.fail(FaultCode::Io))?;
        file.flush().map_err(|_| self.fail(FaultCode::Io))?;
        file.sync_all().map_err(|_| self.fail(FaultCode::Io))?;
        sync_dir(&self.dir).map_err(|code| self.fail(code))?;
        Ok(())
    }

    fn write_progress(&mut self, state: &str) -> Result<(), Fault> {
        let view = self.progress();
        let body = serde_json::to_vec(&ProgressFile {
            contract: CONTRACT,
            kind: "durable_progress",
            txn_id: self.txn_id.as_deref(),
            state,
            durable_chunks: view.durable_chunks,
            durable_events: view.durable_events,
            durable_bytes: view.durable_bytes,
            decoded_events: view.decoded_events,
            read_lines: view.read_lines,
            read_bytes: view.read_bytes,
            applied: None,
            ack_allowed: false,
            target_applied: false,
            cross_table_consistent: false,
        })
        .map_err(|_| self.fail(FaultCode::Io))?;
        let len = u64::try_from(body.len()).unwrap_or(u64::MAX);
        if !self.progress_reserved {
            self.charge(len)?;
            self.progress_reserved = true;
            self.progress_accounted = len;
        } else if len > self.progress_accounted {
            self.charge(len - self.progress_accounted)?;
            self.progress_accounted = len;
        }
        let path = self.dir.join("progress.json");
        if self.progress_created {
            let mut file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&path)
                .map_err(|_| self.fail(FaultCode::Io))?;
            file.write_all(&body).map_err(|_| self.fail(FaultCode::Io))?;
            file.flush().map_err(|_| self.fail(FaultCode::Io))?;
            file.sync_all().map_err(|_| self.fail(FaultCode::Io))?;
        } else {
            let mut file = self.create_exclusive(&path)?;
            file.write_all(&body).map_err(|_| self.fail(FaultCode::Io))?;
            file.flush().map_err(|_| self.fail(FaultCode::Io))?;
            file.sync_all().map_err(|_| self.fail(FaultCode::Io))?;
            self.progress_created = true;
        }
        sync_dir(&self.dir).map_err(|code| self.fail(code))?;
        Ok(())
    }

    fn remember_hash(&mut self, seq: u64, digest: [u8; 32]) -> Result<(), Fault> {
        let idx = usize::try_from(seq).map_err(|_| self.fail(FaultCode::SequenceOutOfRange))?;
        let limit = usize::try_from(self.limits.max_events)
            .map_err(|_| self.fail(FaultCode::SequenceOutOfRange))?
            .saturating_add(1);
        if idx >= limit {
            return Err(self.fail(FaultCode::SequenceOutOfRange));
        }
        if self.hashes.len() <= idx {
            let mut new_len = self.hashes.len().max(1);
            while new_len <= idx {
                let grown = new_len.saturating_mul(2).min(limit);
                new_len = if grown <= idx { idx + 1 } else { grown };
            }
            self.hashes.resize(new_len, [0u8; 32]);
        }
        self.hashes[idx] = digest;
        Ok(())
    }

    fn hash_at(&self, seq: u64) -> [u8; 32] {
        let idx = usize::try_from(seq).unwrap_or(0);
        self.hashes.get(idx).copied().unwrap_or([0u8; 32])
    }

    fn charge(&mut self, bytes: u64) -> Result<(), Fault> {
        let Some(next) = self.disk.checked_add(bytes) else {
            return Err(self.fail(FaultCode::CountOverflow));
        };
        if next > self.limits.max_disk_bytes {
            return Err(self.fail(FaultCode::DiskQuota));
        }
        self.disk = next;
        Ok(())
    }

    fn create_exclusive(&mut self, path: &Path) -> Result<File, Fault> {
        if self.files >= self.limits.max_files {
            return Err(self.fail(FaultCode::FileCap));
        }
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => {
                self.files += 1;
                Ok(file)
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(self.fail(FaultCode::PathExists))
            }
            Err(_) => Err(self.fail(FaultCode::Io)),
        }
    }

    fn hit(&mut self, checkpoint: Checkpoint) -> Result<(), Fault> {
        let Some(mut hook) = self.checkpoint.take() else {
            return Ok(());
        };
        let result = hook(checkpoint);
        self.checkpoint = Some(hook);
        result
    }

    fn bump_duplicate_begin(&mut self) -> Result<(), Fault> {
        self.duplicate_begin = self
            .duplicate_begin
            .checked_add(1)
            .ok_or_else(|| self.fail(FaultCode::CountOverflow))?;
        Ok(())
    }

    fn fail(&mut self, code: FaultCode) -> Fault {
        if code != FaultCode::EndTooEarly {
            self.phase = Phase::Failed;
        }
        Fault::new(code, self.txn_id.clone())
    }
}

#[derive(Serialize)]
struct ProgressFile<'a> {
    contract: &'a str,
    kind: &'a str,
    txn_id: Option<&'a str>,
    state: &'a str,
    durable_chunks: u64,
    durable_events: u64,
    durable_bytes: u64,
    decoded_events: u64,
    read_lines: u64,
    read_bytes: u64,
    applied: Option<()>,
    ack_allowed: bool,
    target_applied: bool,
    cross_table_consistent: bool,
}

fn confirm_file(path: &Path, expected_len: u64, expected_hex: &str) -> Result<(), FaultCode> {
    let meta = fs::symlink_metadata(path).map_err(|_| FaultCode::Io)?;
    if meta.file_type().is_symlink() {
        return Err(FaultCode::SymlinkRefused);
    }
    if !meta.is_file() {
        return Err(FaultCode::MissingChunk);
    }
    if meta.len() != expected_len {
        return Err(FaultCode::Truncated);
    }
    let bytes = fs::read(path).map_err(|_| FaultCode::Io)?;
    if bytes.len() as u64 != expected_len {
        return Err(FaultCode::Truncated);
    }
    let digest = sha256(&bytes).ok_or(FaultCode::Io)?;
    if hex(&digest) == expected_hex {
        Ok(())
    } else {
        Err(FaultCode::ChecksumFault)
    }
}

fn sync_dir(path: &Path) -> Result<(), FaultCode> {
    let file = File::open(path).map_err(|_| FaultCode::Io)?;
    file.sync_all().map_err(|_| FaultCode::Io)
}

fn resident_hwm_bytes() -> Option<u64> {
    let text = fs::read_to_string("/proc/self/status").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            return kb.checked_mul(1024);
        }
    }
    None
}
