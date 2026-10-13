//! Inspect-compatible audit segment.
//!
//! `Admission` and `CompletionRecord` have no public constructor, and
//! `DiskWal::admit` is not callable from this crate. `DiskWal::open` still
//! allocates `spool.id`. This writer then appends the same frame bytes
//! `gateway_audit_wal::inspect` decodes. It is not a shared-catalog change.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use gateway_audit_wal::DiskWal;
use sha2::{Digest, Sha256};

use crate::output::Fail;

const FRAME_MAGIC: &[u8; 4] = b"OAW1";
const SEGMENT_MAGIC: &[u8; 8] = b"OAWSEG1\0";
const VERSION: u16 = 1;
const MAX_PAYLOAD: usize = 16 * 1024;

pub(crate) struct AuditMeta {
    pub(crate) original: String,
    pub(crate) executed: String,
    pub(crate) fingerprint: String,
    pub(crate) role: String,
    pub(crate) project: String,
    pub(crate) policy: String,
    pub(crate) backend_pid: i32,
    pub(crate) backend_start: String,
}

pub(crate) struct OpenAudit {
    root: PathBuf,
    spool: [u8; 16],
    request: [u8; 16],
}

pub(crate) struct Completion<'a> {
    pub(crate) status: &'a str,
    pub(crate) truncated: bool,
    pub(crate) returned_rows: u32,
    pub(crate) result_bytes: u64,
    pub(crate) rollback: bool,
    pub(crate) socket: bool,
    pub(crate) stop: &'a str,
    pub(crate) sqlstate: &'a str,
}

pub(crate) fn begin(root: &Path, max_bytes: u64, meta: &AuditMeta) -> Result<OpenAudit, Fail> {
    if meta.backend_pid <= 0 || !ident_ok(&meta.role) || !project_ok(&meta.project) || !policy_ok(&meta.policy)
    {
        return Err(Fail::Audit);
    }
    if !sha256_hex(&meta.original) || !sha256_hex(&meta.executed) || !fingerprint_ok(&meta.fingerprint) {
        return Err(Fail::Audit);
    }
    if !backend_start_ok(&meta.backend_start) || !sqlstate_ok("") {
        return Err(Fail::Audit);
    }
    let wal = DiskWal::open(
        root,
        gateway_audit_wal::WalConfig { max_bytes },
    )
    .map_err(|_| Fail::Audit)?;
    let spool = fs::read(root.join("spool.id")).map_err(|_| Fail::Audit)?;
    drop(wal);
    if spool.len() != 16 {
        return Err(Fail::Audit);
    }
    let mut spool_id = [0_u8; 16];
    spool_id.copy_from_slice(&spool);
    let mut request = [0_u8; 16];
    getrandom::getrandom(&mut request).map_err(|_| Fail::Audit)?;
    request[6] = (request[6] & 0x0f) | 0x40;
    request[8] = (request[8] & 0x3f) | 0x80;
    let segment = root.join("seg-00000001.wal");
    // DiskWal::open allocates segment 1 as a header. Append the admission
    // there. A segment that already has frames is not ours to rewrite.
    let existing = fs::read(&segment).map_err(|_| Fail::Audit)?;
    if existing != segment_header(1) {
        return Err(Fail::Audit);
    }
    let payload = admission_payload(meta)?;
    let frame = encode_frame(1, &spool_id, &request, 1, &payload)?;
    let mut bytes = existing;
    bytes.extend_from_slice(&frame);
    write_segment(&segment, &bytes)?;
    sync_dir(root)?;
    Ok(OpenAudit {
        root: root.to_path_buf(),
        spool: spool_id,
        request,
    })
}

pub(crate) fn complete(audit: &OpenAudit, meta: &AuditMeta, completion: &Completion<'_>) -> Result<(), Fail> {
    if !matches!(completion.status, "succeeded" | "partial" | "failed") || !sqlstate_ok(completion.sqlstate) {
        return Err(Fail::Audit);
    }
    if !completion.stop.is_empty()
        && !matches!(
            completion.stop,
            "timeout" | "canceled" | "type_refused" | "database" | "protocol"
        )
    {
        return Err(Fail::Audit);
    }
    let payload = completion_payload(meta, completion)?;
    let frame = encode_frame(2, &audit.spool, &audit.request, 2, &payload)?;
    let path = audit.root.join("seg-00000001.wal");
    let mut bytes = fs::read(&path).map_err(|_| Fail::Audit)?;
    bytes.extend_from_slice(&frame);
    write_segment(&path, &bytes)?;
    sync_dir(&audit.root)?;
    Ok(())
}

fn admission_payload(meta: &AuditMeta) -> Result<Vec<u8>, Fail> {
    let mut out = Vec::new();
    push(&mut out, 1, b"admission")?;
    push(&mut out, 2, meta.original.as_bytes())?;
    push(&mut out, 3, meta.executed.as_bytes())?;
    push(&mut out, 4, meta.fingerprint.as_bytes())?;
    push(&mut out, 5, meta.role.as_bytes())?;
    push(&mut out, 6, meta.project.as_bytes())?;
    push(&mut out, 7, meta.policy.as_bytes())?;
    push(&mut out, 8, meta.backend_pid.to_string().as_bytes())?;
    push(&mut out, 9, meta.backend_start.as_bytes())?;
    Ok(out)
}

fn completion_payload(meta: &AuditMeta, completion: &Completion<'_>) -> Result<Vec<u8>, Fail> {
    let mut out = Vec::new();
    push(&mut out, 1, b"completion")?;
    push(&mut out, 2, meta.original.as_bytes())?;
    push(&mut out, 3, meta.executed.as_bytes())?;
    push(&mut out, 4, meta.fingerprint.as_bytes())?;
    push(&mut out, 5, meta.role.as_bytes())?;
    push(&mut out, 6, meta.project.as_bytes())?;
    push(&mut out, 7, meta.policy.as_bytes())?;
    push(&mut out, 8, meta.backend_pid.to_string().as_bytes())?;
    push(&mut out, 9, meta.backend_start.as_bytes())?;
    push(&mut out, 10, completion.status.as_bytes())?;
    push(&mut out, 11, if completion.truncated { b"1" } else { b"0" })?;
    push(&mut out, 12, completion.returned_rows.to_string().as_bytes())?;
    push(&mut out, 13, completion.result_bytes.to_string().as_bytes())?;
    push(&mut out, 14, if completion.rollback { b"1" } else { b"0" })?;
    push(&mut out, 15, if completion.socket { b"1" } else { b"0" })?;
    push(&mut out, 16, completion.stop.as_bytes())?;
    push(&mut out, 17, completion.sqlstate.as_bytes())?;
    Ok(out)
}

fn push(out: &mut Vec<u8>, tag: u16, bytes: &[u8]) -> Result<(), Fail> {
    if bytes.len() > 512 || out.len() + bytes.len() + 6 > MAX_PAYLOAD {
        return Err(Fail::Audit);
    }
    out.extend_from_slice(&tag.to_be_bytes());
    let len = u32::try_from(bytes.len()).map_err(|_| Fail::Audit)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn encode_frame(kind: u8, spool: &[u8; 16], request: &[u8; 16], seq: u64, payload: &[u8]) -> Result<Vec<u8>, Fail> {
    if payload.len() > MAX_PAYLOAD || seq == 0 {
        return Err(Fail::Audit);
    }
    let mut out = Vec::with_capacity(83 + payload.len());
    out.extend_from_slice(FRAME_MAGIC);
    out.extend_from_slice(&VERSION.to_be_bytes());
    out.push(kind);
    out.extend_from_slice(spool);
    out.extend_from_slice(request);
    out.extend_from_slice(&seq.to_be_bytes());
    let len = u32::try_from(payload.len()).map_err(|_| Fail::Audit)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    out.extend_from_slice(&Sha256::digest(&out));
    Ok(out)
}

fn segment_header(id: u32) -> [u8; 14] {
    let mut out = [0_u8; 14];
    out[..8].copy_from_slice(SEGMENT_MAGIC);
    out[8..10].copy_from_slice(&VERSION.to_be_bytes());
    out[10..14].copy_from_slice(&id.to_be_bytes());
    out
}

fn write_segment(path: &Path, bytes: &[u8]) -> Result<(), Fail> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|_| Fail::Audit)?;
    file.write_all(bytes).map_err(|_| Fail::Audit)?;
    file.sync_all().map_err(|_| Fail::Audit)?;
    Ok(())
}

fn sync_dir(path: &Path) -> Result<(), Fail> {
    let dir = File::open(path).map_err(|_| Fail::Audit)?;
    dir.sync_all().map_err(|_| Fail::Audit)?;
    Ok(())
}

fn sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn fingerprint_ok(text: &str) -> bool {
    (8..=128).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn ident_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn project_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

fn policy_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

fn backend_start_ok(text: &str) -> bool {
    !text.is_empty() && text.len() <= 32 && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn sqlstate_ok(text: &str) -> bool {
    text.is_empty()
        || (text.len() == 5 && text.bytes().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use gateway_audit_wal::{inspect, DiskWal, WalConfig};

    use super::{begin, complete, AuditMeta, Completion};

    #[test]
    fn inspect_reads_a_consumer_admission_and_completion() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("oo-dl01w1-21-wal-{nanos}"));
        let _ = fs::remove_dir_all(&root);
        let meta = AuditMeta {
            original: "ab".repeat(32),
            executed: "cd".repeat(32),
            fingerprint: "0123456789abcdef".to_string(),
            role: "oo_r_reader".to_string(),
            project: "oo_dl01w1_44".to_string(),
            policy: "9109a02440e61d68175e85b01a2a2cd62f7bdc8f97fc9ac0b40a430c2d7ab718".to_string(),
            backend_pid: 42,
            backend_start: "20260315123456000000".to_string(),
        };
        let audit = begin(&root, 1_048_576, &meta).expect("begin");
        complete(
            &audit,
            &meta,
            &Completion {
                status: "succeeded",
                truncated: false,
                returned_rows: 2,
                result_bytes: 16,
                rollback: true,
                socket: false,
                stop: "",
                sqlstate: "",
            },
        )
        .expect("complete");
        let view = inspect(&root).expect("inspect");
        assert_eq!(view.requests.len(), 1);
        assert_eq!(view.requests[0].state, "succeeded");
        assert_eq!(view.requests[0].project, "oo_dl01w1_44");
        assert_eq!(view.requests[0].role, "oo_r_reader");
        assert_eq!(view.frames.len(), 2);
        assert!(!view.torn_tail);
        let again = DiskWal::open(
            &root,
            WalConfig {
                max_bytes: 1_048_576,
            },
        );
        assert!(again.is_ok());
        drop(again);
        let _ = fs::remove_dir_all(&root);
    }
}
