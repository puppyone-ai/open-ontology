//! Atomic quarantine directory. `Drop` does not seal, repair, or delete.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::model::{
    identity_preimage, is_sorted_unique, receipt_bytes, store_bytes, Binding, ControlEvent,
    FaultCode, Limits, Manifest, Status, MAX_EVENTS, MAX_PAYLOAD_BYTES,
};
use crate::pathutil::{
    check_store_path, create_dir_new, create_write, file_len, kind, read_limited, rename_absent,
    sync_dir, sync_file,
};
use crate::sha256::{hex, sha256};

const MANIFEST_MAX: u64 = 65_536;
const STORE_MAX: u64 = 1_024;
const RECEIPT_MAX: u64 = 256;
const SCAN_SLACK: usize = 8;

struct Seen {
    binding: Option<Binding>,
    binding_torn: bool,
    durable: Vec<Manifest>,
    hints: BTreeSet<String>,
    names: BTreeSet<String>,
    incomplete_events: u64,
    unknown_incomplete: bool,
    foreign: bool,
    events_dir: bool,
    disk: u64,
}

pub fn put(root: &Path, event: &ControlEvent, limits: &Limits) -> Status {
    if let Err(code) = check_store_path(root) {
        return Status::fault(code, Some(event));
    }
    match kind(root) {
        Err(code) => Status::fault(code, Some(event)),
        Ok(None) => admit_new(root, event, limits),
        Ok(Some(kind)) if kind.is_symlink() || !kind.is_dir() => {
            Status::fault(FaultCode::PathRefused, Some(event))
        }
        Ok(Some(_)) => match scan(root) {
            Err(code) => Status::fault(code, Some(event)),
            Ok(seen) => admit_existing(root, event, limits, seen),
        },
    }
}

pub fn replay(root: &Path, event: &ControlEvent) -> Status {
    if let Err(code) = check_store_path(root) {
        return Status::fault(code, Some(event));
    }
    match kind(root) {
        Err(code) => Status::fault(code, Some(event)),
        Ok(None) => Status::fault(FaultCode::NotFound, Some(event)),
        Ok(Some(kind)) if kind.is_symlink() || !kind.is_dir() => {
            Status::fault(FaultCode::PathRefused, Some(event))
        }
        Ok(Some(_)) => match scan(root) {
            Err(code) => Status::fault(code, Some(event)),
            Ok(seen) => {
                if seen.foreign {
                    return refuse(FaultCode::ForeignDirectory, event, &seen);
                }
                if seen.binding_torn || seen.incomplete_events > 0 || seen.unknown_incomplete {
                    return refuse(FaultCode::Incomplete, event, &seen);
                }
                match_existing(root, event, &seen)
            }
        },
    }
}

pub fn recover(root: &Path) -> Status {
    if let Err(code) = check_store_path(root) {
        return Status::fault(code, None);
    }
    match scan(root) {
        Err(code) => Status::fault(code, None),
        Ok(seen) => status_from_seen(&seen),
    }
}

fn admit_new(root: &Path, event: &ControlEvent, limits: &Limits) -> Status {
    let paused = vec![event.collection.clone()];
    let prepared = match prepare(event, &paused, 0, true, limits, 1) {
        Ok(prepared) => prepared,
        Err(code) => return Status::fault(code, Some(event)),
    };
    if let Err(code) = create_dir_new(root) {
        return Status::fault(code, Some(event));
    }
    if let Err(code) = create_dir_new(&root.join("events")) {
        return fail_after_write(root, event, code);
    }
    match seal(root, event, &prepared, true) {
        Ok(()) => success(event, &paused, 1, 1),
        Err(code) => fail_after_write(root, event, code),
    }
}

fn admit_existing(root: &Path, event: &ControlEvent, limits: &Limits, seen: Seen) -> Status {
    if seen.foreign {
        return refuse(FaultCode::ForeignDirectory, event, &seen);
    }
    if seen.binding_torn || seen.incomplete_events > 0 || seen.unknown_incomplete {
        return refuse(FaultCode::Incomplete, event, &seen);
    }
    let name = match dir_name(event) {
        Ok(name) => name,
        Err(code) => return Status::fault(code, Some(event)),
    };
    if seen.names.contains(&name) {
        return match_existing(root, event, &seen);
    }
    let Some(binding) = seen.binding.as_ref() else {
        return refuse(FaultCode::Incomplete, event, &seen);
    };
    if !binding.matches_event(event) {
        return refuse(FaultCode::BindingConflict, event, &seen);
    }
    let mut paused = seen
        .durable
        .iter()
        .map(|manifest| manifest.collection.clone())
        .collect::<BTreeSet<_>>();
    paused.insert(event.collection.clone());
    if paused.len() > crate::model::MAX_PAUSED {
        return refuse(FaultCode::RecordCap, event, &seen);
    }
    let paused: Vec<String> = paused.into_iter().collect();
    let next_events = u64::try_from(seen.names.len()).unwrap_or(u64::MAX).saturating_add(1);
    let prepared = match prepare(event, &paused, seen.disk, false, limits, next_events) {
        Ok(prepared) => prepared,
        Err(code) => return refuse(code, event, &seen),
    };
    let durable_after = u64::try_from(seen.durable.len()).unwrap_or(u64::MAX).saturating_add(1);
    match seal(root, event, &prepared, false) {
        Ok(()) => success(event, &paused, next_events, durable_after),
        Err(code) => fail_after_write(root, event, code),
    }
}

struct Prepared {
    manifest: Vec<u8>,
    receipt: Vec<u8>,
    store: Vec<u8>,
}

fn prepare(
    event: &ControlEvent,
    paused: &[String],
    disk: u64,
    include_store: bool,
    limits: &Limits,
    next_events: u64,
) -> Result<Prepared, FaultCode> {
    if next_events > limits.max_events {
        return Err(FaultCode::RecordCap);
    }
    let manifest = Manifest::build(event, paused)?;
    let manifest_sha = digest(&manifest)?;
    let receipt = receipt_bytes(&event.payload_sha256, &manifest_sha);
    let store = store_bytes(event);
    let mut add = u64::try_from(event.payload.len())
        .ok()
        .and_then(|len| len.checked_add(u64::try_from(manifest.len()).ok()?))
        .and_then(|len| len.checked_add(u64::try_from(receipt.len()).ok()?))
        .ok_or(FaultCode::DiskQuota)?;
    if include_store {
        add = add
            .checked_add(u64::try_from(store.len()).ok().ok_or(FaultCode::DiskQuota)?)
            .ok_or(FaultCode::DiskQuota)?;
    }
    if disk.checked_add(add).ok_or(FaultCode::DiskQuota)? > limits.max_disk_bytes {
        return Err(FaultCode::DiskQuota);
    }
    Ok(Prepared {
        manifest,
        receipt,
        store,
    })
}

fn seal(root: &Path, event: &ControlEvent, prepared: &Prepared, write_store: bool) -> Result<(), FaultCode> {
    let dir = event_dir(root, event)?;
    create_dir_new(&dir)?;
    let payload_partial = dir.join("payload.partial");
    let payload_file = create_write(&payload_partial, &event.payload)?;
    checkpoint("before_data_fsync", root)?;
    sync_file(&payload_file)?;
    drop(payload_file);
    let reread = read_limited(&payload_partial, MAX_PAYLOAD_BYTES)?;
    if reread != event.payload {
        return Err(FaultCode::Io);
    }
    checkpoint("before_manifest", root)?;
    let manifest_partial = dir.join("manifest.json.partial");
    let manifest_file = create_write(&manifest_partial, &prepared.manifest)?;
    sync_file(&manifest_file)?;
    drop(manifest_file);
    let store_partial = root.join("STORE.partial");
    if write_store {
        let store_file = create_write(&store_partial, &prepared.store)?;
        sync_file(&store_file)?;
        drop(store_file);
    }
    checkpoint("before_rename", root)?;
    rename_absent(&payload_partial, &dir.join("payload"))?;
    rename_absent(&manifest_partial, &dir.join("manifest.json"))?;
    if write_store {
        rename_absent(&store_partial, &root.join("STORE"))?;
    }
    checkpoint("before_parent_fsync", root)?;
    sync_dir(&dir)?;
    sync_dir(&root.join("events"))?;
    sync_dir(root)?;
    let receipt_file = create_write(&dir.join("RECEIPT"), &prepared.receipt)?;
    sync_file(&receipt_file)?;
    drop(receipt_file);
    sync_dir(&dir)?;
    sync_dir(&root.join("events"))?;
    sync_dir(root)?;
    Ok(())
}

fn success(event: &ControlEvent, paused: &[String], events: u64, durable_events: u64) -> Status {
    let mut status = Status::base();
    status.fill_event(event);
    status.durable_quarantined = true;
    status.paused = true;
    status.paused_collections = paused.to_vec();
    status.events = events;
    status.durable_events = durable_events;
    status.finish_flags();
    status
}

fn fail_after_write(root: &Path, event: &ControlEvent, code: FaultCode) -> Status {
    let mut status = Status::fault(code, Some(event));
    status.incomplete = true;
    status.paused = true;
    status.durable_quarantined = false;
    if let Ok(seen) = scan(root) {
        copy_counts(&mut status, &seen);
        status.incomplete = true;
        status.paused = true;
        status.durable_quarantined = false;
    }
    status.finish_flags();
    status
}

fn refuse(code: FaultCode, event: &ControlEvent, seen: &Seen) -> Status {
    let mut status = Status::fault(code, Some(event));
    copy_counts(&mut status, seen);
    status.durable_quarantined = false;
    status.replay = false;
    if seen.incomplete_events > 0 || seen.unknown_incomplete || seen.binding_torn {
        status.incomplete = true;
        status.paused = true;
    }
    if !seen.durable.is_empty() || !seen.hints.is_empty() {
        status.paused = true;
    }
    status.finish_flags();
    status
}

fn match_existing(root: &Path, event: &ControlEvent, seen: &Seen) -> Status {
    let Ok(name) = dir_name(event) else {
        return Status::fault(FaultCode::Io, Some(event));
    };
    if !seen.names.contains(&name) {
        return refuse(FaultCode::NotFound, event, seen);
    }
    let Some(manifest) = seen
        .durable
        .iter()
        .find(|item| manifest_dir(item).as_deref() == Some(name.as_str()))
    else {
        return refuse(FaultCode::Incomplete, event, seen);
    };
    let stored = match stored_payload(root, event) {
        Ok(bytes) => bytes,
        Err(_) => return refuse(FaultCode::Incomplete, event, seen),
    };
    if stored == event.payload && same_body(event, manifest) {
        let mut status = Status::base();
        status.replay = true;
        status.durable_quarantined = true;
        status.paused = true;
        status.fill_event(event);
        copy_counts(&mut status, seen);
        status.incomplete = false;
        status.finish_flags();
        status
    } else {
        refuse(FaultCode::PayloadConflict, event, seen)
    }
}

fn stored_payload(root: &Path, event: &ControlEvent) -> Result<Vec<u8>, FaultCode> {
    let path = event_dir(root, event)?.join("payload");
    read_limited(&path, MAX_PAYLOAD_BYTES)
}

fn same_body(event: &ControlEvent, manifest: &Manifest) -> bool {
    manifest.source == event.source
        && manifest.lineage == event.lineage
        && manifest.epoch == event.epoch
        && manifest.event_id == event.event_id
        && manifest.sequence == event.sequence
        && manifest.collection == event.collection
        && manifest.reason == event.reason.as_str()
        && manifest.payload_sha256 == event.payload_sha256
        && manifest.source_position == event.source_position
        && manifest.payload_bytes == u64::try_from(event.payload.len()).unwrap_or(u64::MAX)
}

fn status_from_seen(seen: &Seen) -> Status {
    let mut status = Status::base();
    copy_counts(&mut status, seen);
    let torn = seen.binding_torn || seen.incomplete_events > 0 || seen.unknown_incomplete;
    if seen.foreign && !seen.events_dir && seen.binding.is_none() && !seen.binding_torn {
        status.fault = Some(FaultCode::ForeignDirectory.as_str());
        status.paused = false;
        status.incomplete = false;
        status.finish_flags();
        return status;
    }
    if seen.foreign {
        status.fault = Some(FaultCode::ForeignDirectory.as_str());
        status.incomplete = true;
        status.paused = true;
        status.durable_quarantined = false;
        status.finish_flags();
        return status;
    }
    if seen.names.is_empty() && seen.binding.is_none() && !seen.binding_torn && !seen.events_dir {
        status.fault = Some(FaultCode::ForeignDirectory.as_str());
        status.finish_flags();
        return status;
    }
    if torn || seen.binding.is_none() {
        status.incomplete = true;
        status.paused = true;
        status.durable_quarantined = false;
        status.finish_flags();
        return status;
    }
    status.durable_quarantined = !seen.durable.is_empty();
    status.paused = !seen.durable.is_empty();
    status.incomplete = false;
    if seen.durable.len() == 1 {
        fill_manifest(&mut status, &seen.durable[0]);
    }
    status.finish_flags();
    status
}

fn copy_counts(status: &mut Status, seen: &Seen) {
    status.events = u64::try_from(seen.names.len()).unwrap_or(u64::MAX);
    status.durable_events = u64::try_from(seen.durable.len()).unwrap_or(u64::MAX);
    status.incomplete_events = seen.incomplete_events;
    status.unknown_incomplete = seen.unknown_incomplete;
    let mut paused = seen.hints.clone();
    for manifest in &seen.durable {
        paused.insert(manifest.collection.clone());
        for name in &manifest.paused_collections {
            paused.insert(name.clone());
        }
    }
    status.paused_collections = paused.into_iter().collect();
}

fn fill_manifest(status: &mut Status, manifest: &Manifest) {
    status.source = Some(manifest.source.clone());
    status.lineage = Some(manifest.lineage.clone());
    status.epoch = Some(manifest.epoch);
    status.event_id = Some(manifest.event_id.clone());
    status.sequence = Some(manifest.sequence);
    status.collection = Some(manifest.collection.clone());
    status.reason = Some(manifest.reason.clone());
    status.payload_sha256 = Some(manifest.payload_sha256.clone());
    status.source_position = Some(manifest.source_position.clone());
    status.attempt_collection = Some(manifest.collection.clone());
}

fn scan(root: &Path) -> Result<Seen, FaultCode> {
    let Some(root_kind) = kind(root)? else {
        return Err(FaultCode::NotFound);
    };
    if root_kind.is_symlink() || !root_kind.is_dir() {
        return Err(FaultCode::PathRefused);
    }
    let mut seen = Seen {
        binding: None,
        binding_torn: false,
        durable: Vec::new(),
        hints: BTreeSet::new(),
        names: BTreeSet::new(),
        incomplete_events: 0,
        unknown_incomplete: false,
        foreign: false,
        events_dir: false,
        disk: 0,
    };
    let mut entries = Vec::new();
    for entry in fs::read_dir(root).map_err(|_| FaultCode::Io)? {
        entries.push(entry.map_err(|_| FaultCode::Io)?);
        if entries.len() > MAX_EVENTS as usize + SCAN_SLACK {
            seen.unknown_incomplete = true;
            seen.foreign = true;
            return Ok(seen);
        }
    }
    for entry in &entries {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            seen.foreign = true;
            continue;
        };
        let file_type = entry.file_type().map_err(|_| FaultCode::Io)?;
        if file_type.is_symlink() {
            seen.foreign = true;
            seen.unknown_incomplete = true;
            continue;
        }
        if name == "STORE" && file_type.is_file() {
            read_store(&entry.path(), &mut seen)?;
        } else if name == "STORE.partial" && file_type.is_file() {
            seen.binding_torn = true;
            seen.disk = seen.disk.saturating_add(file_len(&entry.path())?);
        }
    }
    for entry in &entries {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let file_type = entry.file_type().map_err(|_| FaultCode::Io)?;
        if file_type.is_symlink() {
            continue;
        }
        if name == "events" && file_type.is_dir() {
            seen.events_dir = true;
            scan_events(&entry.path(), &mut seen)?;
        } else if name != "STORE" && name != "STORE.partial" {
            seen.foreign = true;
            if file_type.is_file() {
                seen.disk = seen.disk.saturating_add(file_len(&entry.path())?);
            }
        }
    }
    if seen.binding_torn {
        seen.incomplete_events = seen
            .incomplete_events
            .saturating_add(u64::try_from(seen.durable.len()).unwrap_or(u64::MAX));
        seen.durable.clear();
    }
    Ok(seen)
}

fn read_store(path: &Path, seen: &mut Seen) -> Result<(), FaultCode> {
    let bytes = match read_limited(path, STORE_MAX) {
        Ok(bytes) => bytes,
        Err(FaultCode::PayloadTooLarge | FaultCode::PathRefused) => {
            seen.binding_torn = true;
            return Ok(());
        }
        Err(code) => return Err(code),
    };
    seen.disk = seen.disk.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
    match Binding::parse(&bytes) {
        Some(binding) => seen.binding = Some(binding),
        None => seen.binding_torn = true,
    }
    Ok(())
}

fn scan_events(path: &Path, seen: &mut Seen) -> Result<(), FaultCode> {
    let mut count = 0usize;
    for entry in fs::read_dir(path).map_err(|_| FaultCode::Io)? {
        count += 1;
        if count > MAX_EVENTS as usize + SCAN_SLACK {
            seen.unknown_incomplete = true;
            seen.incomplete_events = seen.incomplete_events.saturating_add(1);
            break;
        }
        let entry = entry.map_err(|_| FaultCode::Io)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            seen.foreign = true;
            seen.unknown_incomplete = true;
            continue;
        };
        let child = entry.path();
        let file_type = entry.file_type().map_err(|_| FaultCode::Io)?;
        if file_type.is_symlink() || !file_type.is_dir() || !is_hex_name(&name) {
            seen.foreign = true;
            seen.unknown_incomplete = true;
            seen.incomplete_events = seen.incomplete_events.saturating_add(1);
            continue;
        }
        seen.names.insert(name.clone());
        classify_event(&child, &name, seen)?;
    }
    Ok(())
}

fn classify_event(dir: &Path, name: &str, seen: &mut Seen) -> Result<(), FaultCode> {
    let mut payload = false;
    let mut manifest = false;
    let mut receipt = false;
    let mut partial = false;
    let mut extra = false;
    let mut symlink = false;
    let mut count = 0usize;
    for entry in fs::read_dir(dir).map_err(|_| FaultCode::Io)? {
        count += 1;
        if count > 16 {
            extra = true;
            break;
        }
        let entry = entry.map_err(|_| FaultCode::Io)?;
        let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
            extra = true;
            continue;
        };
        let path = entry.path();
        let file_type = entry.file_type().map_err(|_| FaultCode::Io)?;
        if file_type.is_symlink() {
            symlink = true;
            continue;
        }
        if file_type.is_file() {
            match file_len(&path) {
                Ok(len) => seen.disk = seen.disk.saturating_add(len),
                Err(FaultCode::PathRefused) => symlink = true,
                Err(code) => return Err(code),
            }
        }
        match file_name.as_str() {
            "payload" if file_type.is_file() => payload = true,
            "payload.partial" if file_type.is_file() => partial = true,
            "manifest.json" if file_type.is_file() => manifest = true,
            "manifest.json.partial" if file_type.is_file() => partial = true,
            "RECEIPT" if file_type.is_file() => receipt = true,
            _ => extra = true,
        }
    }
    if symlink || extra || partial || !(payload && manifest && receipt) {
        seen.incomplete_events = seen.incomplete_events.saturating_add(1);
        if !absorb_hint(&dir.join("manifest.json"), &mut seen.hints)
            && !absorb_hint(&dir.join("manifest.json.partial"), &mut seen.hints)
        {
            seen.unknown_incomplete = true;
        }
        return Ok(());
    }
    match load_durable(dir, name, seen.binding.as_ref()) {
        Ok(Some(manifest)) => {
            seen.hints.insert(manifest.collection.clone());
            seen.durable.push(manifest);
        }
        Ok(None) => {
            seen.incomplete_events = seen.incomplete_events.saturating_add(1);
            if !absorb_hint(&dir.join("manifest.json"), &mut seen.hints) {
                seen.unknown_incomplete = true;
            }
        }
        Err(code) => return Err(code),
    }
    Ok(())
}

fn load_durable(dir: &Path, name: &str, binding: Option<&Binding>) -> Result<Option<Manifest>, FaultCode> {
    let Some(binding) = binding else {
        return Ok(None);
    };
    let payload = match read_limited(&dir.join("payload"), MAX_PAYLOAD_BYTES) {
        Ok(bytes) => bytes,
        Err(FaultCode::PayloadTooLarge | FaultCode::PathRefused) => return Ok(None),
        Err(code) => return Err(code),
    };
    let manifest_bytes = match read_limited(&dir.join("manifest.json"), MANIFEST_MAX) {
        Ok(bytes) => bytes,
        Err(FaultCode::PayloadTooLarge | FaultCode::PathRefused) => return Ok(None),
        Err(code) => return Err(code),
    };
    let receipt = match read_limited(&dir.join("RECEIPT"), RECEIPT_MAX) {
        Ok(bytes) => bytes,
        Err(FaultCode::PayloadTooLarge | FaultCode::PathRefused) => return Ok(None),
        Err(code) => return Err(code),
    };
    let Some(manifest) = parse_manifest_strict(&manifest_bytes) else {
        return Ok(None);
    };
    if !binding.matches_manifest(&manifest) {
        return Ok(None);
    }
    let preimage = format!(
        "{{\"epoch\":{},\"event_id\":\"{}\",\"lineage\":\"{}\",\"source\":\"{}\"}}",
        manifest.epoch, manifest.event_id, manifest.lineage, manifest.source
    );
    let expected = digest(preimage.as_bytes())?;
    if expected != name {
        return Ok(None);
    }
    let payload_sha = digest(&payload)?;
    if payload_sha != manifest.payload_sha256
        || u64::try_from(payload.len()).ok() != Some(manifest.payload_bytes)
    {
        return Ok(None);
    }
    let manifest_sha = digest(&manifest_bytes)?;
    if receipt != receipt_bytes(&payload_sha, &manifest_sha) {
        return Ok(None);
    }
    Ok(Some(manifest))
}

fn absorb_hint(path: &Path, hints: &mut BTreeSet<String>) -> bool {
    let Ok(bytes) = read_limited(path, MANIFEST_MAX) else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return false;
    };
    let body = text.strip_suffix('\n').unwrap_or(text);
    let Ok(manifest) = serde_json::from_str::<Manifest>(body) else {
        return false;
    };
    let mut any = false;
    if crate::model::valid_collection(&manifest.collection) {
        hints.insert(manifest.collection);
        any = true;
    }
    for name in manifest.paused_collections {
        if crate::model::valid_collection(&name) {
            hints.insert(name);
            any = true;
        }
    }
    any
}

fn parse_manifest_strict(bytes: &[u8]) -> Option<Manifest> {
    let text = std::str::from_utf8(bytes).ok()?;
    let body = text.strip_suffix('\n')?;
    if body.contains('\n') {
        return None;
    }
    let manifest: Manifest = serde_json::from_str(body).ok()?;
    if !manifest.flags_ok() {
        return None;
    }
    let mut again = serde_json::to_vec(&manifest).ok()?;
    again.push(b'\n');
    if again != bytes {
        return None;
    }
    if !is_sorted_unique(&manifest.paused_collections) {
        return None;
    }
    Some(manifest)
}

fn manifest_dir(manifest: &Manifest) -> Option<String> {
    let preimage = format!(
        "{{\"epoch\":{},\"event_id\":\"{}\",\"lineage\":\"{}\",\"source\":\"{}\"}}",
        manifest.epoch, manifest.event_id, manifest.lineage, manifest.source
    );
    Some(hex(&sha256(preimage.as_bytes())?))
}

fn dir_name(event: &ControlEvent) -> Result<String, FaultCode> {
    digest(identity_preimage(event).as_bytes())
}

fn event_dir(root: &Path, event: &ControlEvent) -> Result<PathBuf, FaultCode> {
    Ok(root.join("events").join(dir_name(event)?))
}

fn digest(bytes: &[u8]) -> Result<String, FaultCode> {
    Ok(hex(&sha256(bytes).ok_or(FaultCode::Io)?))
}

fn is_hex_name(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn checkpoint(stage: &str, root: &Path) -> Result<(), FaultCode> {
    let Ok(wanted) = std::env::var("CDC_QUARANTINE_CHECKPOINT") else {
        return Ok(());
    };
    if wanted != stage {
        return Ok(());
    }
    let mark = std::env::var("CDC_QUARANTINE_MARK").map_err(|_| FaultCode::Io)?;
    let mark_path = Path::new(&mark);
    if mark.is_empty() || mark_path.starts_with(root) {
        return Err(FaultCode::Io);
    }
    let mut file = File::create_new(mark_path).map_err(|_| FaultCode::Io)?;
    file.write_all(b"waiting\n").map_err(|_| FaultCode::Io)?;
    file.sync_all().map_err(|_| FaultCode::Io)?;
    loop {
        thread::sleep(Duration::from_millis(50));
    }
}
