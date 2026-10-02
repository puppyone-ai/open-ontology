//! Canonical request bytes and the frozen response door.
//!
//! Serialization is `serde_json`. The bytes are accepted only when
//! [`oo_core::api::parse_api_json`] accepts them again. Response bytes are
//! parsed the same way, with the row-count and row-width checks below.

use oo_core::api::{ErrorEnvelope, QueryRequest, QueryResponse, parse_api_json};
use oo_core::diagnostic::{CONTRACT_API, Diagnostic};

use crate::QUERY_MAX_ROWS;
use crate::error::{LocalError, QueryError};

pub(crate) fn encode_request(request: &QueryRequest, cap: usize) -> Result<Vec<u8>, QueryError> {
    let bytes = canonical_request(request)?;
    let limit = cap.min(crate::QUERY_HTTP_MAX_BYTES);
    if bytes.len() > limit {
        return Err(QueryError::Local(LocalError::RequestTooLarge {
            len: bytes.len(),
            cap: limit,
        }));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| QueryError::Local(LocalError::RequestRejected))?;
    let reparsed = parse_api_json::<QueryRequest>(text, accept)
        .map_err(|_| QueryError::Local(LocalError::RequestRejected))?;
    if &reparsed != request {
        return Err(QueryError::Local(LocalError::RequestRejected));
    }
    let again = canonical_request(&reparsed)?;
    if again != bytes {
        return Err(QueryError::Local(LocalError::RequestRejected));
    }
    Ok(bytes)
}

/// Optional fields are omitted. `serde_json` would emit JSON null, and the
/// frozen schema rejects null for those integers and objects.
fn canonical_request(request: &QueryRequest) -> Result<Vec<u8>, QueryError> {
    let mut value = serde_json::to_value(request)
        .map_err(|_| QueryError::Local(LocalError::RequestRejected))?;
    omit_null_fields(&mut value);
    serde_json::to_vec(&value).map_err(|_| QueryError::Local(LocalError::RequestRejected))
}

fn omit_null_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|_, child| !child.is_null());
            for child in map.values_mut() {
                omit_null_fields(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                omit_null_fields(item);
            }
        }
        _ => {}
    }
}

pub(crate) fn decode_response(bytes: &[u8]) -> Result<QueryResponse, QueryError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| QueryError::Protocol(crate::error::ProtocolError::Rejected))?;
    parse_api_json::<QueryResponse>(text, validate_response)
        .map_err(|_| QueryError::Protocol(crate::error::ProtocolError::Rejected))
}

pub(crate) fn decode_error(bytes: &[u8]) -> Result<ErrorEnvelope, QueryError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| QueryError::Protocol(crate::error::ProtocolError::Rejected))?;
    parse_api_json::<ErrorEnvelope>(text, accept)
        .map_err(|_| QueryError::Protocol(crate::error::ProtocolError::Rejected))
}

fn accept<T>(_: &T) -> Vec<Diagnostic> {
    Vec::new()
}

fn validate_response(response: &QueryResponse) -> Vec<Diagnostic> {
    let mut errors = Vec::new();
    if response.returned_rows != response.rows.len() as u64 {
        errors.push(Diagnostic::error(
            CONTRACT_API,
            "cross_field",
            "/returned_rows",
            "returned_rows does not match the row array",
        ));
    }
    if response.rows.len() > QUERY_MAX_ROWS {
        errors.push(Diagnostic::error(
            CONTRACT_API,
            "cross_field",
            "/rows",
            "row count exceeds the public max_rows ceiling",
        ));
    }
    for (index, row) in response.rows.iter().enumerate() {
        if row.len() != response.columns.len() {
            errors.push(Diagnostic::error(
                CONTRACT_API,
                "cross_field",
                format!("/rows/{index}"),
                "row width does not match the column list",
            ));
        }
    }
    errors
}

const SCRUB_WORK_BUDGET: u64 = 4_000_000;
const SCRUB_OUTPUT_CAP: usize = 16 * 1024;
const OMISSIONS: &[&str] = &["[redacted]", "[omitted]", "****"];

/// Remove the caller's bearer, SQL, and base URL from gateway text.
///
/// Each secret byte may appear raw, as one `%HH` (either hex case), as `\/`
/// when the byte is `/`, or as `+` when the byte is a space. The text is not
/// percent-decoded, so a second encoding pass is never applied. A field that
/// exceeds the work or output bound, or that would still contain a secret, is
/// replaced by a static omission.
pub(crate) fn scrub_text(text: &str, token: &str, sql: &str, endpoint: &str) -> String {
    let mut owned = Vec::new();
    if !token.is_empty() {
        owned.push((token.to_owned(), false));
        owned.push((format!("Bearer {token}"), false));
    }
    if !sql.is_empty() {
        owned.push((sql.to_owned(), false));
    }
    owned.extend(locator_forms(endpoint));
    let needles = needles_from(&owned);
    if needles.is_empty() {
        return text.to_owned();
    }
    scrub_needles(text, &needles).unwrap_or_else(|| omission(&needles).to_owned())
}

struct Needle {
    bytes: Vec<u8>,
    fold_scheme: bool,
    scheme_end: usize,
}

fn needles_from(owned: &[(String, bool)]) -> Vec<Needle> {
    let mut needles = Vec::new();
    for (value, fold_scheme) in owned {
        if value.is_empty()
            || needles
                .iter()
                .any(|needle: &Needle| needle.bytes.as_slice() == value.as_bytes())
        {
            continue;
        }
        let scheme_end = if *fold_scheme {
            value.find("://").unwrap_or(0)
        } else {
            0
        };
        needles.push(Needle {
            bytes: value.as_bytes().to_vec(),
            fold_scheme: *fold_scheme,
            scheme_end,
        });
    }
    needles
}

/// Full query URL, origin, mount, and path. The longest match wins, so an
/// origin replacement cannot leave the mount path behind.
fn locator_forms(endpoint: &str) -> Vec<(String, bool)> {
    let mut forms = Vec::new();
    if endpoint.is_empty() {
        return forms;
    }
    forms.push((endpoint.to_owned(), true));
    if let Some(origin) = endpoint_origin(endpoint) {
        forms.push((origin.to_owned(), true));
        if let Some((_, authority)) = origin.split_once("://")
            && !authority.is_empty()
        {
            forms.push((authority.to_owned(), false));
        }
    }
    if let Some(path) = endpoint_path(endpoint)
        && path.len() > 1
    {
        forms.push((path.to_owned(), false));
        if let Some(mount) = path.strip_suffix("/v1/query")
            && mount.len() > 1
        {
            forms.push((mount.to_owned(), false));
        }
    }
    if let Some(base) = endpoint.strip_suffix("/v1/query")
        && !base.is_empty()
        && base != endpoint
    {
        forms.push((base.to_owned(), true));
        if !base.ends_with('/') {
            forms.push((format!("{base}/"), true));
        }
    }
    forms
}

fn endpoint_path(endpoint: &str) -> Option<&str> {
    let (_, rest) = endpoint.split_once("://")?;
    let index = rest.find('/')?;
    Some(&rest[index..])
}

enum Span {
    Miss,
    Hit(usize),
    Stop,
}

fn scrub_needles(text: &str, needles: &[Needle]) -> Option<String> {
    let marker = omission(needles);
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut index = 0;
    let mut work = 0u64;
    while index < bytes.len() {
        if work > SCRUB_WORK_BUDGET {
            return None;
        }
        let mut best: Option<usize> = None;
        for needle in needles {
            match match_span(bytes, index, needle, &mut work) {
                Span::Stop => return None,
                Span::Miss => {}
                Span::Hit(end) => best = Some(best.map_or(end, |current| current.max(end))),
            }
        }
        if let Some(end) = best {
            out.push_str(marker);
            if out.len() > SCRUB_OUTPUT_CAP || !text.is_char_boundary(end) {
                return None;
            }
            index = end;
            continue;
        }
        let ch = text[index..].chars().next()?;
        out.push(ch);
        if out.len() > SCRUB_OUTPUT_CAP {
            return None;
        }
        index += ch.len_utf8();
    }
    if secret_remains(&out, needles, &mut work) || work > SCRUB_WORK_BUDGET {
        return None;
    }
    let generic = oo_core::schema_util::scrub_endpoint_text(&out);
    if generic.len() > SCRUB_OUTPUT_CAP || secret_remains(&generic, needles, &mut work) {
        return None;
    }
    Some(generic)
}

fn omission(needles: &[Needle]) -> &'static str {
    for candidate in OMISSIONS {
        let mut work = 0u64;
        if !secret_remains(candidate, needles, &mut work) {
            return candidate;
        }
    }
    ""
}

fn secret_remains(text: &str, needles: &[Needle], work: &mut u64) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if *work > SCRUB_WORK_BUDGET {
            return true;
        }
        for needle in needles {
            match match_span(bytes, index, needle, work) {
                Span::Hit(_) | Span::Stop => return true,
                Span::Miss => {}
            }
        }
        let Some(ch) = text[index..].chars().next() else {
            return true;
        };
        index += ch.len_utf8();
    }
    false
}

/// `%HH` is tried before the raw byte so an encoded secret is consumed whole.
fn match_span(text: &[u8], start: usize, needle: &Needle, work: &mut u64) -> Span {
    if needle.bytes.is_empty() || start >= text.len() {
        return Span::Miss;
    }
    let mut index = start;
    let mut secret = 0;
    while secret < needle.bytes.len() {
        *work += 1;
        if *work > SCRUB_WORK_BUDGET {
            return Span::Stop;
        }
        if index >= text.len() {
            return Span::Miss;
        }
        let expected = needle.bytes[secret];
        let fold = needle.fold_scheme && secret < needle.scheme_end;
        if text[index] == b'%'
            && index + 2 < text.len()
            && hex_byte(text[index + 1], text[index + 2])
                .is_some_and(|decoded| byte_eq(decoded, expected, fold))
        {
            index += 3;
            secret += 1;
            continue;
        }
        if expected == b'/' && index + 1 < text.len() && text[index] == b'\\' && text[index + 1] == b'/'
        {
            index += 2;
            secret += 1;
            continue;
        }
        if byte_eq(text[index], expected, fold) {
            index += 1;
            secret += 1;
            continue;
        }
        if expected == b' ' && text[index] == b'+' {
            index += 1;
            secret += 1;
            continue;
        }
        return Span::Miss;
    }
    if index > start && utf8_boundary(text, index) {
        Span::Hit(index)
    } else {
        Span::Miss
    }
}

fn utf8_boundary(text: &[u8], index: usize) -> bool {
    match text.get(index) {
        None => index == text.len(),
        Some(byte) => byte & 0b1100_0000 != 0b1000_0000,
    }
}

fn byte_eq(found: u8, expected: u8, fold: bool) -> bool {
    if fold {
        found.eq_ignore_ascii_case(&expected)
    } else {
        found == expected
    }
}

fn hex_byte(hi: u8, lo: u8) -> Option<u8> {
    Some((hex_nibble(hi)? << 4) | hex_nibble(lo)?)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn endpoint_origin(endpoint: &str) -> Option<&str> {
    let (scheme, rest) = endpoint.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    if authority.is_empty() {
        return None;
    }
    Some(&endpoint[..scheme.len() + 3 + authority.len()])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(json: &str) -> QueryRequest {
        parse_api_json(json, accept).expect("request fixture")
    }

    #[test]
    fn canonical_request_matches_the_wire_fixture_and_omits_null_fields() {
        let full = request(include_str!(
            "../../../contracts/sdk/json-query/fixtures/query-request.json"
        ));
        let bytes = encode_request(&full, crate::QUERY_HTTP_MAX_BYTES).expect("encode");
        assert_eq!(
            bytes,
            include_bytes!("../../../contracts/sdk/json-query/fixtures/query-request.wire.json")
        );
        let mut omitted = full.clone();
        omitted.max_rows = None;
        omitted.timeout_ms = None;
        omitted.client_context = None;
        let short = encode_request(&omitted, crate::QUERY_HTTP_MAX_BYTES).expect("omit");
        assert_eq!(short, br#"{"format":"json","sql":"select 1"}"#);
    }

    #[test]
    fn over_cap_request_is_rejected_without_truncation() {
        let full = request(include_str!(
            "../../../contracts/sdk/json-query/fixtures/query-request.json"
        ));
        let error = encode_request(&full, 8).expect_err("cap");
        match error {
            QueryError::Local(LocalError::RequestTooLarge { len, cap }) => {
                assert!(len > cap);
                assert_eq!(cap, 8);
            }
            other => panic!("{other}"),
        }
    }
}
