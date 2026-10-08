use serde::Deserialize;
use serde::Serialize;

pub const VERSION: &str = "oo.answer-citations/v1";
pub const TRUST: &str = "local_operator_declared";
pub const PROTOCOL: &str = "oo.gateway.query-pg-audit/v1";
pub const STDIN_MAX: usize = 1_048_576;
pub const ANSWER_MAX: usize = 65_536;
pub const RECEIPT_MAX: usize = 8;
pub const BODY_MAX: usize = 32_768;
pub const LINE_MAX: usize = 4_096;
pub const PAIR_MAX: usize = 8;
pub const CITATION_MAX: usize = 32;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub version: String,
    pub trust: String,
    pub binding: Binding,
    pub receipts: Vec<ReceiptIn>,
    pub answer: AnswerIn,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub session_id: String,
    pub turn_id: String,
    pub role: String,
    pub project: String,
    pub protocol: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptIn {
    pub receipt_sha256: String,
    pub body: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerIn {
    pub declares_data: bool,
    pub text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmitLine {
    pub protocol: String,
    pub record: String,
    pub original_sha256: String,
    pub executed_sha256: String,
    pub fingerprint_hex: String,
    pub role: String,
    pub project: String,
    pub policy_id: String,
    pub backend_pid: i64,
    pub backend_start: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteLine {
    pub protocol: String,
    pub record: String,
    pub status: String,
    pub truncated: bool,
    pub returned_rows: u32,
    pub result_bytes: u64,
    pub rollback_confirmed: bool,
    pub socket_dropped: bool,
    pub stop: Option<String>,
    pub sqlstate: Option<String>,
    pub original_sha256: String,
    pub executed_sha256: String,
    pub fingerprint_hex: String,
    pub role: String,
    pub project: String,
    pub policy_id: String,
    pub backend_pid: i64,
    pub backend_start: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub version: &'static str,
    pub trust: &'static str,
    pub authenticated_audit: bool,
    pub entailment: &'static str,
    pub warning_comprehension: &'static str,
    pub session_id: String,
    pub turn_id: String,
    pub role: String,
    pub project: String,
    pub protocol: &'static str,
    pub citation_status: &'static str,
    pub declares_data: bool,
    pub missing_citation: bool,
    pub receipts: Vec<ReceiptOut>,
    pub admitted: Vec<Admitted>,
    pub citations: Vec<CitationOut>,
    pub warnings: Vec<CodedId>,
    pub disclosures: Vec<Disclosure>,
}

#[derive(Debug, Serialize)]
pub struct ReceiptOut {
    pub ordinal: u32,
    pub receipt_sha256: String,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Admitted {
    pub query_id: String,
    pub receipt_ordinal: u32,
    pub pair_ordinal: u32,
    pub status: &'static str,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct CitationOut {
    pub query_id: String,
    pub ordinal: u32,
    pub classification: &'static str,
}

#[derive(Debug, Serialize)]
pub struct CodedId {
    pub code: &'static str,
    pub query_id: String,
}

#[derive(Debug, Serialize)]
pub struct Disclosure {
    pub code: &'static str,
    pub query_id: String,
    pub required: bool,
    pub comprehended: &'static str,
}

pub fn caller_label(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
}

pub fn slug(value: &str, max: usize) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= max
        && (bytes[0] == b'_' || bytes[0].is_ascii_lowercase())
        && bytes[1..]
            .iter()
            .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

pub fn role_name(value: &str) -> bool {
    value
        .strip_prefix("oo_r_")
        .is_some_and(|rest| slug(rest, 32))
}

pub fn hex_lower(value: &str, len: usize) -> bool {
    value.len() == len && hex_lower_range(value, len, len)
}

pub fn hex_lower_range(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn digits(value: &str, max: usize) -> bool {
    (1..=max).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn stop_name(value: &str) -> bool {
    matches!(
        value,
        "timeout" | "canceled" | "type_refused" | "database" | "protocol"
    )
}

pub fn sqlstate_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 5
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

pub fn status_name(value: &str) -> bool {
    matches!(value, "succeeded" | "partial" | "failed")
}

pub fn pid_ok(value: i64) -> bool {
    (1..=i64::from(i32::MAX)).contains(&value)
}
