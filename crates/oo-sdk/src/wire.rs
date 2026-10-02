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

pub(crate) fn scrub_text(text: &str, token: &str, sql: &str, endpoint: &str) -> String {
    let mut out = text.to_owned();
    if token.len() >= 8 {
        out = out.replace(token, "[redacted]");
        let bearer = format!("Bearer {token}");
        out = out.replace(&bearer, "[redacted]");
    }
    if sql.len() >= 24 {
        out = out.replace(sql, "[redacted]");
    }
    let mut out = oo_core::schema_util::scrub_endpoint_text(&out);
    // Core keeps a proved host. This client's own origin is still a locator.
    if let Some(origin) = endpoint_origin(endpoint)
        && origin.len() >= 8
    {
        out = out.replace(origin, "[redacted]");
    }
    out
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
