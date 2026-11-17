//! Small HTTP helpers: query strings, JSON bodies, `If-Match`, responses.

use axum::body::Body;
use axum::http::{HeaderMap, Response, StatusCode, header};
use serde_json::Value;

use super::error::{ApiError, code};

/// Parsed query string. Repeated keys keep the last value.
#[derive(Clone, Debug, Default)]
pub struct Query(Vec<(String, String)>);

impl Query {
    /// Parse `a=1&b=x%20y`; malformed escapes are a 400.
    pub fn parse(raw: Option<&str>) -> Result<Self, ApiError> {
        let mut out = Vec::new();
        for pair in raw.unwrap_or("").split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let bad = || ApiError::bad("/query", "malformed percent encoding");
            out.push((
                pct_decode(k).ok_or_else(bad)?,
                pct_decode(v).ok_or_else(bad)?,
            ));
        }
        Ok(Self(out))
    }

    /// Last value of `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Boolean parameter with a default.
    pub fn flag(&self, key: &str, default: bool) -> Result<bool, ApiError> {
        match self.get(key) {
            None => Ok(default),
            Some("true") | Some("1") => Ok(true),
            Some("false") | Some("0") => Ok(false),
            Some(_) => Err(ApiError::bad(
                &format!("/query/{key}"),
                "expected a boolean",
            )),
        }
    }

    /// `limit`: default 100, 1 to 1000.
    pub fn limit(&self) -> Result<i64, ApiError> {
        match self.get("limit") {
            None => Ok(100),
            Some(v) => match v.parse::<i64>() {
                Ok(n) if (1..=1000).contains(&n) => Ok(n),
                _ => Err(ApiError::bad("/query/limit", "limit must be 1 to 1000")),
            },
        }
    }
}

/// Percent decoding (`+` is a space). `None` on a bad escape or invalid UTF-8.
pub fn pct_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3)?;
                let v = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                out.push(v);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Request body as JSON; an empty body is `{}`.
pub fn parse_json(body: &[u8]) -> Result<Value, ApiError> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Object(Default::default()));
    }
    serde_json::from_slice(body).map_err(|_| ApiError::bad("", "body is not valid JSON"))
}

/// `If-Match: "r<revision>"` (weak marker and missing quotes tolerated).
pub fn if_match(headers: &HeaderMap) -> Result<i64, ApiError> {
    let raw = headers
        .get(header::IF_MATCH)
        .ok_or_else(|| ApiError::new(code::PRECONDITION_REQUIRED, "If-Match is required"))?
        .to_str()
        .map_err(|_| ApiError::bad("/headers/If-Match", "not text"))?;
    let t = raw.trim().trim_start_matches("W/").trim_matches('"');
    t.strip_prefix('r')
        .and_then(|n| n.parse::<i64>().ok())
        .filter(|n| *n >= 1)
        .ok_or_else(|| ApiError::bad("/headers/If-Match", "expected \"r<revision>\""))
}

/// `"r<revision>"`.
pub fn etag(revision: i64) -> String {
    format!("\"r{revision}\"")
}

/// A JSON response.
pub fn json(status: u16, value: &Value) -> Response<Body> {
    json_with(status, value, &[])
}

/// A JSON response with extra headers.
pub fn json_with(status: u16, value: &Value, headers: &[(&str, String)]) -> Response<Body> {
    let mut b = Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8");
    for (k, v) in headers {
        b = b.header(*k, v.as_str());
    }
    b.body(Body::from(value.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

/// A plain text response with its own content type.
pub fn text(status: u16, content_type: &str, body: String) -> Response<Body> {
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

/// Lowercase UUID text.
pub fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => matches!(c, b'0'..=b'9' | b'a'..=b'f'),
        })
}

/// SQL expression: RFC 3339 UTC text of a timestamptz column.
pub const FMT: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parsing() {
        let q = Query::parse(Some("a=1&b=x%20y&c=%E4%B8%AD&a=2&flag=true&limit=5")).unwrap();
        assert_eq!(q.get("a"), Some("2"));
        assert_eq!(q.get("b"), Some("x y"));
        assert_eq!(q.get("c"), Some("中"));
        assert!(q.flag("flag", false).unwrap());
        assert!(!q.flag("missing", false).unwrap());
        assert_eq!(q.limit().unwrap(), 5);
        assert!(Query::parse(Some("a=%zz")).is_err());
        assert!(Query::parse(Some("limit=0")).unwrap().limit().is_err());
        assert!(Query::parse(Some("limit=1001")).unwrap().limit().is_err());
        assert_eq!(Query::parse(None).unwrap().limit().unwrap(), 100);
    }

    #[test]
    fn if_match_forms() {
        let mut h = HeaderMap::new();
        assert_eq!(if_match(&h).unwrap_err().code, code::PRECONDITION_REQUIRED);
        h.insert(header::IF_MATCH, "\"r7\"".parse().unwrap());
        assert_eq!(if_match(&h).unwrap(), 7);
        h.insert(header::IF_MATCH, "W/\"r3\"".parse().unwrap());
        assert_eq!(if_match(&h).unwrap(), 3);
        h.insert(header::IF_MATCH, "7".parse().unwrap());
        assert_eq!(if_match(&h).unwrap_err().code, code::BAD_REQUEST);
        assert_eq!(etag(4), "\"r4\"");
    }

    #[test]
    fn uuid_shape() {
        assert!(is_uuid("0f8fad5b-d9cb-469f-a165-70867728950e"));
        assert!(!is_uuid("0F8FAD5B-D9CB-469F-A165-70867728950E"));
        assert!(!is_uuid("nope"));
    }

    #[test]
    fn json_bodies() {
        assert_eq!(parse_json(b"  ").unwrap(), serde_json::json!({}));
        assert!(parse_json(b"{").is_err());
    }
}
