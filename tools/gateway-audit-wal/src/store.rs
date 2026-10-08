//! Directory spool. One exclusive `File` lock. Frames are durable only after `fsync`.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use oo_gateway::query_executor::{
    Admission, AuditAdmission, CompletionRecord, CompletionStatus, ExecError, StopClass,
};

use crate::error::{io_fault, FaultKind, WalError};
use crate::frame::{
    decode_payload, encode_frame, encode_payload, footer, scan_segment, segment_header, sha256,
    uuid_string, DecodedFrame, Kind, Payload, SEGMENT_HEADER_LEN,
};

const SEGMENT_CAP: u64 = 16 * 1024 * 1024;
const CHECK_MAGIC: &[u8; 8] = b"OAWCHK01";

#[derive(Clone, Debug)]
pub struct WalConfig {
    pub max_bytes: u64,
}

impl Default for WalConfig {
    fn default() -> Self {
        Self {
            max_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FrameView {
    pub spool_id: String,
    pub request_id: String,
    pub event_seq: u64,
    pub kind: String,
    pub project: String,
    pub role: String,
    pub payload_hash: String,
    pub segment_id: u32,
}

#[derive(Clone, Debug)]
pub struct RequestView {
    pub request_id: String,
    pub state: String,
    pub project: String,
    pub role: String,
}

#[derive(Clone, Debug)]
pub struct SealedFrame {
    pub spool_id: String,
    pub request_id: String,
    pub request: [u8; 16],
    pub event_seq: u64,
    pub payload_hash: [u8; 32],
    pub payload_hex: String,
    pub project: String,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct Inspect {
    pub spool_id: String,
    pub frames: Vec<FrameView>,
    pub requests: Vec<RequestView>,
    pub torn_tail: bool,
}

struct Stored {
    segment: u32,
    request: [u8; 16],
    seq: u64,
    kind: Kind,
    hash: [u8; 32],
    payload: Payload,
}

struct Pending {
    request: [u8; 16],
    meta_key: String,
}

pub struct DiskWal {
    root: PathBuf,
    spool: [u8; 16],
    // The open file owns the exclusive lock until this value is dropped.
    #[allow(dead_code)]
    lock: File,
    active: File,
    active_id: u32,
    active_len: u64,
    active_frames: u32,
    used: u64,
    sealed: BTreeMap<u32, [u8; 32]>,
    index: BTreeMap<([u8; 16], u64), [u8; 32]>,
    stored: Vec<Stored>,
    pending: Option<Pending>,
    sync_faults: u32,
    config: WalConfig,
    poisoned: bool,
}

impl DiskWal {
    pub fn open(root: &Path, config: WalConfig) -> Result<Self, WalError> {
        if config.max_bytes < 64 {
            return Err(WalError::Bound);
        }
        fs::create_dir_all(root).map_err(WalError::Io)?;
        if let Some(parent) = root.parent() {
            sync_dir(parent)?;
        }
        sync_dir(root)?;
        let lock_path = root.join("writer.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(WalError::Io)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(WalError::Busy),
            Err(TryLockError::Error(err)) => return Err(WalError::Io(err)),
        }
        sync_dir(root)?;
        // One allocated page so a later ENOSPC can still overwrite the fault token.
        // fs::write truncates first and then cannot allocate that page.
        reserve_fault_slot(root)?;
        let spool = load_spool_id(root)?;
        let active = lock.try_clone().map_err(WalError::Io)?;
        let mut wal = Self {
            root: root.to_path_buf(),
            spool,
            lock,
            active,
            active_id: 0,
            active_len: 0,
            active_frames: 0,
            used: 0,
            sealed: BTreeMap::new(),
            index: BTreeMap::new(),
            stored: Vec::new(),
            pending: None,
            sync_faults: 0,
            config,
            poisoned: false,
        };
        wal.reload()?;
        Ok(wal)
    }

    pub fn spool_id(&self) -> String {
        uuid_string(&self.spool)
    }

    pub fn fail_next_syncs(&mut self, count: u32) {
        self.sync_faults = count;
    }

    pub fn record_not_admitted(&mut self, project: &str, role: &str) -> Result<(), WalError> {
        let project = crate::frame::require_project(project)?;
        let role = crate::frame::require_ident(role)?;
        let payload = Payload {
            kind: Kind::Refusal,
            original: String::new(),
            executed: String::new(),
            fingerprint: String::new(),
            role,
            project,
            policy: String::new(),
            backend_pid: 0,
            backend_start: String::new(),
            status: String::new(),
            truncated: false,
            returned_rows: 0,
            result_bytes: 0,
            rollback: false,
            socket: false,
            stop: String::new(),
            sqlstate: String::new(),
            reason: "explicit_not_admitted".to_string(),
        };
        let bytes = encode_payload(&payload)?;
        let request = new_id()?;
        self.append(Kind::Refusal, request, 1, &bytes, payload)?;
        Ok(())
    }

    pub fn seal_active_segment(&mut self) -> Result<(), WalError> {
        if self.poisoned {
            return Err(WalError::Corrupt);
        }
        if self.active_frames == 0 {
            return Ok(());
        }
        let mut content = vec![0u8; self.active_len as usize];
        self.active.seek(SeekFrom::Start(0)).map_err(WalError::Io)?;
        self.active.read_exact(&mut content).map_err(WalError::Io)?;
        let foot = footer(&content, self.active_frames);
        if self.used.saturating_add(foot.len() as u64) > self.config.max_bytes {
            self.note(FaultKind::Quota);
            return Err(WalError::Quota);
        }
        self.active.seek(SeekFrom::End(0)).map_err(WalError::Io)?;
        self.active.write_all(&foot).map_err(|err| self.mark_io(err))?;
        if let Err(err) = self.sync_active() {
            let _ = self.active.set_len(self.active_len);
            self.force_sync();
            return Err(err);
        }
        self.active_len += foot.len() as u64;
        self.used += foot.len() as u64;
        let hash = sha256_file(&self.segment_path(self.active_id))?;
        self.sealed.insert(self.active_id, hash);
        let next = self.active_id.saturating_add(1);
        if next == 0 {
            self.poisoned = true;
            return Err(WalError::Bound);
        }
        if let Err(err) = self.create_segment(next) {
            self.poisoned = true;
            return Err(err);
        }
        Ok(())
    }

    pub fn sealed_frames(&self) -> Result<Vec<SealedFrame>, WalError> {
        let rows = read_checkpoint(&self.root)?;
        let mut out = Vec::new();
        for item in &self.stored {
            if !self.sealed.contains_key(&item.segment) {
                continue;
            }
            if rows.iter().any(|row| row.request == item.request && row.seq == item.seq) {
                continue;
            }
            let bytes = encode_payload(&item.payload)?;
            if sha256(&bytes) != item.hash {
                return Err(WalError::Corrupt);
            }
            out.push(SealedFrame {
                spool_id: uuid_string(&self.spool),
                request_id: uuid_string(&item.request),
                request: item.request,
                event_seq: item.seq,
                payload_hash: item.hash,
                payload_hex: crate::frame::hex_encode(&bytes),
                project: item.payload.project.clone(),
                kind: item.kind.as_str().to_string(),
            });
        }
        Ok(out)
    }

    pub fn checkpoint_ack(
        &mut self,
        request: [u8; 16],
        seq: u64,
        payload_hash: [u8; 32],
    ) -> Result<(), WalError> {
        let stored = self
            .stored
            .iter()
            .find(|item| item.request == request && item.seq == seq)
            .ok_or(WalError::AckMismatch)?;
        if stored.hash != payload_hash {
            return Err(WalError::AckMismatch);
        }
        let segment_hash = self.sealed.get(&stored.segment).copied().ok_or(WalError::AckMismatch)?;
        let mut rows = read_checkpoint(&self.root)?;
        if let Some(existing) = rows.iter().find(|row| row.request == request && row.seq == seq) {
            if existing.payload_hash != payload_hash || existing.segment_hash != segment_hash {
                return Err(WalError::Conflict);
            }
            return Ok(());
        }
        if rows.iter().any(|row| row.segment == stored.segment && row.segment_hash != segment_hash) {
            return Err(WalError::Conflict);
        }
        rows.push(CheckRow {
            spool: self.spool,
            request,
            seq,
            payload_hash,
            segment: stored.segment,
            segment_hash,
        });
        write_checkpoint(&self.root, &rows)?;
        Ok(())
    }

    pub fn gc_acknowledged(&mut self) -> Result<u32, WalError> {
        let rows = read_checkpoint(&self.root)?;
        let mut remove = Vec::new();
        for (segment, expected_hash) in &self.sealed {
            let path = self.segment_path(*segment);
            let bytes = fs::read(&path).map_err(WalError::Io)?;
            let live = sha256(&bytes);
            if &live != expected_hash {
                return Err(WalError::Corrupt);
            }
            let scan = scan_segment(&bytes)?;
            if !scan.sealed || scan.torn || scan.frames.is_empty() {
                return Err(WalError::Corrupt);
            }
            let mut complete = true;
            for frame in &scan.frames {
                let hit = rows.iter().find(|row| {
                    row.request == frame.request && row.seq == frame.seq && row.segment == *segment
                });
                match hit {
                    Some(row) if row.payload_hash == frame.payload_hash && row.segment_hash == live => {}
                    Some(_) => return Err(WalError::Corrupt),
                    None => complete = false,
                }
            }
            if complete {
                remove.push(*segment);
            }
        }
        for segment in &remove {
            fs::remove_file(self.segment_path(*segment)).map_err(WalError::Io)?;
            self.sealed.remove(segment);
            self.stored.retain(|item| item.segment != *segment);
        }
        if !remove.is_empty() {
            sync_dir(&self.root)?;
        }
        u32::try_from(remove.len()).map_err(|_| WalError::Bound)
    }

    fn reload(&mut self) -> Result<(), WalError> {
        let mut ids = segment_ids(&self.root)?;
        if ids.is_empty() {
            self.create_segment(1)?;
            return Ok(());
        }
        ids.sort_unstable();
        for (index, id) in ids.iter().enumerate() {
            if index > 0 {
                let expected = ids[index - 1].checked_add(1).ok_or(WalError::Corrupt)?;
                if *id != expected {
                    return Err(WalError::Corrupt);
                }
            }
            let path = self.segment_path(*id);
            let meta = fs::metadata(&path).map_err(WalError::Io)?;
            if meta.len() > SEGMENT_CAP {
                return Err(WalError::Corrupt);
            }
            let bytes = fs::read(&path).map_err(WalError::Io)?;
            self.used += bytes.len() as u64;
            let scan = scan_segment(&bytes)?;
            if scan.segment_id != *id {
                return Err(WalError::Corrupt);
            }
            let last = index + 1 == ids.len();
            if scan.torn && (scan.sealed || !last) {
                return Err(WalError::Corrupt);
            }
            for frame in &scan.frames {
                self.remember(*id, frame)?;
            }
            if scan.sealed {
                self.sealed.insert(*id, sha256(&bytes));
            }
            if last && !scan.sealed {
                if scan.keep < bytes.len() {
                    let file = OpenOptions::new().read(true).write(true).open(&path).map_err(WalError::Io)?;
                    file.set_len(scan.keep as u64).map_err(WalError::Io)?;
                    file.sync_all().map_err(WalError::Io)?;
                    self.used -= (bytes.len() - scan.keep) as u64;
                    self.active = file;
                } else {
                    self.active = OpenOptions::new().read(true).write(true).open(&path).map_err(WalError::Io)?;
                }
                self.active_id = *id;
                self.active_len = scan.keep as u64;
                self.active_frames = u32::try_from(scan.frames.len()).map_err(|_| WalError::Bound)?;
            }
        }
        if self.active_id == 0 {
            let last = ids.last().copied().ok_or(WalError::Corrupt)?;
            let next = last.checked_add(1).ok_or(WalError::Bound)?;
            self.create_segment(next)?;
        }
        if self.used > self.config.max_bytes {
            return Err(WalError::Quota);
        }
        Ok(())
    }

    fn remember(&mut self, segment: u32, frame: &DecodedFrame) -> Result<(), WalError> {
        if frame.spool != self.spool {
            return Err(WalError::Corrupt);
        }
        let payload = decode_payload(&frame.payload)?;
        if payload.kind != frame.kind {
            return Err(WalError::Corrupt);
        }
        if self.index.insert((frame.request, frame.seq), frame.payload_hash).is_some() {
            return Err(WalError::Corrupt);
        }
        self.stored.push(Stored {
            segment,
            request: frame.request,
            seq: frame.seq,
            kind: frame.kind,
            hash: frame.payload_hash,
            payload,
        });
        Ok(())
    }

    fn create_segment(&mut self, id: u32) -> Result<(), WalError> {
        let path = self.segment_path(id);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(WalError::Io)?;
        let header = segment_header(id);
        if self.used.saturating_add(header.len() as u64) > self.config.max_bytes {
            drop(file);
            let _ = fs::remove_file(&path);
            self.note(FaultKind::Quota);
            return Err(WalError::Quota);
        }
        if let Err(err) = file.write_all(&header).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&path);
            self.note(io_fault(&err));
            return Err(WalError::Io(err));
        }
        if let Err(err) = sync_dir(&self.root) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(err);
        }
        self.used += header.len() as u64;
        self.active = file;
        self.active_id = id;
        self.active_len = SEGMENT_HEADER_LEN as u64;
        self.active_frames = 0;
        Ok(())
    }

    fn append(
        &mut self,
        kind: Kind,
        request: [u8; 16],
        seq: u64,
        payload_bytes: &[u8],
        payload: Payload,
    ) -> Result<(), WalError> {
        if self.poisoned {
            return Err(WalError::Corrupt);
        }
        if self.index.contains_key(&(request, seq)) {
            return Err(WalError::Conflict);
        }
        let frame = encode_frame(kind, &self.spool, &request, seq, payload_bytes)?;
        let hash = sha256(payload_bytes);
        if self.used.saturating_add(frame.len() as u64) > self.config.max_bytes {
            self.note(FaultKind::Quota);
            return Err(WalError::Quota);
        }
        let previous = self.active_len;
        self.active.seek(SeekFrom::End(0)).map_err(WalError::Io)?;
        if let Err(err) = self.active.write_all(&frame) {
            self.note(io_fault(&err));
            let _ = self.active.set_len(previous);
            self.force_sync();
            return Err(WalError::Io(err));
        }
        self.active_len += frame.len() as u64;
        if let Err(err) = self.sync_active() {
            let _ = self.active.set_len(previous);
            self.active_len = previous;
            self.force_sync();
            return Err(err);
        }
        self.used += frame.len() as u64;
        self.active_frames += 1;
        self.index.insert((request, seq), hash);
        self.stored.push(Stored {
            segment: self.active_id,
            request,
            seq,
            kind,
            hash,
            payload,
        });
        Ok(())
    }

    fn sync_active(&mut self) -> Result<(), WalError> {
        if self.sync_faults > 0 {
            self.sync_faults -= 1;
            self.note(FaultKind::Sync);
            self.poisoned = true;
            return Err(WalError::Io(std::io::Error::other("injected sync failure")));
        }
        self.active.sync_all().map_err(|err| {
            self.note(io_fault(&err));
            if matches!(io_fault(&err), FaultKind::NoSpace) {
                self.poisoned = true;
            }
            WalError::Io(err)
        })
    }

    fn force_sync(&mut self) {
        let saved = self.sync_faults;
        self.sync_faults = 0;
        let _ = self.active.sync_all();
        self.sync_faults = saved;
    }

    fn note(&self, kind: FaultKind) {
        let path = self.root.join("last-fault.txt");
        let bytes = kind.as_str().as_bytes();
        let Ok(mut file) = OpenOptions::new().write(true).open(&path) else {
            return;
        };
        if file.seek(SeekFrom::Start(0)).is_err() || file.write_all(bytes).is_err() {
            return;
        }
        let _ = file.set_len(bytes.len() as u64);
        let _ = file.sync_all();
    }

    fn mark_io(&self, err: std::io::Error) -> WalError {
        self.note(io_fault(&err));
        WalError::Io(err)
    }

    fn segment_path(&self, id: u32) -> PathBuf {
        self.root.join(format!("seg-{id:08}.wal"))
    }

    fn admit_sync(&mut self, admission: &Admission<'_>) -> Result<(), ExecError> {
        let built = payload_from_admission(admission).map_err(|err| self.as_exec(err))?;
        let bytes = encode_payload(&built).map_err(|err| self.as_exec(err))?;
        let request = new_id().map_err(|err| self.as_exec(err))?;
        let key = meta_key(&built);
        self.append(Kind::Admission, request, 1, &bytes, built)
            .map_err(|err| self.as_exec(err))?;
        self.pending = Some(Pending { request, meta_key: key });
        Ok(())
    }

    fn complete_sync(&mut self, completion: &CompletionRecord) -> Result<(), ExecError> {
        let pending = self.pending.take().ok_or(ExecError::AuditUnavailable)?;
        let built = match payload_from_completion(completion) {
            Ok(value) => value,
            Err(err) => {
                self.pending = Some(pending);
                return Err(self.as_exec(err));
            }
        };
        if meta_key(&built) != pending.meta_key {
            self.pending = Some(pending);
            self.note(FaultKind::Corrupt);
            return Err(ExecError::AuditUnavailable);
        }
        let bytes = match encode_payload(&built) {
            Ok(value) => value,
            Err(err) => {
                self.pending = Some(pending);
                return Err(self.as_exec(err));
            }
        };
        if let Err(err) = self.append(Kind::Completion, pending.request, 2, &bytes, built) {
            self.pending = Some(pending);
            return Err(self.as_exec(err));
        }
        Ok(())
    }

    fn as_exec(&self, err: WalError) -> ExecError {
        let kind = match &err {
            WalError::Quota => FaultKind::Quota,
            WalError::Corrupt => FaultKind::Corrupt,
            WalError::Io(io) => io_fault(io),
            _ => FaultKind::Write,
        };
        self.note(kind);
        let _ = err;
        ExecError::AuditUnavailable
    }
}

impl AuditAdmission for DiskWal {
    fn admit(
        &mut self,
        admission: &Admission<'_>,
    ) -> impl std::future::Future<Output = Result<(), ExecError>> + Send {
        std::future::ready(self.admit_sync(admission))
    }

    fn complete(
        &mut self,
        completion: &CompletionRecord,
    ) -> impl std::future::Future<Output = Result<(), ExecError>> + Send {
        std::future::ready(self.complete_sync(completion))
    }
}

pub fn inspect(root: &Path) -> Result<Inspect, WalError> {
    let spool = read_spool_id(root)?;
    let mut ids = segment_ids(root)?;
    ids.sort_unstable();
    let mut frames = Vec::new();
    let mut grouped: BTreeMap<[u8; 16], Vec<Stored>> = BTreeMap::new();
    let mut torn_tail = false;
    for (index, id) in ids.iter().enumerate() {
        let bytes = fs::read(root.join(format!("seg-{id:08}.wal"))).map_err(WalError::Io)?;
        if bytes.len() as u64 > SEGMENT_CAP {
            return Err(WalError::Corrupt);
        }
        let scan = scan_segment(&bytes)?;
        let last = index + 1 == ids.len();
        if scan.torn && (scan.sealed || !last) {
            return Err(WalError::Corrupt);
        }
        if scan.torn {
            torn_tail = true;
        }
        for frame in scan.frames {
            if frame.spool != spool {
                return Err(WalError::Corrupt);
            }
            let payload = decode_payload(&frame.payload)?;
            frames.push(FrameView {
                spool_id: uuid_string(&frame.spool),
                request_id: uuid_string(&frame.request),
                event_seq: frame.seq,
                kind: frame.kind.as_str().to_string(),
                project: payload.project.clone(),
                role: payload.role.clone(),
                payload_hash: crate::frame::hex_encode(&frame.payload_hash),
                segment_id: *id,
            });
            grouped.entry(frame.request).or_default().push(Stored {
                segment: *id,
                request: frame.request,
                seq: frame.seq,
                kind: frame.kind,
                hash: frame.payload_hash,
                payload,
            });
        }
    }
    let mut requests = Vec::new();
    for (request, items) in grouped {
        let state = request_state(&items)?.to_string();
        let first = &items[0].payload;
        requests.push(RequestView {
            request_id: uuid_string(&request),
            state,
            project: first.project.clone(),
            role: first.role.clone(),
        });
    }
    Ok(Inspect {
        spool_id: uuid_string(&spool),
        frames,
        requests,
        torn_tail,
    })
}

fn request_state(items: &[Stored]) -> Result<&'static str, WalError> {
    let mut admission = false;
    let mut refusal = false;
    let mut status: Option<&str> = None;
    for item in items {
        match item.kind {
            Kind::Admission => {
                if admission || refusal || status.is_some() || item.seq != 1 {
                    return Err(WalError::Corrupt);
                }
                admission = true;
            }
            Kind::Refusal => {
                if admission || refusal || status.is_some() || item.seq != 1 {
                    return Err(WalError::Corrupt);
                }
                refusal = true;
            }
            Kind::Completion => {
                if !admission || refusal || status.is_some() || item.seq != 2 {
                    return Err(WalError::Corrupt);
                }
                status = Some(item.payload.status.as_str());
            }
        }
    }
    if refusal {
        return Ok("not_admitted");
    }
    if let Some(status) = status {
        return match status {
            "succeeded" => Ok("succeeded"),
            "partial" => Ok("partial"),
            "failed" => Ok("failed"),
            _ => Err(WalError::Corrupt),
        };
    }
    if admission {
        return Ok("admitted");
    }
    Err(WalError::Corrupt)
}

fn payload_from_admission(admission: &Admission<'_>) -> Result<Payload, WalError> {
    Ok(Payload {
        kind: Kind::Admission,
        original: crate::frame::require_sha256_hex(admission.original_sha256())?,
        executed: crate::frame::require_sha256_hex(admission.executed_sha256())?,
        fingerprint: crate::frame::require_fingerprint(admission.fingerprint_hex())?,
        role: crate::frame::require_ident(admission.role())?,
        project: crate::frame::require_project(admission.project())?,
        policy: crate::frame::require_policy(admission.policy_id())?,
        backend_pid: admission.backend_pid(),
        backend_start: crate::frame::require_backend_start(admission.backend_start())?,
        status: String::new(),
        truncated: false,
        returned_rows: 0,
        result_bytes: 0,
        rollback: false,
        socket: false,
        stop: String::new(),
        sqlstate: String::new(),
        reason: String::new(),
    })
}

fn payload_from_completion(completion: &CompletionRecord) -> Result<Payload, WalError> {
    let stop = match completion.stop() {
        None => String::new(),
        Some(StopClass::Timeout) => "timeout".to_string(),
        Some(StopClass::Canceled) => "canceled".to_string(),
        Some(StopClass::TypeRefused) => "type_refused".to_string(),
        Some(StopClass::Database) => "database".to_string(),
        Some(StopClass::Protocol) => "protocol".to_string(),
    };
    let status = match completion.status() {
        CompletionStatus::Succeeded => "succeeded",
        CompletionStatus::Partial => "partial",
        CompletionStatus::Failed => "failed",
    };
    Ok(Payload {
        kind: Kind::Completion,
        original: crate::frame::require_sha256_hex(completion.original_sha256())?,
        executed: crate::frame::require_sha256_hex(completion.executed_sha256())?,
        fingerprint: crate::frame::require_fingerprint(completion.fingerprint_hex())?,
        role: crate::frame::require_ident(completion.role())?,
        project: crate::frame::require_project(completion.project())?,
        policy: crate::frame::require_policy(completion.policy_id())?,
        backend_pid: completion.backend_pid(),
        backend_start: crate::frame::require_backend_start(completion.backend_start())?,
        status: status.to_string(),
        truncated: completion.truncated(),
        returned_rows: completion.returned_rows(),
        result_bytes: completion.result_bytes(),
        rollback: completion.rollback_confirmed(),
        socket: completion.socket_dropped(),
        stop,
        sqlstate: crate::frame::require_sqlstate(completion.sqlstate().unwrap_or(""))?,
        reason: String::new(),
    })
}

fn meta_key(payload: &Payload) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        payload.original,
        payload.executed,
        payload.fingerprint,
        payload.role,
        payload.project,
        payload.policy,
        payload.backend_pid,
        payload.backend_start
    )
}

fn new_id() -> Result<[u8; 16], WalError> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|err| {
        WalError::Io(std::io::Error::other(err.to_string()))
    })?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes)
}

fn load_spool_id(root: &Path) -> Result<[u8; 16], WalError> {
    let path = root.join("spool.id");
    if path.exists() {
        return read_spool_id(root);
    }
    let id = new_id()?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(WalError::Io)?;
    file.write_all(&id).map_err(WalError::Io)?;
    file.sync_all().map_err(WalError::Io)?;
    sync_dir(root)?;
    Ok(id)
}

fn read_spool_id(root: &Path) -> Result<[u8; 16], WalError> {
    let bytes = fs::read(root.join("spool.id")).map_err(WalError::Io)?;
    if bytes.len() != 16 {
        return Err(WalError::Corrupt);
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(&bytes);
    Ok(id)
}

fn segment_ids(root: &Path) -> Result<Vec<u32>, WalError> {
    let mut ids = Vec::new();
    for entry in fs::read_dir(root).map_err(WalError::Io)? {
        let entry = entry.map_err(WalError::Io)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(rest) = name.strip_prefix("seg-") else {
            continue;
        };
        let Some(number) = rest.strip_suffix(".wal") else {
            continue;
        };
        if number.len() != 8 || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(WalError::Corrupt);
        }
        let id: u32 = number.parse().map_err(|_| WalError::Corrupt)?;
        if id == 0 {
            return Err(WalError::Corrupt);
        }
        ids.push(id);
    }
    Ok(ids)
}

fn reserve_fault_slot(root: &Path) -> Result<(), WalError> {
    let path = root.join("last-fault.txt");
    if fs::metadata(&path).map_or(0, |meta| meta.len()) > 0 {
        return Ok(());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(WalError::Io)?;
    file.write_all(&[0u8; 4096]).map_err(WalError::Io)?;
    file.sync_all().map_err(WalError::Io)?;
    sync_dir(root)
}

fn sync_dir(path: &Path) -> Result<(), WalError> {
    let dir = File::open(path).map_err(WalError::Io)?;
    dir.sync_all().map_err(WalError::Io)?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<[u8; 32], WalError> {
    let bytes = fs::read(path).map_err(WalError::Io)?;
    Ok(sha256(&bytes))
}

struct CheckRow {
    spool: [u8; 16],
    request: [u8; 16],
    seq: u64,
    payload_hash: [u8; 32],
    segment: u32,
    segment_hash: [u8; 32],
}

fn read_checkpoint(root: &Path) -> Result<Vec<CheckRow>, WalError> {
    let path = root.join("frontier.chk");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(&path).map_err(WalError::Io)?;
    if bytes.len() < 8 + 32 || &bytes[..8] != CHECK_MAGIC {
        return Err(WalError::Corrupt);
    }
    let body = &bytes[..bytes.len() - 32];
    let mut expected = [0u8; 32];
    expected.copy_from_slice(&bytes[bytes.len() - 32..]);
    if sha256(body) != expected {
        return Err(WalError::Corrupt);
    }
    let mut rest = &body[8..];
    let mut rows = Vec::new();
    const ROW: usize = 16 + 16 + 8 + 32 + 4 + 32;
    while !rest.is_empty() {
        if rest.len() < ROW {
            return Err(WalError::Corrupt);
        }
        let mut spool = [0u8; 16];
        spool.copy_from_slice(&rest[..16]);
        let mut request = [0u8; 16];
        request.copy_from_slice(&rest[16..32]);
        let seq = u64::from_be_bytes(rest[32..40].try_into().map_err(|_| WalError::Corrupt)?);
        let mut payload_hash = [0u8; 32];
        payload_hash.copy_from_slice(&rest[40..72]);
        let segment = u32::from_be_bytes(rest[72..76].try_into().map_err(|_| WalError::Corrupt)?);
        let mut segment_hash = [0u8; 32];
        segment_hash.copy_from_slice(&rest[76..108]);
        rows.push(CheckRow {
            spool,
            request,
            seq,
            payload_hash,
            segment,
            segment_hash,
        });
        rest = &rest[ROW..];
    }
    Ok(rows)
}

fn write_checkpoint(root: &Path, rows: &[CheckRow]) -> Result<(), WalError> {
    let mut body = Vec::new();
    body.extend_from_slice(CHECK_MAGIC);
    for row in rows {
        body.extend_from_slice(&row.spool);
        body.extend_from_slice(&row.request);
        body.extend_from_slice(&row.seq.to_be_bytes());
        body.extend_from_slice(&row.payload_hash);
        body.extend_from_slice(&row.segment.to_be_bytes());
        body.extend_from_slice(&row.segment_hash);
    }
    let sum = sha256(&body);
    body.extend_from_slice(&sum);
    let tmp = root.join("frontier.chk.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)
        .map_err(WalError::Io)?;
    file.write_all(&body).map_err(WalError::Io)?;
    file.sync_all().map_err(WalError::Io)?;
    fs::rename(&tmp, root.join("frontier.chk")).map_err(WalError::Io)?;
    sync_dir(root)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static N: AtomicU64 = AtomicU64::new(0);

    fn scratch() -> PathBuf {
        let id = N.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("oo-wal-{}-{}", std::process::id(), id));
        fs::create_dir(&path).unwrap();
        path
    }

    fn admission() -> Payload {
        Payload {
            kind: Kind::Admission,
            original: "ab".repeat(32),
            executed: "cd".repeat(32),
            fingerprint: "0123456789abcdef".to_string(),
            role: "oo_r_clerk".to_string(),
            project: "shop".to_string(),
            policy: "a".repeat(64),
            backend_pid: 7,
            backend_start: "20261008010101000000".to_string(),
            status: String::new(),
            truncated: false,
            returned_rows: 0,
            result_bytes: 0,
            rollback: false,
            socket: false,
            stop: String::new(),
            sqlstate: String::new(),
            reason: String::new(),
        }
    }

    #[test]
    fn lock_blocks_a_second_writer() {
        let dir = scratch();
        let first = DiskWal::open(&dir, WalConfig { max_bytes: 1_048_576 }).unwrap();
        let second = DiskWal::open(&dir, WalConfig { max_bytes: 1_048_576 });
        assert!(matches!(second, Err(WalError::Busy)));
        drop(first);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn torn_tail_keeps_the_frame_and_corruption_fails_closed() {
        let dir = scratch();
        let mut wal = DiskWal::open(&dir, WalConfig { max_bytes: 1_048_576 }).unwrap();
        let payload = admission();
        let bytes = encode_payload(&payload).unwrap();
        let request = new_id().unwrap();
        wal.append(Kind::Admission, request, 1, &bytes, payload).unwrap();
        wal.seal_active_segment().unwrap();
        drop(wal);
        let path = dir.join("seg-00000001.wal");
        let mut raw = fs::read(&path).unwrap();
        let original = raw.clone();
        let mut torn = segment_header(2).to_vec();
        torn.extend_from_slice(&[0x01, 0x02, 0x03]);
        fs::write(dir.join("seg-00000002.wal"), &torn).unwrap();
        let view = inspect(&dir).unwrap();
        assert!(view.torn_tail);
        assert_eq!(view.requests.len(), 1);
        assert_eq!(view.requests[0].state, "admitted");
        assert!(!view.requests.iter().any(|item| item.state == "succeeded"));
        raw[40] ^= 0xff;
        fs::write(&path, &raw).unwrap();
        assert!(matches!(inspect(&dir), Err(WalError::Corrupt)));
        fs::write(&path, &original).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn quota_does_not_write() {
        let dir = scratch();
        let mut wal = DiskWal::open(&dir, WalConfig { max_bytes: 1024 }).unwrap();
        let err = wal.record_not_admitted("shop;select", "oo_r_clerk");
        assert!(matches!(err, Err(WalError::Bound)));
        assert!(inspect(&dir).unwrap().frames.is_empty());
        drop(wal);
        let _ = fs::remove_dir_all(&dir);

        let tight = scratch();
        let mut wal = DiskWal::open(&tight, WalConfig { max_bytes: 100 }).unwrap();
        let err = wal.record_not_admitted("shop", "oo_r_clerk");
        assert!(matches!(err, Err(WalError::Quota)));
        drop(wal);
        assert!(inspect(&tight).unwrap().frames.is_empty());
        assert_eq!(fs::read_to_string(tight.join("last-fault.txt")).unwrap(), "quota");
        let _ = fs::remove_dir_all(&tight);
    }

    #[test]
    fn single_frame_reopen_stays_admitted() {
        let dir = scratch();
        let mut wal = DiskWal::open(&dir, WalConfig { max_bytes: 1_048_576 }).unwrap();
        let spool = wal.spool_id();
        wal.record_not_admitted("shop", "oo_r_clerk").unwrap();
        drop(wal);
        let view = inspect(&dir).unwrap();
        assert_eq!(view.spool_id, spool);
        assert_eq!(view.requests.len(), 1);
        assert_eq!(view.requests[0].state, "not_admitted");
        assert!(!view.requests.iter().any(|item| item.state == "succeeded"));
        let again = DiskWal::open(&dir, WalConfig { max_bytes: 1_048_576 }).unwrap();
        assert_eq!(again.spool_id(), spool);
        drop(again);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_fault_rolls_back_the_frame() {
        let dir = scratch();
        let mut wal = DiskWal::open(&dir, WalConfig { max_bytes: 1_048_576 }).unwrap();
        wal.fail_next_syncs(1);
        let err = wal.record_not_admitted("shop", "oo_r_clerk");
        assert!(err.is_err());
        drop(wal);
        let text = fs::read_to_string(dir.join("last-fault.txt")).unwrap();
        assert_eq!(text, "sync");
        assert!(inspect(&dir).unwrap().frames.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refusal_bytes_have_no_sql() {
        let dir = scratch();
        let mut wal = DiskWal::open(&dir, WalConfig { max_bytes: 1_048_576 }).unwrap();
        wal.record_not_admitted("shop", "oo_r_clerk").unwrap();
        drop(wal);
        let bytes = fs::read(dir.join("seg-00000001.wal")).unwrap();
        assert!(!bytes.windows(6).any(|window| window == b"SELECT"));
        let view = inspect(&dir).unwrap();
        assert_eq!(view.requests[0].state, "not_admitted");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ack_hash_is_immutable_and_gc_skips_partial() {
        let dir = scratch();
        let mut wal = DiskWal::open(&dir, WalConfig { max_bytes: 1_048_576 }).unwrap();
        wal.record_not_admitted("shop", "oo_r_clerk").unwrap();
        wal.record_not_admitted("shop", "oo_r_reader").unwrap();
        wal.seal_active_segment().unwrap();
        let first = wal.stored[0].request;
        let hash = wal.stored[0].hash;
        wal.checkpoint_ack(first, 1, hash).unwrap();
        let mut other = hash;
        other[0] ^= 0xff;
        assert!(wal.checkpoint_ack(first, 1, other).is_err());
        assert_eq!(wal.gc_acknowledged().unwrap(), 0);
        let second = wal.stored[1].request;
        let second_hash = wal.stored[1].hash;
        wal.checkpoint_ack(second, 1, second_hash).unwrap();
        assert_eq!(wal.gc_acknowledged().unwrap(), 1);
        assert!(!dir.join("seg-00000001.wal").exists());
        assert!(dir.join("frontier.chk").exists());
        drop(wal);
        let mut again = DiskWal::open(&dir, WalConfig { max_bytes: 1_048_576 }).unwrap();
        again.record_not_admitted("shop", "oo_r_clerk").unwrap();
        drop(again);
        assert!(inspect(&dir).unwrap().requests.iter().any(|item| item.state == "not_admitted"));
        let _ = fs::remove_dir_all(&dir);
    }
}
