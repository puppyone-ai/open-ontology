//! JSON Schema helpers shared by hand-written `JsonSchema` implementations.
//!
//! Generated schemas are draft 2020-12 objects. Callers validate instances with
//! `jsonschema` offline mode so `$schema` URIs are never fetched.

use schemars::Schema;
use serde_json::Value;

pub fn schema_object(value: Value) -> Schema {
    serde_json::from_value(value).expect("hand-written schema must be a JSON Schema object or bool")
}

pub fn string_schema(pattern: &'static str, min_length: usize, max_length: usize) -> Schema {
    schema_object(serde_json::json!({
        "type": "string",
        "pattern": pattern,
        "minLength": min_length,
        "maxLength": max_length
    }))
}

/// Draft 2020-12 compiler used by every trust-boundary parser.
///
/// `maxLength` counts Unicode scalars. `x-utf8-max-bytes` is the byte budget.
/// Format assertions are on. Unknown formats stay annotations. This compiler
/// does not make JSON Schema equivalent to the Rust constructors: year `0000`,
/// a negative decimal scale, and `retryable` versus `ErrorCode` are Rust rules.
pub fn compile_schema(
    schema: &Value,
) -> Result<jsonschema::Validator, jsonschema::ValidationError<'static>> {
    jsonschema::draft202012::options()
        .offline()
        .should_validate_formats(true)
        .with_keyword("x-utf8-max-bytes", utf8_max_bytes)
        .build(schema)
}

fn utf8_max_bytes<'a>(
    _parent: &'a serde_json::Map<String, Value>,
    schema: &'a Value,
    _path: jsonschema::paths::Location,
) -> Result<Box<dyn for<'i> jsonschema::Keyword<'i>>, jsonschema::ValidationError<'a>> {
    let Some(max) = schema.as_u64() else {
        return Err(jsonschema::ValidationError::custom(
            "x-utf8-max-bytes must be an integer",
        ));
    };
    let max = usize::try_from(max).map_err(|_| {
        jsonschema::ValidationError::custom("x-utf8-max-bytes does not fit this host")
    })?;
    Ok(Box::new(Utf8MaxBytes { max }))
}

struct Utf8MaxBytes {
    max: usize,
}

impl<'i> jsonschema::Keyword<'i> for Utf8MaxBytes {
    fn validate(
        &self,
        instance: &'i serde_json::Value,
    ) -> Result<(), jsonschema::ValidationError<'i>> {
        if self.is_valid(instance) {
            Ok(())
        } else {
            Err(jsonschema::ValidationError::custom(
                "string exceeds its UTF-8 byte budget",
            ))
        }
    }

    fn is_valid(&self, instance: &'i serde_json::Value) -> bool {
        match instance.as_str() {
            Some(text) => text.len() <= self.max,
            None => true,
        }
    }
}

/// How many percent-decoding passes may be used to classify a URL.
///
/// Each pass shrinks or stays the same size. A `%` that is still unresolved
/// after this bound is unproved and the URL is redacted. Decoded bytes are
/// never written back into the diagnostic.
const PERCENT_DECODE_BOUND: usize = 8;

/// Redact userinfo, query, and fragment in a URL that a diagnostic quotes.
///
/// A URL token runs until Unicode whitespace. Other non-ASCII scalars stay
/// inside the token. Text outside a URL is copied by slice, so non-URL
/// Unicode stays byte-for-byte. This is not a general secret scanner:
/// `token=abc` with no scheme is unchanged. A URL whose boundary cannot be
/// proved is replaced by `scheme://redacted`.
pub fn scrub_endpoint_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let Some(start) = next_url_start(text, index) else {
            out.push_str(&text[index..]);
            break;
        };
        out.push_str(&text[index..start]);
        let end = url_token_end(text, start);
        let (core, trailing) = peel_trailing_punctuation(&text[start..end]);
        out.push_str(&redact_url(core));
        out.push_str(trailing);
        index = end;
    }
    out
}

fn next_url_start(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = from;
    while index + 3 <= bytes.len() {
        if bytes[index..].starts_with(b"://") {
            let mut start = index;
            while start > from && is_scheme_byte(bytes[start - 1]) {
                start -= 1;
            }
            let scheme_continues = start == from && from > 0 && is_scheme_byte(bytes[from - 1]);
            if !scheme_continues && start < index && bytes[start].is_ascii_alphabetic() {
                return Some(start);
            }
        }
        index += 1;
    }
    None
}

fn is_scheme_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')
}

fn url_token_end(text: &str, start: usize) -> usize {
    let mut index = start;
    for ch in text[start..].chars() {
        if ch.is_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }
    index
}

fn peel_trailing_punctuation(token: &str) -> (&str, &str) {
    let bytes = token.as_bytes();
    let mut end = bytes.len();
    while end > 0 && is_trailing_punctuation(bytes[end - 1]) {
        if !token[..end - 1].contains("://") {
            break;
        }
        // A `]` that closes the host stays in the URL. Peeling it and then
        // failing the proof would append that bracket after `redacted`.
        if bytes[end - 1] == b']' && bracket_closes_host(&token[..end]) {
            break;
        }
        end -= 1;
    }
    (&token[..end], &token[end..])
}

/// The final byte closes an IPv6 host, rather than prose wrapped around the URL.
///
/// A later `]`, or a `]` after the authority has already ended at `/`, `?`, or
/// `#`, is prose and may peel. Malformed host brackets stay inside the token
/// so a failed proof does not write them back onto `redacted`.
fn bracket_closes_host(token: &str) -> bool {
    let Some((_, rest)) = token.split_once("://") else {
        return false;
    };
    let bytes = rest.as_bytes();
    if bytes.last() != Some(&b']') {
        return false;
    }
    let mut depth: u32 = 0;
    let mut opened = false;
    for (index, &byte) in bytes.iter().enumerate() {
        match byte {
            b'[' => {
                depth = depth.saturating_add(1);
                opened = true;
            }
            b']' if depth == 0 => return false,
            b']' => {
                depth -= 1;
                if index + 1 == bytes.len() {
                    return opened && depth == 0;
                }
            }
            b'/' | b'?' | b'#' if depth == 0 => return false,
            _ => {}
        }
    }
    false
}

fn is_trailing_punctuation(byte: u8) -> bool {
    matches!(
        byte,
        b'"' | b'\'' | b')' | b']' | b'}' | b',' | b'.' | b';' | b':' | b'>' | b'<'
    )
}

fn redact_url(core: &str) -> String {
    let Some(marker) = core.find("://") else {
        return core.to_owned();
    };
    let scheme = &core[..marker];
    let rest = &core[marker + 3..];
    match split_url(rest) {
        Some(parts) => render_url(scheme, &parts),
        None => format!("{scheme}://redacted"),
    }
}

struct UrlParts<'a> {
    authority: &'a str,
    path: Option<&'a str>,
    has_query: bool,
}

fn split_url(rest: &str) -> Option<UrlParts<'_>> {
    if rest.is_empty()
        || rest.contains("://")
        || rest.contains('"')
        || rest.contains('\'')
        || percent_hides_boundary(rest)
    {
        return None;
    }
    let before_fragment = rest.split_once('#').map(|(head, _)| head).unwrap_or(rest);
    let (before_query, has_query) = match before_fragment.split_once('?') {
        Some((head, _)) => (head, true),
        None => (before_fragment, false),
    };
    let (authority, path) = match before_query.split_once('/') {
        Some((authority, path)) => (authority, Some(path)),
        None => (before_query, None),
    };
    if authority.is_empty() || authority.bytes().filter(|byte| *byte == b'@').count() > 1 {
        return None;
    }
    if let Some((_, host)) = authority.split_once('@')
        && host.is_empty()
    {
        return None;
    }
    if path.is_some_and(|path| path.contains('@')) {
        return None;
    }
    if segment_hides_needle(authority) || path.is_some_and(segment_hides_needle) {
        return None;
    }
    if !authority_is_proved(authority) {
        return None;
    }
    Some(UrlParts {
        authority,
        path,
        has_query,
    })
}

/// A proved authority is userinfo plus a host, or a host with no userinfo.
///
/// The authority is percent-decoded at most [`PERCENT_DECODE_BOUND`] times
/// before the host check. Those bytes are classification only and are never
/// written. `host:443`, `user:443`, and `[2001:db8::1]:443` stay, including
/// when the colon is encoded (`user%3A443`). A colon that is not a numeric
/// port and not an IPv6 literal fails closed. A `%` left after the bound,
/// or an invalid `%HH`, fails closed.
fn authority_is_proved(authority: &str) -> bool {
    let Some(decoded) = classify_authority(authority.as_bytes()) else {
        return false;
    };
    match std::str::from_utf8(&decoded) {
        Ok(text) => authority_shape_proved(text),
        // A decoded colon that is not UTF-8 cannot be proved as a port or
        // an IPv6 literal. No colon keeps the raw-host check.
        Err(_) => !decoded.contains(&b':') && authority_shape_proved(authority),
    }
}

fn authority_shape_proved(authority: &str) -> bool {
    match authority.split_once('@') {
        Some((_, host)) => host_is_proved(host),
        None => host_is_proved(authority),
    }
}

/// Eight-pass decode of one authority. `None` means the authority is unproved.
/// The returned bytes must not be copied into a diagnostic.
fn classify_authority(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut current = bytes.to_vec();
    for _ in 0..PERCENT_DECODE_BOUND {
        match decode_once(&current) {
            Decode::Invalid | Decode::Hidden => return None,
            Decode::Done => return Some(current),
            Decode::Next(next) => current = next,
        }
    }
    if current.contains(&b'%') {
        None
    } else {
        Some(current)
    }
}

fn host_is_proved(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    if let Some(rest) = host.strip_prefix('[') {
        let Some((inside, after)) = rest.split_once(']') else {
            return false;
        };
        if !ipv6_body(inside) {
            return false;
        }
        if after.is_empty() {
            return true;
        }
        let Some(port) = after.strip_prefix(':') else {
            return false;
        };
        return numeric_port(port);
    }
    match host.rsplit_once(':') {
        None => true,
        Some((name, port)) => !name.is_empty() && !name.contains(':') && numeric_port(port),
    }
}

fn ipv6_body(inside: &str) -> bool {
    !inside.is_empty()
        && inside.contains(':')
        && inside
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b':' || byte == b'.')
}

fn numeric_port(port: &str) -> bool {
    !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

fn render_url(scheme: &str, parts: &UrlParts<'_>) -> String {
    let mut out = String::new();
    out.push_str(scheme);
    out.push_str("://");
    if let Some((_, host)) = parts.authority.split_once('@') {
        out.push_str("redacted@");
        out.push_str(host);
    } else {
        out.push_str(parts.authority);
    }
    if let Some(path) = parts.path {
        out.push('/');
        out.push_str(path);
    }
    if parts.has_query {
        out.push_str("?redacted");
    }
    out
}

fn segment_hides_needle(text: &str) -> bool {
    let mut current = text.as_bytes().to_vec();
    for _ in 0..PERCENT_DECODE_BOUND {
        if contains_needle(&current) {
            return true;
        }
        match decode_once(&current) {
            Decode::Invalid | Decode::Hidden => return true,
            Decode::Done => return false,
            Decode::Next(next) => current = next,
        }
    }
    contains_needle(&current) || current.contains(&b'%')
}

fn contains_needle(bytes: &[u8]) -> bool {
    const NEEDLES: &[&[u8]] = &[
        b"password=",
        b"token=",
        b"secret=",
        b"api_key=",
        b"access_key=",
    ];
    let lower: Vec<u8> = bytes.iter().map(u8::to_ascii_lowercase).collect();
    NEEDLES
        .iter()
        .any(|needle| lower.windows(needle.len()).any(|window| window == *needle))
}

enum Decode {
    Invalid,
    Done,
    Hidden,
    Next(Vec<u8>),
}

/// One percent-decoding pass. `Hidden` means a `%HH` octet is `@`, `?`, or `#`.
/// A decoded `:` stays `Next`. [`authority_is_proved`] decides whether that
/// colon is a port, an IPv6 literal, or unproved userinfo. The caller discards
/// `Next`; it is classification only.
fn decode_once(bytes: &[u8]) -> Decode {
    if !bytes.contains(&b'%') {
        return Decode::Done;
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut hidden = false;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Decode::Invalid;
            }
            let Some(hi) = hex_value(bytes[index + 1]) else {
                return Decode::Invalid;
            };
            let Some(lo) = hex_value(bytes[index + 2]) else {
                return Decode::Invalid;
            };
            let value = (hi << 4) | lo;
            if matches!(value, b'@' | b'?' | b'#') {
                hidden = true;
            }
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    if hidden {
        Decode::Hidden
    } else {
        Decode::Next(out)
    }
}

fn percent_hides_boundary(text: &str) -> bool {
    let mut current = text.as_bytes().to_vec();
    for _ in 0..PERCENT_DECODE_BOUND {
        match decode_once(&current) {
            Decode::Invalid | Decode::Hidden => return true,
            Decode::Done => return false,
            Decode::Next(next) => current = next,
        }
    }
    current.contains(&b'%')
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
