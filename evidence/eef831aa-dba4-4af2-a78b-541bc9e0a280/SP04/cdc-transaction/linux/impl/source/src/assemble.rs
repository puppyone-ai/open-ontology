//! Open-transaction journal. Complete output follows end order.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::jsonline::parse_wire;
use crate::lsn::{encode_position, parse_commit_lsn, position_value};
use crate::model::{
    canonical_preimage, event_hash, raw_value_bytes, validate_bind, validate_key, validate_payload,
    validate_table, validate_timestamp, validate_txn_id, Attestations, Bind, CanonicalData, Cell,
    CompletedTxn, DecodedProgress, EmittedRow, Fault, FaultCode, Op, PendingStatus, PendingTxn,
    Progress, ReadProgress, SeenProgress, Wire, CONTRACT, MAX_ACTIVE, MAX_LINE_BYTES,
    MAX_PAYLOAD_BYTES, MAX_TXN_EVENTS, PROVENANCE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Accepted,
    Idempotent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOut {
    pub effect: Effect,
    pub completed: Option<CompletedTxn>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenStatus {
    Open,
    Refused,
}

struct StoredEvent {
    global: u64,
    collection_seq: u64,
    table: String,
    key: Vec<Cell>,
    op: Op,
    payload: Vec<Cell>,
    unavailable: Vec<u16>,
    hash: String,
    charge: u64,
}

struct OpenTxn {
    txn_id: String,
    events: Vec<StoredEvent>,
    payload: u64,
    status: OpenStatus,
    by_global: HashMap<u64, usize>,
    by_collection: HashMap<(String, u64), usize>,
}

struct EndIdentity {
    count: u64,
    per_collection: BTreeMap<String, u64>,
    commit_lsn: String,
    timestamp: String,
}

struct Incoming {
    txn_id: String,
    global: u64,
    collection_seq: u64,
    table: String,
    key: Vec<Cell>,
    op: Op,
    payload: Vec<Cell>,
    unavailable: Vec<u16>,
}

enum Staged {
    Idempotent,
    Fresh { index: usize, charge: u64, hash: String },
}

struct EndDraft {
    index: usize,
    txn_id: String,
    count: u64,
    per_collection: BTreeMap<String, u64>,
    commit_lsn: String,
    timestamp: String,
    position: u128,
    position_text: String,
}

struct EmitAt<'a> {
    index: usize,
    commit_order: u64,
    commit_lsn: &'a str,
    position: &'a str,
    timestamp: &'a str,
    count: u64,
    per_collection: &'a BTreeMap<String, u64>,
}

struct FinishedEnd {
    index: usize,
    txn_id: String,
    count: u64,
    per_collection: BTreeMap<String, u64>,
    commit_lsn: String,
    timestamp: String,
    position: u128,
    position_text: String,
    journal_events: u64,
    journal_payload: u64,
    next_order: u64,
    selected_rows: u64,
    completed: CompletedTxn,
}

#[derive(Default)]
struct Seen {
    bind: u64,
    barrier: u64,
    begin: u64,
    data: u64,
    end: u64,
    rollback: u64,
    duplicate_data: u64,
    duplicate_begin: u64,
    duplicate_end: u64,
    duplicate_rollback: u64,
}

#[derive(Default)]
pub struct Assembler {
    bind: Option<Bind>,
    selected: HashSet<String>,
    epoch: u32,
    lineage: String,
    floor: Option<u128>,
    positions: HashSet<u128>,
    commit_order: u64,
    open: Vec<OpenTxn>,
    done: HashMap<String, EndIdentity>,
    rolled_back: HashSet<String>,
    journal_events: u64,
    journal_payload: u64,
    seen: Seen,
    read_lines: u64,
    read_bytes: u64,
    selected_rows: u64,
    last_position: Option<String>,
    last_lsn: Option<String>,
    last_epoch: Option<u32>,
    last_lineage: Option<String>,
    resume_required: bool,
    needs_spool: bool,
    fault: Option<Fault>,
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn note_newline(&mut self) -> Result<(), Fault> {
        self.add_bytes(1)
    }

    pub fn note_line_too_long(&mut self, observed: u64) -> Fault {
        self.note_refusal(observed, FaultCode::LineTooLong)
    }

    pub fn note_malformed(&mut self, observed: u64) -> Fault {
        self.note_refusal(observed, FaultCode::Malformed)
    }

    pub fn apply_line(&mut self, line: &str) -> Result<ApplyOut, Fault> {
        if let Err(fault) = self.bump_lines() {
            return Err(fault);
        }
        let bytes = u64::try_from(line.len()).unwrap_or(u64::MAX);
        if let Err(fault) = self.add_bytes(bytes) {
            return Err(fault);
        }
        if let Some(fault) = self.fault.clone() {
            return Err(fault);
        }
        if line.len() > MAX_LINE_BYTES {
            return Err(self.fail(FaultCode::LineTooLong, None));
        }
        let wire = match parse_wire(line) {
            Ok(wire) => wire,
            Err(code) => return Err(self.fail(code, None)),
        };
        self.dispatch(wire)
    }

    pub fn finish(&mut self) -> Progress {
        if self.fault.is_none() && self.bind.is_none() {
            let _ = self.fail(FaultCode::BindRequired, None);
        } else if self.fault.is_none() && !self.open.is_empty() {
            self.resume_required = true;
        }
        self.progress()
    }

    pub fn progress(&self) -> Progress {
        Progress {
            contract: CONTRACT,
            kind: "progress",
            source: self.bind.as_ref().map(|bind| bind.source.clone()),
            read: ReadProgress {
                lines: self.read_lines,
                bytes: self.read_bytes,
            },
            seen: self.seen_progress(),
            complete_decoded: self.decoded_progress(),
            applied: None,
            resume_required: self.resume_required,
            needs_spool: self.needs_spool,
            ack_allowed: false,
            target_applied: false,
            cross_table_consistent: false,
            upstream_commit_lsn_verified: false,
            debezium_normalizer: false,
            p2_verified: false,
            p11_verified: false,
            commit_position_provenance: self.bind.as_ref().map(|_| PROVENANCE),
            pending: self.pending(),
            fault: self.fault.as_ref().map(|fault| fault.code),
        }
    }

    fn note_refusal(&mut self, observed: u64, code: FaultCode) -> Fault {
        let _ = self.add_bytes(observed);
        if let Some(fault) = self.fault.clone() {
            return fault;
        }
        let _ = self.bump_lines();
        if let Some(fault) = self.fault.clone() {
            return fault;
        }
        self.fail(code, None)
    }

    fn pending(&self) -> Vec<PendingTxn> {
        self.open
            .iter()
            .map(|txn| PendingTxn {
                txn_id: txn.txn_id.clone(),
                data_events: u64::try_from(txn.events.len()).unwrap_or(u64::MAX),
                payload_bytes: txn.payload,
                status: match txn.status {
                    OpenStatus::Open => PendingStatus::Open,
                    OpenStatus::Refused => PendingStatus::Refused,
                },
            })
            .collect()
    }

    fn seen_progress(&self) -> SeenProgress {
        SeenProgress {
            bind: self.seen.bind,
            barrier: self.seen.barrier,
            begin: self.seen.begin,
            data: self.seen.data,
            end: self.seen.end,
            rollback: self.seen.rollback,
            duplicate_data: self.seen.duplicate_data,
            duplicate_begin: self.seen.duplicate_begin,
            duplicate_end: self.seen.duplicate_end,
            duplicate_rollback: self.seen.duplicate_rollback,
        }
    }

    fn decoded_progress(&self) -> DecodedProgress {
        DecodedProgress {
            transactions: self.seen.end,
            selected_rows: self.selected_rows,
            last_position: self.last_position.clone(),
            last_commit_lsn: self.last_lsn.clone(),
            last_epoch: self.last_epoch,
            last_lineage: self.last_lineage.clone(),
        }
    }

    fn dispatch(&mut self, wire: Wire) -> Result<ApplyOut, Fault> {
        if self.bind.is_none() {
            return self.dispatch_unbound(wire);
        }
        match wire {
            Wire::Bind { .. } => Err(self.fail(FaultCode::BindDuplicate, None)),
            Wire::Barrier {
                reason,
                lineage,
                epoch,
            } => self.apply_barrier(reason, lineage, epoch),
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
            } => self.apply_data(Incoming {
                txn_id,
                global: global_txn_sequence,
                collection_seq: data_collection_sequence,
                table,
                key,
                op,
                payload,
                unavailable: unavailable_ordinals,
            }),
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

    fn dispatch_unbound(&mut self, wire: Wire) -> Result<ApplyOut, Fault> {
        let Wire::Bind {
            source,
            lineage,
            epoch,
            selected_tables,
            schema_hash,
            config_hash,
            type_hash,
            hook_hashes,
            commit_position_provenance,
        } = wire
        else {
            return Err(self.fail(FaultCode::BindRequired, None));
        };
        self.bind_feed(
            Bind {
                source,
                lineage,
                epoch,
                selected_tables,
                schema_hash,
                config_hash,
                type_hash,
                hook_hashes,
            },
            commit_position_provenance,
        )
    }

    fn bind_feed(&mut self, bind: Bind, provenance: String) -> Result<ApplyOut, Fault> {
        if let Err(code) = validate_bind(&bind, &provenance) {
            return Err(self.fail(code, None));
        }
        self.selected = bind.selected_tables.iter().cloned().collect();
        self.epoch = bind.epoch;
        self.lineage.clone_from(&bind.lineage);
        self.bind = Some(bind);
        self.bump(SeenField::Bind)?;
        Ok(accepted())
    }

    fn apply_barrier(
        &mut self,
        reason: crate::model::BarrierReason,
        lineage: Option<String>,
        epoch: u32,
    ) -> Result<ApplyOut, Fault> {
        if !self.open.is_empty() {
            return Err(self.fail(FaultCode::OpenTxnAtBarrier, None));
        }
        match reason {
            crate::model::BarrierReason::Epoch => self.barrier_epoch(lineage, epoch)?,
            crate::model::BarrierReason::Reset => self.barrier_reset(lineage, epoch)?,
        }
        self.bump(SeenField::Barrier)?;
        Ok(accepted())
    }

    fn barrier_epoch(&mut self, lineage: Option<String>, epoch: u32) -> Result<(), Fault> {
        if lineage.is_some() {
            return Err(self.fail(FaultCode::LineageNotAllowed, None));
        }
        if epoch == self.epoch {
            return Err(self.fail(FaultCode::EpochUnchanged, None));
        }
        self.epoch = epoch;
        self.floor = None;
        Ok(())
    }

    fn barrier_reset(&mut self, lineage: Option<String>, epoch: u32) -> Result<(), Fault> {
        let Some(lineage) = lineage else {
            return Err(self.fail(FaultCode::LineageRequired, None));
        };
        if !is_hex64(&lineage) {
            return Err(self.fail(FaultCode::BarrierInvalid, None));
        }
        if lineage == self.lineage {
            return Err(self.fail(FaultCode::LineageUnchanged, None));
        }
        self.lineage = lineage;
        self.epoch = epoch;
        self.floor = None;
        Ok(())
    }

    fn apply_begin(&mut self, txn_id: String) -> Result<ApplyOut, Fault> {
        if let Err(code) = validate_txn_id(&txn_id) {
            return Err(self.fail(code, None));
        }
        if self.done.contains_key(&txn_id) || self.rolled_back.contains(&txn_id) {
            return Err(self.fail(FaultCode::TxnIdReuse, Some(txn_id)));
        }
        if self.open.iter().any(|txn| txn.txn_id == txn_id) {
            self.bump(SeenField::DuplicateBegin)?;
            return Ok(idempotent());
        }
        if self.open.len() >= MAX_ACTIVE {
            return Err(self.fail(FaultCode::OpenSetExceeded, Some(txn_id)));
        }
        self.open.push(OpenTxn {
            txn_id,
            events: Vec::new(),
            payload: 0,
            status: OpenStatus::Open,
            by_global: HashMap::new(),
            by_collection: HashMap::new(),
        });
        self.bump(SeenField::Begin)?;
        Ok(accepted())
    }

    fn apply_data(&mut self, data: Incoming) -> Result<ApplyOut, Fault> {
        match self.stage_data(&data)? {
            Staged::Idempotent => {
                self.bump(SeenField::DuplicateData)?;
                Ok(idempotent())
            }
            Staged::Fresh { index, charge, hash } => self.store_data(data, index, charge, hash),
        }
    }

    fn stage_data(&mut self, data: &Incoming) -> Result<Staged, Fault> {
        self.reject_bad_data(data)?;
        if self.done.contains_key(&data.txn_id) || self.rolled_back.contains(&data.txn_id) {
            return Err(self.fail(FaultCode::TxnIdReuse, Some(data.txn_id.clone())));
        }
        let Some(index) = self.open.iter().position(|txn| txn.txn_id == data.txn_id) else {
            return Err(self.fail(FaultCode::UnknownTxn, Some(data.txn_id.clone())));
        };
        if let Some(existing) = self.open[index].by_global.get(&data.global).copied() {
            return self.replay_data(index, existing, data);
        }
        if self.open[index]
            .by_collection
            .contains_key(&(data.table.clone(), data.collection_seq))
        {
            return Err(self.fail(FaultCode::DuplicateConflict, Some(data.txn_id.clone())));
        }
        let (charge, hash) = self.charge_data(data)?;
        self.admit_charge(index, charge, &data.txn_id)?;
        Ok(Staged::Fresh { index, charge, hash })
    }

    fn reject_bad_data(&mut self, data: &Incoming) -> Result<(), Fault> {
        if let Err(code) = validate_txn_id(&data.txn_id) {
            return Err(self.fail(code, None));
        }
        if data.global == 0 || data.collection_seq == 0 {
            return Err(self.fail(FaultCode::SequenceInvalid, Some(data.txn_id.clone())));
        }
        if let Err(code) = validate_table(&data.table) {
            return Err(self.fail(code, Some(data.txn_id.clone())));
        }
        if let Err(code) = validate_key(&data.key) {
            return Err(self.fail(code, Some(data.txn_id.clone())));
        }
        if let Err(code) = validate_payload(&data.payload, &data.unavailable) {
            return Err(self.fail(code, Some(data.txn_id.clone())));
        }
        Ok(())
    }

    fn replay_data(&mut self, index: usize, existing: usize, data: &Incoming) -> Result<Staged, Fault> {
        let same = {
            let stored = &self.open[index].events[existing];
            stored.global == data.global
                && stored.collection_seq == data.collection_seq
                && stored.table == data.table
                && stored.key == data.key
                && stored.op == data.op
                && stored.payload == data.payload
                && stored.unavailable == data.unavailable
        };
        if same {
            Ok(Staged::Idempotent)
        } else {
            Err(self.fail(FaultCode::DuplicateConflict, Some(data.txn_id.clone())))
        }
    }

    fn charge_data(&mut self, data: &Incoming) -> Result<(u64, String), Fault> {
        let raw = match raw_value_bytes(&data.key, &data.payload) {
            Ok(raw) => raw,
            Err(code) => return Err(self.fail(code, Some(data.txn_id.clone()))),
        };
        if raw > MAX_PAYLOAD_BYTES {
            return Err(self.fail(FaultCode::NeedsSpool, Some(data.txn_id.clone())));
        }
        let preimage = match canonical_preimage(&CanonicalData {
            txn_id: &data.txn_id,
            global_txn_sequence: data.global,
            data_collection_sequence: data.collection_seq,
            table: &data.table,
            key: &data.key,
            op: data.op,
            payload: &data.payload,
            unavailable_ordinals: &data.unavailable,
        }) {
            Ok(preimage) => preimage,
            Err(code) => return Err(self.fail(code, Some(data.txn_id.clone()))),
        };
        let charge = u64::try_from(preimage.len()).unwrap_or(u64::MAX);
        let hash = match event_hash(&preimage) {
            Ok(hash) => hash,
            Err(code) => return Err(self.fail(code, Some(data.txn_id.clone()))),
        };
        Ok((charge, hash))
    }

    fn admit_charge(&mut self, index: usize, charge: u64, txn_id: &str) -> Result<(), Fault> {
        let txn_events = u64::try_from(self.open[index].events.len()).unwrap_or(u64::MAX);
        if txn_events >= MAX_TXN_EVENTS || self.journal_events >= MAX_TXN_EVENTS {
            return Err(self.fail(FaultCode::NeedsSpool, Some(txn_id.to_owned())));
        }
        let txn_payload = self.open[index].payload;
        let over_txn = txn_payload.checked_add(charge).is_none_or(|sum| sum > MAX_PAYLOAD_BYTES);
        let over_journal = self
            .journal_payload
            .checked_add(charge)
            .is_none_or(|sum| sum > MAX_PAYLOAD_BYTES);
        if charge > MAX_PAYLOAD_BYTES || over_txn || over_journal {
            return Err(self.fail(FaultCode::NeedsSpool, Some(txn_id.to_owned())));
        }
        Ok(())
    }

    fn store_data(
        &mut self,
        data: Incoming,
        index: usize,
        charge: u64,
        hash: String,
    ) -> Result<ApplyOut, Fault> {
        let Some(new_txn_payload) = self.open[index].payload.checked_add(charge) else {
            return Err(self.fail(FaultCode::NeedsSpool, Some(data.txn_id)));
        };
        let Some(new_journal_events) = self.journal_events.checked_add(1) else {
            return Err(self.fail(FaultCode::NeedsSpool, Some(data.txn_id)));
        };
        let Some(new_journal_payload) = self.journal_payload.checked_add(charge) else {
            return Err(self.fail(FaultCode::NeedsSpool, Some(data.txn_id)));
        };
        let event_index = self.open[index].events.len();
        self.open[index].by_global.insert(data.global, event_index);
        self.open[index]
            .by_collection
            .insert((data.table.clone(), data.collection_seq), event_index);
        self.open[index].events.push(StoredEvent {
            global: data.global,
            collection_seq: data.collection_seq,
            table: data.table,
            key: data.key,
            op: data.op,
            payload: data.payload,
            unavailable: data.unavailable,
            hash,
            charge,
        });
        self.open[index].payload = new_txn_payload;
        self.journal_events = new_journal_events;
        self.journal_payload = new_journal_payload;
        self.bump(SeenField::Data)?;
        Ok(accepted())
    }

    fn apply_end(
        &mut self,
        txn_id: String,
        count: u64,
        per_collection: BTreeMap<String, u64>,
        commit_lsn: String,
        timestamp: String,
    ) -> Result<ApplyOut, Fault> {
        if let Some(done) = self.known_end(&txn_id, count, &per_collection, &commit_lsn, &timestamp)? {
            return Ok(done);
        }
        let index = self.require_open(&txn_id)?;
        if let Err(code) = check_counts(&self.open[index], count, &per_collection) {
            return Err(self.fail(code, Some(txn_id)));
        }
        let (position, position_text) = self.end_position(&txn_id, &commit_lsn, &timestamp)?;
        self.commit_end(EndDraft {
            index,
            txn_id,
            count,
            per_collection,
            commit_lsn,
            timestamp,
            position,
            position_text,
        })
    }

    fn known_end(
        &mut self,
        txn_id: &str,
        count: u64,
        per_collection: &BTreeMap<String, u64>,
        commit_lsn: &str,
        timestamp: &str,
    ) -> Result<Option<ApplyOut>, Fault> {
        if let Err(code) = validate_txn_id(txn_id) {
            return Err(self.fail(code, None));
        }
        let same = self.done.get(txn_id).is_some_and(|done| {
            done.count == count
                && &done.per_collection == per_collection
                && done.commit_lsn == commit_lsn
                && done.timestamp == timestamp
        });
        if self.done.contains_key(txn_id) && same {
            self.bump(SeenField::DuplicateEnd)?;
            return Ok(Some(idempotent()));
        }
        if self.done.contains_key(txn_id) {
            return Err(self.fail(FaultCode::EndConflict, Some(txn_id.to_owned())));
        }
        if self.rolled_back.contains(txn_id) {
            return Err(self.fail(FaultCode::TxnIdReuse, Some(txn_id.to_owned())));
        }
        Ok(None)
    }

    fn require_open(&mut self, txn_id: &str) -> Result<usize, Fault> {
        let found = self.open.iter().position(|txn| txn.txn_id == txn_id);
        if let Some(index) = found {
            Ok(index)
        } else {
            Err(self.fail(FaultCode::UnknownTxn, Some(txn_id.to_owned())))
        }
    }

    fn end_position(
        &mut self,
        txn_id: &str,
        commit_lsn: &str,
        timestamp: &str,
    ) -> Result<(u128, String), Fault> {
        if let Err(code) = validate_timestamp(timestamp) {
            return Err(self.fail(code, Some(txn_id.to_owned())));
        }
        let lsn = match parse_commit_lsn(commit_lsn) {
            Ok(lsn) => lsn,
            Err(code) => return Err(self.fail(code, Some(txn_id.to_owned()))),
        };
        let position = match position_value(self.epoch, lsn) {
            Ok(position) => position,
            Err(code) => return Err(self.fail(code, Some(txn_id.to_owned()))),
        };
        let position_text = match encode_position(self.epoch, lsn) {
            Ok(text) => text,
            Err(code) => return Err(self.fail(code, Some(txn_id.to_owned()))),
        };
        if self.positions.contains(&position) {
            return Err(self.fail(
                FaultCode::CommitPositionConflict,
                Some(txn_id.to_owned()),
            ));
        }
        if self.floor.is_some_and(|floor| position < floor) {
            return Err(self.fail(
                FaultCode::CommitPositionRegression,
                Some(txn_id.to_owned()),
            ));
        }
        Ok((position, position_text))
    }

    fn commit_end(&mut self, draft: EndDraft) -> Result<ApplyOut, Fault> {
        let EndDraft {
            index,
            txn_id,
            count,
            per_collection,
            commit_lsn,
            timestamp,
            position,
            position_text,
        } = draft;
        let Some(next_order) = self.commit_order.checked_add(1) else {
            return Err(self.fail(FaultCode::CountOverflow, Some(txn_id)));
        };
        let Some(event_count) = u64::try_from(self.open[index].events.len()) else {
            return Err(self.fail(FaultCode::CountOverflow, Some(txn_id)));
        };
        let payload = self.open[index].payload;
        let Some(journal_events) = self.journal_events.checked_sub(event_count) else {
            return Err(self.fail(FaultCode::CountOverflow, Some(txn_id)));
        };
        let Some(journal_payload) = self.journal_payload.checked_sub(payload) else {
            return Err(self.fail(FaultCode::CountOverflow, Some(txn_id)));
        };
        let Some(completed) = self.emit(&EmitAt {
            index,
            commit_order: next_order,
            commit_lsn: &commit_lsn,
            position: &position_text,
            timestamp: &timestamp,
            count,
            per_collection: &per_collection,
        }) else {
            return Err(self.fail(FaultCode::BindRequired, Some(txn_id)));
        };
        let Some(selected_rows) = self.selected_rows.checked_add(completed.selected_row_count) else {
            return Err(self.fail(FaultCode::CountOverflow, Some(txn_id)));
        };
        self.finish_end(FinishedEnd {
            index,
            txn_id,
            count,
            per_collection,
            commit_lsn,
            timestamp,
            position,
            position_text,
            journal_events,
            journal_payload,
            next_order,
            selected_rows,
            completed,
        })
    }

    fn finish_end(&mut self, done: FinishedEnd) -> Result<ApplyOut, Fault> {
        self.open.remove(done.index);
        self.journal_events = done.journal_events;
        self.journal_payload = done.journal_payload;
        self.positions.insert(done.position);
        self.floor = Some(done.position);
        self.commit_order = done.next_order;
        self.selected_rows = done.selected_rows;
        self.last_position = Some(done.position_text);
        self.last_lsn = Some(done.commit_lsn.clone());
        self.last_epoch = Some(self.epoch);
        self.last_lineage = Some(self.lineage.clone());
        self.done.insert(
            done.txn_id,
            EndIdentity {
                count: done.count,
                per_collection: done.per_collection,
                commit_lsn: done.commit_lsn,
                timestamp: done.timestamp,
            },
        );
        self.bump(SeenField::End)?;
        Ok(ApplyOut {
            effect: Effect::Accepted,
            completed: Some(done.completed),
        })
    }

    fn emit(&self, at: &EmitAt<'_>) -> Option<CompletedTxn> {
        let txn = &self.open[at.index];
        let bind = self.bind.as_ref()?;
        let mut ordered: Vec<&StoredEvent> = txn.events.iter().collect();
        ordered.sort_by_key(|event| event.global);
        let mut rows = Vec::new();
        for event in ordered {
            if self.selected.contains(&event.table) {
                rows.push(emitted_row(event));
            }
        }
        let selected_row_count = u64::try_from(rows.len()).unwrap_or(u64::MAX);
        Some(CompletedTxn {
            contract: CONTRACT,
            kind: "complete_transaction",
            source: bind.source.clone(),
            lineage: self.lineage.clone(),
            epoch: self.epoch,
            schema_hash: bind.schema_hash.clone(),
            config_hash: bind.config_hash.clone(),
            type_hash: bind.type_hash.clone(),
            hook_hashes: bind.hook_hashes.clone(),
            selected_tables: bind.selected_tables.clone(),
            commit_position_provenance: PROVENANCE,
            txn_id: txn.txn_id.clone(),
            commit_order: at.commit_order,
            commit_lsn: at.commit_lsn.to_owned(),
            position: at.position.to_owned(),
            source_commit_timestamp: at.timestamp.to_owned(),
            event_count: at.count,
            per_collection_counts: at.per_collection.clone(),
            selected_row_count,
            rows,
            attestations: Attestations::fixed(),
        })
    }

    fn apply_rollback(&mut self, txn_id: String) -> Result<ApplyOut, Fault> {
        if let Err(code) = validate_txn_id(&txn_id) {
            return Err(self.fail(code, None));
        }
        if self.done.contains_key(&txn_id) {
            return Err(self.fail(FaultCode::RollbackAfterComplete, Some(txn_id)));
        }
        if self.rolled_back.contains(&txn_id) {
            self.bump(SeenField::DuplicateRollback)?;
            return Ok(idempotent());
        }
        let Some(index) = self.open.iter().position(|txn| txn.txn_id == txn_id) else {
            return Err(self.fail(FaultCode::UnknownTxn, Some(txn_id)));
        };
        if self.rolled_back.len() >= rollback_limit() {
            return Err(self.fail(FaultCode::NeedsSpool, Some(txn_id)));
        }
        let Some(event_count) = u64::try_from(self.open[index].events.len()) else {
            return Err(self.fail(FaultCode::CountOverflow, Some(txn_id)));
        };
        let payload = self.open[index].payload;
        let Some(journal_events) = self.journal_events.checked_sub(event_count) else {
            return Err(self.fail(FaultCode::CountOverflow, Some(txn_id)));
        };
        let Some(journal_payload) = self.journal_payload.checked_sub(payload) else {
            return Err(self.fail(FaultCode::CountOverflow, Some(txn_id)));
        };
        self.open.remove(index);
        self.journal_events = journal_events;
        self.journal_payload = journal_payload;
        self.rolled_back.insert(txn_id);
        self.bump(SeenField::Rollback)?;
        Ok(accepted())
    }

    fn fail(&mut self, code: FaultCode, txn_id: Option<String>) -> Fault {
        if code == FaultCode::NeedsSpool {
            self.needs_spool = true;
        }
        if let Some(id) = &txn_id {
            if let Some(txn) = self.open.iter_mut().find(|txn| txn.txn_id == *id) {
                txn.status = OpenStatus::Refused;
            }
        }
        self.resume_required = true;
        let fault = Fault { code, txn_id };
        self.fault = Some(fault.clone());
        fault
    }

    fn bump(&mut self, field: SeenField) -> Result<(), Fault> {
        let Some(next) = self.seen_get(field).checked_add(1) else {
            return Err(self.fail(FaultCode::CountOverflow, None));
        };
        self.seen_set(field, next);
        Ok(())
    }

    fn seen_get(&self, field: SeenField) -> u64 {
        match field {
            SeenField::Bind => self.seen.bind,
            SeenField::Barrier => self.seen.barrier,
            SeenField::Begin => self.seen.begin,
            SeenField::Data => self.seen.data,
            SeenField::End => self.seen.end,
            SeenField::Rollback => self.seen.rollback,
            SeenField::DuplicateData => self.seen.duplicate_data,
            SeenField::DuplicateBegin => self.seen.duplicate_begin,
            SeenField::DuplicateEnd => self.seen.duplicate_end,
            SeenField::DuplicateRollback => self.seen.duplicate_rollback,
        }
    }

    fn seen_set(&mut self, field: SeenField, value: u64) {
        let slot = match field {
            SeenField::Bind => &mut self.seen.bind,
            SeenField::Barrier => &mut self.seen.barrier,
            SeenField::Begin => &mut self.seen.begin,
            SeenField::Data => &mut self.seen.data,
            SeenField::End => &mut self.seen.end,
            SeenField::Rollback => &mut self.seen.rollback,
            SeenField::DuplicateData => &mut self.seen.duplicate_data,
            SeenField::DuplicateBegin => &mut self.seen.duplicate_begin,
            SeenField::DuplicateEnd => &mut self.seen.duplicate_end,
            SeenField::DuplicateRollback => &mut self.seen.duplicate_rollback,
        };
        *slot = value;
    }

    fn bump_lines(&mut self) -> Result<(), Fault> {
        match self.read_lines.checked_add(1) {
            Some(next) => {
                self.read_lines = next;
                Ok(())
            }
            None => Err(self.fail(FaultCode::CountOverflow, None)),
        }
    }

    fn add_bytes(&mut self, extra: u64) -> Result<(), Fault> {
        if self.fault.is_some() && extra == 0 {
            return Ok(());
        }
        match self.read_bytes.checked_add(extra) {
            Some(next) => {
                self.read_bytes = next;
                Ok(())
            }
            None => Err(self.fail(FaultCode::CountOverflow, None)),
        }
    }
}

#[derive(Clone, Copy)]
enum SeenField {
    Bind,
    Barrier,
    Begin,
    Data,
    End,
    Rollback,
    DuplicateData,
    DuplicateBegin,
    DuplicateEnd,
    DuplicateRollback,
}

fn accepted() -> ApplyOut {
    ApplyOut {
        effect: Effect::Accepted,
        completed: None,
    }
}

fn idempotent() -> ApplyOut {
    ApplyOut {
        effect: Effect::Idempotent,
        completed: None,
    }
}

fn rollback_limit() -> usize {
    usize::try_from(MAX_TXN_EVENTS).unwrap_or(usize::MAX)
}

fn emitted_row(event: &StoredEvent) -> EmittedRow {
    EmittedRow {
        global_txn_sequence: event.global,
        data_collection_sequence: event.collection_seq,
        table: event.table.clone(),
        key: event.key.clone(),
        op: event.op,
        payload: event.payload.clone(),
        unavailable_ordinals: event.unavailable.clone(),
        event_hash: event.hash.clone(),
    }
}

fn check_counts(
    txn: &OpenTxn,
    count: u64,
    declared: &BTreeMap<String, u64>,
) -> Result<(), FaultCode> {
    check_declared_sum(count, declared)?;
    check_declared_names(declared)?;
    let observed = observed_sequences(txn);
    if !same_collection_names(&observed, declared) {
        return Err(FaultCode::CollectionUnknown);
    }
    check_collection_sizes(&observed, declared)?;
    check_global_size(txn, count)?;
    check_collection_sequences(&observed, declared)?;
    check_global_sequences(txn, count)
}

fn check_declared_sum(count: u64, declared: &BTreeMap<String, u64>) -> Result<(), FaultCode> {
    let mut sum = 0u64;
    for value in declared.values() {
        sum = sum.checked_add(*value).ok_or(FaultCode::CountOverflow)?;
    }
    if sum == count {
        Ok(())
    } else {
        Err(FaultCode::CollectionCountGap)
    }
}

fn check_declared_names(declared: &BTreeMap<String, u64>) -> Result<(), FaultCode> {
    for name in declared.keys() {
        if validate_table(name).is_err() {
            return Err(FaultCode::CollectionInvalid);
        }
    }
    Ok(())
}

fn observed_sequences(txn: &OpenTxn) -> BTreeMap<String, Vec<u64>> {
    let mut observed: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for event in &txn.events {
        observed
            .entry(event.table.clone())
            .or_default()
            .push(event.collection_seq);
    }
    observed
}

fn same_collection_names(observed: &BTreeMap<String, Vec<u64>>, declared: &BTreeMap<String, u64>) -> bool {
    observed.len() == declared.len() && observed.keys().all(|name| declared.contains_key(name))
}

fn check_collection_sizes(
    observed: &BTreeMap<String, Vec<u64>>,
    declared: &BTreeMap<String, u64>,
) -> Result<(), FaultCode> {
    for (name, sequences) in observed {
        let have = u64::try_from(sequences.len()).map_err(|_| FaultCode::CountOverflow)?;
        let declared_count = declared[name];
        if have < declared_count {
            return Err(FaultCode::EndTooEarly);
        }
        if have > declared_count {
            return Err(FaultCode::CollectionCountGap);
        }
    }
    Ok(())
}

fn check_global_size(txn: &OpenTxn, count: u64) -> Result<(), FaultCode> {
    let stored = u64::try_from(txn.events.len()).map_err(|_| FaultCode::CountOverflow)?;
    if stored < count {
        Err(FaultCode::EndTooEarly)
    } else if stored > count {
        Err(FaultCode::CountMismatch)
    } else {
        Ok(())
    }
}

fn check_collection_sequences(
    observed: &BTreeMap<String, Vec<u64>>,
    declared: &BTreeMap<String, u64>,
) -> Result<(), FaultCode> {
    for (name, sequences) in observed {
        let declared_count = declared[name];
        let mut seen = HashSet::new();
        for sequence in sequences {
            if *sequence == 0 || *sequence > declared_count {
                return Err(FaultCode::SequenceOutOfRange);
            }
            if !seen.insert(*sequence) {
                return Err(FaultCode::DuplicateConflict);
            }
        }
        let have = u64::try_from(seen.len()).map_err(|_| FaultCode::CountOverflow)?;
        if have != declared_count {
            return Err(FaultCode::SequenceGap);
        }
    }
    Ok(())
}

fn check_global_sequences(txn: &OpenTxn, count: u64) -> Result<(), FaultCode> {
    let mut globals = HashSet::new();
    for event in &txn.events {
        if event.global == 0 || event.global > count {
            return Err(FaultCode::SequenceOutOfRange);
        }
        if !globals.insert(event.global) {
            return Err(FaultCode::DuplicateConflict);
        }
    }
    let have = u64::try_from(globals.len()).map_err(|_| FaultCode::CountOverflow)?;
    if have == count {
        Ok(())
    } else {
        Err(FaultCode::SequenceGap)
    }
}

fn is_hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
