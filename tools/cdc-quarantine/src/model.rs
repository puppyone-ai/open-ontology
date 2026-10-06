//! Frozen control-event schema. See `CONTRACT.md`.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::jsonline::parse_json_line;
use crate::sha256::{hex, sha256};

pub const CONTRACT: &str = "cdc-quarantine-v1";
pub const MAX_PAYLOAD_BYTES: u64 = 65_536;
pub const MAX_EVENTS: u64 = 1_024;
pub const MAX_DISK_BYTES: u64 = 67_108_864;
pub const MAX_LINE_BYTES: usize = 1_048_576;
pub const MAX_PAUSED: usize = 256;

const KEYS: [&str; 11] = [
    "collection",
    "epoch",
    "event_id",
    "kind",
    "lineage",
    "payload",
    "payload_sha256",
    "reason",
    "sequence",
    "source",
    "source_position",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultCode {
    Malformed,
    LineTooLong,
    PayloadTooLarge,
    DigestMismatch,
    PayloadConflict,
    PathRefused,
    ForeignDirectory,
    BindingConflict,
    DiskQuota,
    RecordCap,
    LimitRaised,
    Incomplete,
    Io,
    NotFound,
    Usage,
}

impl FaultCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::LineTooLong => "line_too_long",
            Self::PayloadTooLarge => "payload_too_large",
            Self::DigestMismatch => "digest_mismatch",
            Self::PayloadConflict => "payload_conflict",
            Self::PathRefused => "path_refused",
            Self::ForeignDirectory => "foreign_directory",
            Self::BindingConflict => "binding_conflict",
            Self::DiskQuota => "disk_quota",
            Self::RecordCap => "record_cap",
            Self::LimitRaised => "limit_raised",
            Self::Incomplete => "incomplete",
            Self::Io => "io",
            Self::NotFound => "not_found",
            Self::Usage => "usage",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    SchemaChanged,
    Truncate,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SchemaChanged => "schema_changed",
            Self::Truncate => "truncate",
        }
    }

    fn parse(text: &str) -> Result<Self, FaultCode> {
        match text {
            "schema_changed" => Ok(Self::SchemaChanged),
            "truncate" => Ok(Self::Truncate),
            _ => Err(FaultCode::Malformed),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_payload_bytes: u64,
    pub max_events: u64,
    pub max_disk_bytes: u64,
}

impl Limits {
    pub fn product() -> Self {
        Self {
            max_payload_bytes: MAX_PAYLOAD_BYTES,
            max_events: MAX_EVENTS,
            max_disk_bytes: MAX_DISK_BYTES,
        }
    }
}

pub fn limits_from_env() -> Result<Limits, FaultCode> {
    let mut limits = Limits::product();
    if let Some(text) = env_var("CDC_QUARANTINE_MAX_PAYLOAD_BYTES")? {
        limits.max_payload_bytes = parse_limit(&text, MAX_PAYLOAD_BYTES)?;
    }
    if let Some(text) = env_var("CDC_QUARANTINE_MAX_EVENTS")? {
        limits.max_events = parse_limit(&text, MAX_EVENTS)?;
    }
    if let Some(text) = env_var("CDC_QUARANTINE_MAX_DISK_BYTES")? {
        limits.max_disk_bytes = parse_limit(&text, MAX_DISK_BYTES)?;
    }
    Ok(limits)
}

fn env_var(name: &str) -> Result<Option<String>, FaultCode> {
    match std::env::var(name) {
        Ok(text) => Ok(Some(text)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(FaultCode::Usage),
    }
}

fn parse_limit(text: &str, max: u64) -> Result<u64, FaultCode> {
    if text.is_empty() || text.len() > 20 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FaultCode::Usage);
    }
    if text.len() > 1 && text.starts_with('0') {
        return Err(FaultCode::Usage);
    }
    let value: u64 = text.parse().map_err(|_| FaultCode::Usage)?;
    if value > max {
        return Err(FaultCode::LimitRaised);
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlEvent {
    pub source: String,
    pub lineage: String,
    pub epoch: u32,
    pub event_id: String,
    pub sequence: u64,
    pub collection: String,
    pub reason: Reason,
    pub payload: Vec<u8>,
    pub payload_sha256: String,
    pub source_position: String,
}

pub fn identity_preimage(event: &ControlEvent) -> String {
    format!(
        "{{\"epoch\":{},\"event_id\":\"{}\",\"lineage\":\"{}\",\"source\":\"{}\"}}",
        event.epoch, event.event_id, event.lineage, event.source
    )
}

#[derive(Clone, Debug)]
pub struct Rejected {
    pub code: FaultCode,
    pub event: Option<ControlEvent>,
}

impl From<FaultCode> for Box<Rejected> {
    fn from(code: FaultCode) -> Self {
        Box::new(Rejected { code, event: None })
    }
}

pub fn parse_control(line: &str, limits: &Limits) -> Result<ControlEvent, Box<Rejected>> {
    let value = parse_json_line(line)?;
    let Value::Object(map) = value else {
        return Err(FaultCode::Malformed.into());
    };
    if map.len() != KEYS.len() {
        return Err(FaultCode::Malformed.into());
    }
    for key in KEYS {
        if !map.contains_key(key) {
            return Err(FaultCode::Malformed.into());
        }
    }
    let kind = json_str(map.get("kind").ok_or(FaultCode::Malformed)?)?;
    if kind != "control" {
        return Err(FaultCode::Malformed.into());
    }
    let source = json_str(map.get("source").ok_or(FaultCode::Malformed)?)?;
    let lineage = json_str(map.get("lineage").ok_or(FaultCode::Malformed)?)?;
    let event_id = json_str(map.get("event_id").ok_or(FaultCode::Malformed)?)?;
    let collection = json_str(map.get("collection").ok_or(FaultCode::Malformed)?)?;
    let reason_text = json_str(map.get("reason").ok_or(FaultCode::Malformed)?)?;
    let reason = Reason::parse(&reason_text)?;
    let payload_sha256 = json_str(map.get("payload_sha256").ok_or(FaultCode::Malformed)?)?;
    let source_position = json_str(map.get("source_position").ok_or(FaultCode::Malformed)?)?;
    let epoch = json_u32(map.get("epoch").ok_or(FaultCode::Malformed)?)?;
    let sequence = json_u64(map.get("sequence").ok_or(FaultCode::Malformed)?)?;
    if sequence == 0 {
        return Err(FaultCode::Malformed.into());
    }
    let payload = match map.get("payload").ok_or(FaultCode::Malformed)? {
        Value::String(text) => text.as_bytes().to_vec(),
        _ => return Err(FaultCode::Malformed.into()),
    };
    if !valid_source(&source)
        || !valid_lineage(&lineage)
        || !valid_event_id(&event_id)
        || !valid_collection(&collection)
        || !valid_hex(&payload_sha256)
        || !valid_position(&source_position)
    {
        return Err(FaultCode::Malformed.into());
    }
    let event = ControlEvent {
        source,
        lineage,
        epoch,
        event_id,
        sequence,
        collection,
        reason,
        payload,
        payload_sha256,
        source_position,
    };
    let payload_len = u64::try_from(event.payload.len()).unwrap_or(u64::MAX);
    if payload_len == 0 || payload_len > limits.max_payload_bytes {
        return Err(Box::new(Rejected {
            code: FaultCode::PayloadTooLarge,
            event: Some(event),
        }));
    }
    let digest = hex(&sha256(&event.payload).ok_or(FaultCode::Io)?);
    if digest != event.payload_sha256 {
        return Err(Box::new(Rejected {
            code: FaultCode::DigestMismatch,
            event: Some(event),
        }));
    }
    Ok(event)
}

fn json_str(value: &Value) -> Result<String, FaultCode> {
    match value {
        Value::String(text) => Ok(text.clone()),
        _ => Err(FaultCode::Malformed),
    }
}

fn json_u32(value: &Value) -> Result<u32, FaultCode> {
    let number = json_u64(value)?;
    u32::try_from(number).map_err(|_| FaultCode::Malformed)
}

fn json_u64(value: &Value) -> Result<u64, FaultCode> {
    match value {
        Value::Number(number) => number.as_u64().ok_or(FaultCode::Malformed),
        _ => Err(FaultCode::Malformed),
    }
}

pub fn valid_source(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

pub fn valid_lineage(text: &str) -> bool {
    valid_hex(text)
}

pub fn valid_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn valid_event_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    bytes[0].is_ascii_alphanumeric()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'.' | b':' | b'-')
        })
}

pub fn valid_collection(text: &str) -> bool {
    let Some((schema, table)) = text.split_once('.') else {
        return false;
    };
    if schema.contains('.') || table.contains('.') {
        return false;
    }
    valid_source(schema) && valid_table(table)
}

fn valid_table(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn valid_epoch(text: &str) -> bool {
    if text.is_empty() || text.len() > 10 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    !(text.len() > 1 && text.starts_with('0'))
}

pub fn valid_position(text: &str) -> bool {
    if text.is_empty() || text.len() > 38 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    !(text.len() > 1 && text.starts_with('0'))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub contract: String,
    pub kind: String,
    pub source: String,
    pub lineage: String,
    pub epoch: u32,
    pub event_id: String,
    pub sequence: u64,
    pub collection: String,
    pub reason: String,
    pub payload_sha256: String,
    pub payload_bytes: u64,
    pub source_position: String,
    pub source_position_provenance: String,
    pub paused_collections: Vec<String>,
    pub schema_applied: bool,
    pub target_truncated: bool,
    pub target_applied: NullField,
    pub cross_table_consistent: bool,
    pub ack_allowed: bool,
    pub ack_eligible: bool,
    pub debezium_router: bool,
    pub upstream_commit_verified: bool,
    pub local_prerequisite: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NullField;

impl Serialize for NullField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_none()
    }
}

impl<'de> Deserialize<'de> for NullField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Option::<serde_json::Value>::deserialize(deserializer)? {
            None => Ok(Self),
            Some(_) => Err(D::Error::custom("expected null")),
        }
    }
}

impl Manifest {
    pub fn build(event: &ControlEvent, paused: &[String]) -> Result<Vec<u8>, FaultCode> {
        if paused.len() > MAX_PAUSED || !paused.iter().any(|name| name == &event.collection) {
            return Err(FaultCode::RecordCap);
        }
        if !is_sorted_unique(paused) {
            return Err(FaultCode::Io);
        }
        let manifest = Self {
            contract: CONTRACT.to_owned(),
            kind: "control_manifest".to_owned(),
            source: event.source.clone(),
            lineage: event.lineage.clone(),
            epoch: event.epoch,
            event_id: event.event_id.clone(),
            sequence: event.sequence,
            collection: event.collection.clone(),
            reason: event.reason.as_str().to_owned(),
            payload_sha256: event.payload_sha256.clone(),
            payload_bytes: u64::try_from(event.payload.len()).map_err(|_| FaultCode::Io)?,
            source_position: event.source_position.clone(),
            source_position_provenance: "caller_declared".to_owned(),
            paused_collections: paused.to_vec(),
            schema_applied: false,
            target_truncated: false,
            target_applied: NullField,
            cross_table_consistent: false,
            ack_allowed: false,
            ack_eligible: false,
            debezium_router: false,
            upstream_commit_verified: false,
            local_prerequisite: false,
        };
        let mut bytes = serde_json::to_vec(&manifest).map_err(|_| FaultCode::Io)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn flags_ok(&self) -> bool {
        self.contract == CONTRACT
            && self.kind == "control_manifest"
            && self.source_position_provenance == "caller_declared"
            && !self.schema_applied
            && !self.target_truncated
            && !self.cross_table_consistent
            && !self.ack_allowed
            && !self.ack_eligible
            && !self.debezium_router
            && !self.upstream_commit_verified
            && !self.local_prerequisite
            && valid_source(&self.source)
            && valid_lineage(&self.lineage)
            && valid_event_id(&self.event_id)
            && valid_collection(&self.collection)
            && valid_hex(&self.payload_sha256)
            && valid_position(&self.source_position)
            && (self.reason == "schema_changed" || self.reason == "truncate")
            && self.sequence >= 1
            && self.payload_bytes > 0
            && self.payload_bytes <= MAX_PAYLOAD_BYTES
            && self.paused_collections.len() <= MAX_PAUSED
            && self.paused_collections.iter().all(|name| valid_collection(name))
            && is_sorted_unique(&self.paused_collections)
            && self
                .paused_collections
                .binary_search(&self.collection)
                .is_ok()
    }
}

pub fn is_sorted_unique(names: &[String]) -> bool {
    names.windows(2).all(|pair| pair[0] < pair[1])
}

pub fn store_bytes(event: &ControlEvent) -> Vec<u8> {
    format!(
        "{CONTRACT}\n{}\n{}\n{}\n",
        event.source, event.lineage, event.epoch
    )
    .into_bytes()
}

pub fn receipt_bytes(payload_sha: &str, manifest_sha: &str) -> Vec<u8> {
    format!("durable_quarantined\n{payload_sha}\n{manifest_sha}\n").into_bytes()
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub source: String,
    pub lineage: String,
    pub epoch: u32,
}

impl Binding {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut lines = text.split('\n');
        let contract = lines.next()?;
        let source = lines.next()?;
        let lineage = lines.next()?;
        let epoch = lines.next()?;
        let rest = lines.next()?;
        if lines.next().is_some() || !rest.is_empty() || contract != CONTRACT {
            return None;
        }
        if !valid_source(source) || !valid_lineage(lineage) || !valid_epoch(epoch) {
            return None;
        }
        let epoch = epoch.parse::<u32>().ok()?;
        Some(Self {
            source: source.to_owned(),
            lineage: lineage.to_owned(),
            epoch,
        })
    }

    pub fn matches_event(&self, event: &ControlEvent) -> bool {
        self.source == event.source && self.lineage == event.lineage && self.epoch == event.epoch
    }

    pub fn matches_manifest(&self, manifest: &Manifest) -> bool {
        self.source == manifest.source && self.lineage == manifest.lineage && self.epoch == manifest.epoch
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub contract: &'static str,
    pub kind: &'static str,
    pub fault: Option<&'static str>,
    pub replay: bool,
    pub paused: bool,
    pub paused_collections: Vec<String>,
    pub unknown_incomplete: bool,
    pub attempt_collection: Option<String>,
    pub attempt_durable: bool,
    pub durable_quarantined: bool,
    pub incomplete: bool,
    pub local_prerequisite: bool,
    pub ack_eligible: bool,
    pub ack_allowed: bool,
    pub schema_applied: bool,
    pub target_truncated: bool,
    pub target_applied: Option<bool>,
    pub cross_table_consistent: bool,
    pub debezium_router: bool,
    pub upstream_commit_verified: bool,
    pub source: Option<String>,
    pub lineage: Option<String>,
    pub epoch: Option<u32>,
    pub event_id: Option<String>,
    pub sequence: Option<u64>,
    pub collection: Option<String>,
    pub reason: Option<String>,
    pub payload_sha256: Option<String>,
    pub source_position: Option<String>,
    pub source_position_provenance: &'static str,
    pub events: u64,
    pub durable_events: u64,
    pub incomplete_events: u64,
}

impl Status {
    pub fn fault(code: FaultCode, event: Option<&ControlEvent>) -> Self {
        let mut status = Self::base();
        status.fault = Some(code.as_str());
        status.paused = event.is_some()
            && !matches!(
                code,
                FaultCode::Malformed | FaultCode::LineTooLong | FaultCode::Usage | FaultCode::LimitRaised
            );
        if let Some(event) = event {
            status.fill_event(event);
            if status.paused {
                status.attempt_collection = Some(event.collection.clone());
            }
        }
        status.finish_flags();
        status
    }

    pub fn base() -> Self {
        Self {
            contract: CONTRACT,
            kind: "quarantine_status",
            fault: None,
            replay: false,
            paused: false,
            paused_collections: Vec::new(),
            unknown_incomplete: false,
            attempt_collection: None,
            attempt_durable: false,
            durable_quarantined: false,
            incomplete: false,
            local_prerequisite: false,
            ack_eligible: false,
            ack_allowed: false,
            schema_applied: false,
            target_truncated: false,
            target_applied: None,
            cross_table_consistent: false,
            debezium_router: false,
            upstream_commit_verified: false,
            source: None,
            lineage: None,
            epoch: None,
            event_id: None,
            sequence: None,
            collection: None,
            reason: None,
            payload_sha256: None,
            source_position: None,
            source_position_provenance: "caller_declared",
            events: 0,
            durable_events: 0,
            incomplete_events: 0,
        }
    }

    pub fn fill_event(&mut self, event: &ControlEvent) {
        self.source = Some(event.source.clone());
        self.lineage = Some(event.lineage.clone());
        self.epoch = Some(event.epoch);
        self.event_id = Some(event.event_id.clone());
        self.sequence = Some(event.sequence);
        self.collection = Some(event.collection.clone());
        self.reason = Some(event.reason.as_str().to_owned());
        self.payload_sha256 = Some(event.payload_sha256.clone());
        self.source_position = Some(event.source_position.clone());
        self.attempt_collection = Some(event.collection.clone());
    }

    pub fn finish_flags(&mut self) {
        self.ack_eligible = false;
        self.ack_allowed = false;
        self.schema_applied = false;
        self.target_truncated = false;
        self.target_applied = None;
        self.cross_table_consistent = false;
        self.debezium_router = false;
        self.upstream_commit_verified = false;
        self.local_prerequisite =
            self.durable_quarantined && !self.incomplete && self.fault.is_none();
        self.attempt_durable = self.local_prerequisite && !self.replay;
        if self.replay && self.durable_quarantined && !self.incomplete && self.fault.is_none() {
            self.attempt_durable = true;
        }
    }

    pub fn exit_code(&self) -> u8 {
        match self.fault.map(FaultCode::from_str) {
            Some(FaultCode::Io | FaultCode::Usage | FaultCode::NotFound) => 1,
            Some(_) => 2,
            None if self.incomplete => 3,
            None => 0,
        }
    }
}

impl FaultCode {
    fn from_str(text: &str) -> Self {
        match text {
            "malformed" => Self::Malformed,
            "line_too_long" => Self::LineTooLong,
            "payload_too_large" => Self::PayloadTooLarge,
            "digest_mismatch" => Self::DigestMismatch,
            "payload_conflict" => Self::PayloadConflict,
            "path_refused" => Self::PathRefused,
            "foreign_directory" => Self::ForeignDirectory,
            "binding_conflict" => Self::BindingConflict,
            "disk_quota" => Self::DiskQuota,
            "record_cap" => Self::RecordCap,
            "limit_raised" => Self::LimitRaised,
            "incomplete" => Self::Incomplete,
            "io" => Self::Io,
            "not_found" => Self::NotFound,
            "usage" => Self::Usage,
            _ => Self::Io,
        }
    }
}
