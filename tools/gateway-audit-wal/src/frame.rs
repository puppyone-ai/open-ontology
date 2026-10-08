//! Length-prefixed frames. The checksum covers the bytes before it.
//! Payload fields are bounded identifiers and hashes. SQL text is not a field.

use sha2::{Digest, Sha256};

use crate::error::WalError;

pub(crate) const FRAME_MAGIC: &[u8; 4] = b"OAW1";
pub(crate) const SEGMENT_MAGIC: &[u8; 8] = b"OAWSEG1\0";
pub(crate) const FOOTER_MAGIC: &[u8; 8] = b"OAWEND01";
pub(crate) const VERSION: u16 = 1;
pub(crate) const MAX_PAYLOAD: usize = 16 * 1024;
pub(crate) const HEADER_LEN: usize = 51;
pub(crate) const CHECKSUM_LEN: usize = 32;
pub(crate) const MIN_FRAME: usize = HEADER_LEN + CHECKSUM_LEN;
pub(crate) const SEGMENT_HEADER_LEN: usize = 14;
pub(crate) const FOOTER_LEN: usize = 44;

const TAG_KIND: u16 = 1;
const TAG_ORIGINAL: u16 = 2;
const TAG_EXECUTED: u16 = 3;
const TAG_FINGERPRINT: u16 = 4;
const TAG_ROLE: u16 = 5;
const TAG_PROJECT: u16 = 6;
const TAG_POLICY: u16 = 7;
const TAG_PID: u16 = 8;
const TAG_BACKEND: u16 = 9;
const TAG_STATUS: u16 = 10;
const TAG_TRUNCATED: u16 = 11;
const TAG_ROWS: u16 = 12;
const TAG_BYTES: u16 = 13;
const TAG_ROLLBACK: u16 = 14;
const TAG_SOCKET: u16 = 15;
const TAG_STOP: u16 = 16;
const TAG_SQLSTATE: u16 = 17;
const TAG_REASON: u16 = 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Admission,
    Completion,
    Refusal,
}

impl Kind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Admission => "admission",
            Self::Completion => "completion",
            Self::Refusal => "refusal",
        }
    }

    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Admission => 1,
            Self::Completion => 2,
            Self::Refusal => 3,
        }
    }

    fn from_code(code: u8) -> Result<Self, WalError> {
        match code {
            1 => Ok(Self::Admission),
            2 => Ok(Self::Completion),
            3 => Ok(Self::Refusal),
            _ => Err(WalError::Corrupt),
        }
    }

    fn from_name(name: &str) -> Result<Self, WalError> {
        match name {
            "admission" => Ok(Self::Admission),
            "completion" => Ok(Self::Completion),
            "refusal" => Ok(Self::Refusal),
            _ => Err(WalError::Corrupt),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Payload {
    pub(crate) kind: Kind,
    pub(crate) original: String,
    pub(crate) executed: String,
    pub(crate) fingerprint: String,
    pub(crate) role: String,
    pub(crate) project: String,
    pub(crate) policy: String,
    pub(crate) backend_pid: i32,
    pub(crate) backend_start: String,
    pub(crate) status: String,
    pub(crate) truncated: bool,
    pub(crate) returned_rows: u32,
    pub(crate) result_bytes: u64,
    pub(crate) rollback: bool,
    pub(crate) socket: bool,
    pub(crate) stop: String,
    pub(crate) sqlstate: String,
    pub(crate) reason: String,
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[(byte >> 4) as usize]));
        out.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    out
}

pub(crate) fn hex_decode(text: &str) -> Result<Vec<u8>, WalError> {
    if !text.len().is_multiple_of(2) || text.len() > 32768 {
        return Err(WalError::Bound);
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 2);
    let mut index = 0;
    while index < bytes.len() {
        let hi = hex_val(bytes[index])?;
        let lo = hex_val(bytes[index + 1])?;
        out.push((hi << 4) | lo);
        index += 2;
    }
    Ok(out)
}

fn hex_val(byte: u8) -> Result<u8, WalError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(WalError::Bound),
    }
}

pub(crate) fn require_sha256_hex(text: &str) -> Result<String, WalError> {
    if text.len() == 64 && text.bytes().all(|byte| hex_val(byte).is_ok()) {
        Ok(text.to_string())
    } else {
        Err(WalError::Bound)
    }
}

pub(crate) fn require_fingerprint(text: &str) -> Result<String, WalError> {
    if (8..=128).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(text.to_ascii_lowercase())
    } else {
        Err(WalError::Bound)
    }
}

pub(crate) fn require_ident(text: &str) -> Result<String, WalError> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || !bytes[0].is_ascii_lowercase() {
        return Err(WalError::Bound);
    }
    if bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        Ok(text.to_string())
    } else {
        Err(WalError::Bound)
    }
}

pub(crate) fn require_project(text: &str) -> Result<String, WalError> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || !bytes[0].is_ascii_lowercase() {
        return Err(WalError::Bound);
    }
    if bytes[1..].iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
    }) {
        Ok(text.to_string())
    } else {
        Err(WalError::Bound)
    }
}

pub(crate) fn require_policy(text: &str) -> Result<String, WalError> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return Err(WalError::Bound);
    }
    if bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
    }) {
        Ok(text.to_string())
    } else {
        Err(WalError::Bound)
    }
}

pub(crate) fn require_backend_start(text: &str) -> Result<String, WalError> {
    if !text.is_empty() && text.len() <= 32 && text.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(text.to_string())
    } else {
        Err(WalError::Bound)
    }
}

pub(crate) fn require_sqlstate(text: &str) -> Result<String, WalError> {
    if text.is_empty() {
        return Ok(String::new());
    }
    if text.len() == 5
        && text
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        Ok(text.to_string())
    } else {
        Err(WalError::Bound)
    }
}

pub(crate) fn segment_header(id: u32) -> [u8; SEGMENT_HEADER_LEN] {
    let mut out = [0u8; SEGMENT_HEADER_LEN];
    out[..8].copy_from_slice(SEGMENT_MAGIC);
    out[8..10].copy_from_slice(&VERSION.to_be_bytes());
    out[10..14].copy_from_slice(&id.to_be_bytes());
    out
}

pub(crate) fn encode_frame(
    kind: Kind,
    spool: &[u8; 16],
    request: &[u8; 16],
    seq: u64,
    payload: &[u8],
) -> Result<Vec<u8>, WalError> {
    if payload.len() > MAX_PAYLOAD || seq == 0 || seq > 1_000_000 {
        return Err(WalError::Bound);
    }
    let mut out = Vec::with_capacity(MIN_FRAME + payload.len());
    out.extend_from_slice(FRAME_MAGIC);
    out.extend_from_slice(&VERSION.to_be_bytes());
    out.push(kind.code());
    out.extend_from_slice(spool);
    out.extend_from_slice(request);
    out.extend_from_slice(&seq.to_be_bytes());
    let len = u32::try_from(payload.len()).map_err(|_| WalError::Bound)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    let sum = sha256(&out);
    out.extend_from_slice(&sum);
    Ok(out)
}

pub(crate) struct DecodedFrame {
    pub(crate) kind: Kind,
    pub(crate) spool: [u8; 16],
    pub(crate) request: [u8; 16],
    pub(crate) seq: u64,
    pub(crate) payload: Vec<u8>,
    pub(crate) payload_hash: [u8; 32],
    pub(crate) len: usize,
}

pub(crate) fn decode_frame(bytes: &[u8]) -> Result<DecodedFrame, WalError> {
    if bytes.len() < MIN_FRAME || &bytes[..4] != FRAME_MAGIC {
        return Err(WalError::Corrupt);
    }
    let version = u16::from_be_bytes([bytes[4], bytes[5]]);
    if version != VERSION {
        return Err(WalError::Corrupt);
    }
    let kind = Kind::from_code(bytes[6])?;
    let mut spool = [0u8; 16];
    spool.copy_from_slice(&bytes[7..23]);
    let mut request = [0u8; 16];
    request.copy_from_slice(&bytes[23..39]);
    let seq = u64::from_be_bytes(bytes[39..47].try_into().map_err(|_| WalError::Corrupt)?);
    let payload_len = u32::from_be_bytes(bytes[47..51].try_into().map_err(|_| WalError::Corrupt)?) as usize;
    if payload_len > MAX_PAYLOAD || seq == 0 || seq > 1_000_000 {
        return Err(WalError::Corrupt);
    }
    let total = HEADER_LEN + payload_len + CHECKSUM_LEN;
    if bytes.len() < total {
        return Err(WalError::Corrupt);
    }
    let payload = bytes[HEADER_LEN..HEADER_LEN + payload_len].to_vec();
    let expected = sha256(&bytes[..HEADER_LEN + payload_len]);
    let actual = &bytes[HEADER_LEN + payload_len..total];
    if actual != expected {
        return Err(WalError::Corrupt);
    }
    Ok(DecodedFrame {
        kind,
        spool,
        request,
        seq,
        payload_hash: sha256(&payload),
        payload,
        len: total,
    })
}

fn push_field(out: &mut Vec<u8>, tag: u16, bytes: &[u8]) -> Result<(), WalError> {
    if bytes.len() > 512 {
        return Err(WalError::Bound);
    }
    out.extend_from_slice(&tag.to_be_bytes());
    let len = u32::try_from(bytes.len()).map_err(|_| WalError::Bound)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

pub(crate) fn encode_payload(payload: &Payload) -> Result<Vec<u8>, WalError> {
    let mut out = Vec::new();
    push_field(&mut out, TAG_KIND, payload.kind.as_str().as_bytes())?;
    match payload.kind {
        Kind::Admission | Kind::Completion => {
            push_field(&mut out, TAG_ORIGINAL, payload.original.as_bytes())?;
            push_field(&mut out, TAG_EXECUTED, payload.executed.as_bytes())?;
            push_field(&mut out, TAG_FINGERPRINT, payload.fingerprint.as_bytes())?;
            push_field(&mut out, TAG_ROLE, payload.role.as_bytes())?;
            push_field(&mut out, TAG_PROJECT, payload.project.as_bytes())?;
            push_field(&mut out, TAG_POLICY, payload.policy.as_bytes())?;
            push_field(&mut out, TAG_PID, payload.backend_pid.to_string().as_bytes())?;
            push_field(&mut out, TAG_BACKEND, payload.backend_start.as_bytes())?;
        }
        Kind::Refusal => {
            push_field(&mut out, TAG_ROLE, payload.role.as_bytes())?;
            push_field(&mut out, TAG_PROJECT, payload.project.as_bytes())?;
            push_field(&mut out, TAG_REASON, payload.reason.as_bytes())?;
        }
    }
    if payload.kind == Kind::Completion {
        push_field(&mut out, TAG_STATUS, payload.status.as_bytes())?;
        push_field(
            &mut out,
            TAG_TRUNCATED,
            if payload.truncated { b"1" } else { b"0" },
        )?;
        push_field(&mut out, TAG_ROWS, payload.returned_rows.to_string().as_bytes())?;
        push_field(&mut out, TAG_BYTES, payload.result_bytes.to_string().as_bytes())?;
        push_field(
            &mut out,
            TAG_ROLLBACK,
            if payload.rollback { b"1" } else { b"0" },
        )?;
        push_field(&mut out, TAG_SOCKET, if payload.socket { b"1" } else { b"0" })?;
        push_field(&mut out, TAG_STOP, payload.stop.as_bytes())?;
        push_field(&mut out, TAG_SQLSTATE, payload.sqlstate.as_bytes())?;
    }
    if out.len() > MAX_PAYLOAD {
        return Err(WalError::Bound);
    }
    Ok(out)
}

pub(crate) fn decode_payload(bytes: &[u8]) -> Result<Payload, WalError> {
    let mut rest = bytes;
    let mut fields: Vec<(u16, Vec<u8>)> = Vec::new();
    while !rest.is_empty() {
        if rest.len() < 6 {
            return Err(WalError::Corrupt);
        }
        let tag = u16::from_be_bytes([rest[0], rest[1]]);
        let len = u32::from_be_bytes([rest[2], rest[3], rest[4], rest[5]]) as usize;
        rest = &rest[6..];
        if len > 512 || rest.len() < len {
            return Err(WalError::Corrupt);
        }
        if fields.iter().any(|(seen, _)| *seen == tag) {
            return Err(WalError::Corrupt);
        }
        fields.push((tag, rest[..len].to_vec()));
        rest = &rest[len..];
    }
    let take = |tag| -> Result<String, WalError> {
        let raw = fields
            .iter()
            .find(|(seen, _)| *seen == tag)
            .map(|(_, value)| value.as_slice())
            .ok_or(WalError::Corrupt)?;
        let text = std::str::from_utf8(raw).map_err(|_| WalError::Corrupt)?;
        Ok(text.to_string())
    };
    let kind = Kind::from_name(&take(TAG_KIND)?)?;
    let mut payload = Payload {
        kind,
        original: String::new(),
        executed: String::new(),
        fingerprint: String::new(),
        role: String::new(),
        project: String::new(),
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
        reason: String::new(),
    };
    match kind {
        Kind::Admission | Kind::Completion => {
            payload.original = require_sha256_hex(&take(TAG_ORIGINAL)?)?;
            payload.executed = require_sha256_hex(&take(TAG_EXECUTED)?)?;
            payload.fingerprint = require_fingerprint(&take(TAG_FINGERPRINT)?)?;
            payload.role = require_ident(&take(TAG_ROLE)?)?;
            payload.project = require_project(&take(TAG_PROJECT)?)?;
            payload.policy = require_policy(&take(TAG_POLICY)?)?;
            let pid = take(TAG_PID)?;
            payload.backend_pid = pid.parse::<i32>().map_err(|_| WalError::Corrupt)?;
            if payload.backend_pid <= 0 {
                return Err(WalError::Corrupt);
            }
            payload.backend_start = require_backend_start(&take(TAG_BACKEND)?)?;
        }
        Kind::Refusal => {
            payload.role = require_ident(&take(TAG_ROLE)?)?;
            payload.project = require_project(&take(TAG_PROJECT)?)?;
            payload.reason = take(TAG_REASON)?;
            if payload.reason != "explicit_not_admitted" {
                return Err(WalError::Corrupt);
            }
        }
    }
    if kind == Kind::Completion {
        payload.status = take(TAG_STATUS)?;
        if !matches!(payload.status.as_str(), "succeeded" | "partial" | "failed") {
            return Err(WalError::Corrupt);
        }
        payload.truncated = flag(&take(TAG_TRUNCATED)?)?;
        payload.returned_rows = take(TAG_ROWS)?.parse::<u32>().map_err(|_| WalError::Corrupt)?;
        payload.result_bytes = take(TAG_BYTES)?.parse::<u64>().map_err(|_| WalError::Corrupt)?;
        payload.rollback = flag(&take(TAG_ROLLBACK)?)?;
        payload.socket = flag(&take(TAG_SOCKET)?)?;
        payload.stop = take(TAG_STOP)?;
        if !payload.stop.is_empty()
            && !matches!(
                payload.stop.as_str(),
                "timeout" | "canceled" | "type_refused" | "database" | "protocol"
            )
        {
            return Err(WalError::Corrupt);
        }
        payload.sqlstate = require_sqlstate(&take(TAG_SQLSTATE)?)?;
    }
    Ok(payload)
}

fn flag(text: &str) -> Result<bool, WalError> {
    match text {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(WalError::Corrupt),
    }
}

pub(crate) struct Scan {
    pub(crate) frames: Vec<DecodedFrame>,
    pub(crate) keep: usize,
    pub(crate) sealed: bool,
    pub(crate) torn: bool,
    pub(crate) segment_id: u32,
}

pub(crate) fn scan_segment(bytes: &[u8]) -> Result<Scan, WalError> {
    if bytes.len() < SEGMENT_HEADER_LEN || &bytes[..8] != SEGMENT_MAGIC {
        return Err(WalError::Corrupt);
    }
    let version = u16::from_be_bytes([bytes[8], bytes[9]]);
    if version != VERSION {
        return Err(WalError::Corrupt);
    }
    let segment_id = u32::from_be_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
    if segment_id == 0 {
        return Err(WalError::Corrupt);
    }
    if bytes.len() >= FOOTER_LEN && bytes[bytes.len() - FOOTER_LEN..].starts_with(FOOTER_MAGIC) {
        return scan_sealed(bytes, segment_id);
    }
    scan_open(bytes, segment_id)
}

fn take_frame(rest: &[u8]) -> Result<DecodedFrame, WalError> {
    if rest.len() < MIN_FRAME || &rest[..4] != FRAME_MAGIC {
        return Err(WalError::Corrupt);
    }
    let payload_len = u32::from_be_bytes([rest[47], rest[48], rest[49], rest[50]]) as usize;
    if payload_len > MAX_PAYLOAD {
        return Err(WalError::Corrupt);
    }
    let total = HEADER_LEN + payload_len + CHECKSUM_LEN;
    if rest.len() < total {
        return Err(WalError::Corrupt);
    }
    let decoded = decode_frame(&rest[..total])?;
    if decoded.kind != decode_payload(&decoded.payload)?.kind {
        return Err(WalError::Corrupt);
    }
    Ok(decoded)
}

fn scan_sealed(bytes: &[u8], segment_id: u32) -> Result<Scan, WalError> {
    let start = bytes.len() - FOOTER_LEN;
    let rest = &bytes[start..];
    let count = u32::from_be_bytes([rest[8], rest[9], rest[10], rest[11]]);
    let mut expected = [0u8; 32];
    expected.copy_from_slice(&rest[12..44]);
    if sha256(&bytes[..start]) != expected {
        return Err(WalError::Corrupt);
    }
    let mut offset = SEGMENT_HEADER_LEN;
    let mut frames = Vec::new();
    while offset < start {
        let decoded = take_frame(&bytes[offset..start])?;
        offset += decoded.len;
        frames.push(decoded);
        if offset > start {
            return Err(WalError::Corrupt);
        }
    }
    if offset != start || count as usize != frames.len() {
        return Err(WalError::Corrupt);
    }
    Ok(Scan {
        frames,
        keep: bytes.len(),
        sealed: true,
        torn: false,
        segment_id,
    })
}

fn scan_open(bytes: &[u8], segment_id: u32) -> Result<Scan, WalError> {
    let mut offset = SEGMENT_HEADER_LEN;
    let mut frames = Vec::new();
    let mut torn = false;
    while offset < bytes.len() {
        let rest = &bytes[offset..];
        if rest.starts_with(FOOTER_MAGIC) {
            return Err(WalError::Corrupt);
        }
        if rest.len() < MIN_FRAME || &rest[..4] != FRAME_MAGIC {
            torn = true;
            break;
        }
        let payload_len = u32::from_be_bytes([rest[47], rest[48], rest[49], rest[50]]) as usize;
        if payload_len > MAX_PAYLOAD {
            return Err(WalError::Corrupt);
        }
        let total = HEADER_LEN + payload_len + CHECKSUM_LEN;
        if rest.len() < total {
            torn = true;
            break;
        }
        let decoded = take_frame(rest)?;
        offset += decoded.len;
        frames.push(decoded);
    }
    Ok(Scan {
        frames,
        keep: offset,
        sealed: false,
        torn,
        segment_id,
    })
}

pub(crate) fn footer(content: &[u8], count: u32) -> [u8; FOOTER_LEN] {
    let mut out = [0u8; FOOTER_LEN];
    out[..8].copy_from_slice(FOOTER_MAGIC);
    out[8..12].copy_from_slice(&count.to_be_bytes());
    out[12..44].copy_from_slice(&sha256(content));
    out
}

pub(crate) fn uuid_string(bytes: &[u8; 16]) -> String {
    let hex = hex_encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub(crate) fn parse_uuid(text: &str) -> Result<[u8; 16], WalError> {
    let lowered = text.to_ascii_lowercase();
    let stripped: String = lowered.chars().filter(|char| *char != '-').collect();
    if stripped.len() != 32 {
        return Err(WalError::Bound);
    }
    let raw = hex_decode(&stripped)?;
    let mut out = [0u8; 16];
    out.copy_from_slice(&raw);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Payload {
        Payload {
            kind: Kind::Admission,
            original: "ab".repeat(32),
            executed: "cd".repeat(32),
            fingerprint: "0123456789abcdef".to_string(),
            role: "oo_r_clerk".to_string(),
            project: "shop".to_string(),
            policy: "a".repeat(64),
            backend_pid: 42,
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
    fn round_trip_hides_sql() {
        let payload = encode_payload(&sample()).unwrap();
        assert!(!payload.windows(6).any(|window| window == b"SELECT"));
        assert!(!payload.windows(22).any(|window| window == b"classified-literal-token"));
        let decoded = decode_payload(&payload).unwrap();
        assert_eq!(decoded.project, "shop");
        let spool = [7u8; 16];
        let request = [9u8; 16];
        let frame = encode_frame(Kind::Admission, &spool, &request, 1, &payload).unwrap();
        let back = decode_frame(&frame).unwrap();
        assert_eq!(back.seq, 1);
        assert_eq!(back.payload_hash, sha256(&payload));
        let mut bad = frame.clone();
        bad[60] ^= 0x01;
        assert!(decode_frame(&bad).is_err());
    }

    #[test]
    fn sql_shaped_project_is_rejected() {
        assert!(require_project("shop;drop").is_err());
        assert!(require_ident("oo_r_clerk'").is_err());
    }
}
