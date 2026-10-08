//! One JSON document. Duplicate keys are refused.
//! Request bodies also refuse waiver and credential fields at every depth.
//! Pack documents refuse waiver fields only at the root, so a reviewed
//! `provenance.authorization` value can stay.

use std::collections::HashSet;

use serde_json::{Map, Number, Value};

use crate::error::ApiError;
use crate::sha256::sha256_hex;

const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 20_000;

const CREDENTIALS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "authorization",
    "credential",
    "private_key",
    "ca_pem",
    "api_key",
];

const WAIVERS: &[&str] = &[
    "force",
    "waive",
    "all_accepted",
    "allaccepted",
    "product_mode_can_activate",
    "customer_approved",
];

pub fn parse_request(bytes: &[u8]) -> Result<Value, ApiError> {
    parse(bytes, 65_536, true)
}

pub fn parse_document(bytes: &[u8]) -> Result<Value, ApiError> {
    parse(bytes, 2_097_152, false)
}

pub fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

pub fn canonical_sha(value: &Value) -> String {
    sha256_hex(canonical(value).as_bytes())
}

fn parse(bytes: &[u8], limit: usize, request: bool) -> Result<Value, ApiError> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err(ApiError::malformed("json"));
    }
    let mut parser = Parser { bytes, index: 0, nodes: 0, request };
    parser.skip_ws();
    let value = parser.value(1, true)?;
    parser.skip_ws();
    if parser.index != parser.bytes.len() {
        return Err(ApiError::malformed("json"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    nodes: usize,
    request: bool,
}

impl<'a> Parser<'a> {
    fn value(&mut self, depth: usize, root: bool) -> Result<Value, ApiError> {
        if depth > MAX_DEPTH || self.nodes >= MAX_NODES {
            return Err(ApiError::malformed("json"));
        }
        self.nodes += 1;
        self.skip_ws();
        let Some(byte) = self.peek() else {
            return Err(ApiError::malformed("json"));
        };
        match byte {
            b'{' => self.object(depth, root),
            b'[' => self.array(depth),
            b'"' => Ok(Value::String(self.string()?)),
            b't' => self.literal(b"true", Value::Bool(true)),
            b'f' => self.literal(b"false", Value::Bool(false)),
            b'n' => self.literal(b"null", Value::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(ApiError::malformed("json")),
        }
    }

    fn object(&mut self, depth: usize, root: bool) -> Result<Value, ApiError> {
        self.bump(b'{')?;
        let mut map = Map::new();
        let mut seen = HashSet::new();
        self.skip_ws();
        if self.eat(b'}') {
            return Ok(Value::Object(map));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(ApiError::malformed("json"));
            }
            let key = self.string()?;
            if !seen.insert(key.clone()) {
                return Err(ApiError::malformed("json"));
            }
            if self.request && (CREDENTIALS.contains(&key.as_str()) || WAIVERS.contains(&key.as_str())) {
                return Err(ApiError::malformed("json"));
            }
            if root && WAIVERS.contains(&key.as_str()) {
                return Err(ApiError::malformed("json"));
            }
            self.skip_ws();
            self.bump(b':')?;
            self.skip_ws();
            let child = self.value(depth + 1, false)?;
            map.insert(key, child);
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            self.bump(b'}')?;
            break;
        }
        Ok(Value::Object(map))
    }

    fn array(&mut self, depth: usize) -> Result<Value, ApiError> {
        self.bump(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.eat(b']') {
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth + 1, false)?);
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            self.bump(b']')?;
            break;
        }
        Ok(Value::Array(items))
    }

    fn literal(&mut self, text: &[u8], value: Value) -> Result<Value, ApiError> {
        if self.bytes[self.index..].starts_with(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err(ApiError::malformed("json"))
        }
    }

    fn number(&mut self) -> Result<Value, ApiError> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        let digits = self.index;
        if self.peek() == Some(b'0') {
            self.index += 1;
            if self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                return Err(ApiError::malformed("json"));
            }
        } else if self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.index += 1;
            }
        } else {
            return Err(ApiError::malformed("json"));
        }
        if self.index == digits {
            return Err(ApiError::malformed("json"));
        }
        if self.request {
            if self.peek().is_some_and(|byte| matches!(byte, b'.' | b'e' | b'E' | b'+')) {
                return Err(ApiError::malformed("json"));
            }
        } else if !self.fraction_and_exponent()? {
            return Err(ApiError::malformed("json"));
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| ApiError::malformed("json"))?;
        if self.request {
            let number = if let Ok(value) = text.parse::<i64>() {
                Number::from(value)
            } else if let Ok(value) = text.parse::<u64>() {
                Number::from(value)
            } else {
                return Err(ApiError::malformed("json"));
            };
            return Ok(Value::Number(number));
        }
        text.parse::<Number>().map(Value::Number).map_err(|_| ApiError::malformed("json"))
    }

    fn fraction_and_exponent(&mut self) -> Result<bool, ApiError> {
        if self.peek() == Some(b'.') {
            self.index += 1;
            let start = self.index;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.index += 1;
            }
            if self.index == start {
                return Ok(false);
            }
        }
        if self.peek().is_some_and(|byte| matches!(byte, b'e' | b'E')) {
            self.index += 1;
            if self.peek().is_some_and(|byte| matches!(byte, b'+' | b'-')) {
                self.index += 1;
            }
            let start = self.index;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.index += 1;
            }
            if self.index == start {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn string(&mut self) -> Result<String, ApiError> {
        self.bump(b'"')?;
        let mut out = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(ApiError::malformed("json"));
            };
            match byte {
                b'"' => {
                    self.index += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.index += 1;
                    let Some(escaped) = self.peek() else {
                        return Err(ApiError::malformed("json"));
                    };
                    self.index += 1;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.hex4()?),
                        _ => return Err(ApiError::malformed("json")),
                    }
                }
                0x00..=0x1f => return Err(ApiError::malformed("json")),
                _ => {
                    let rest = &self.bytes[self.index..];
                    let width = utf8_width(rest).ok_or(ApiError::malformed("json"))?;
                    let text = std::str::from_utf8(&rest[..width]).map_err(|_| ApiError::malformed("json"))?;
                    out.push_str(text);
                    self.index += width;
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<char, ApiError> {
        if self.index + 4 > self.bytes.len() {
            return Err(ApiError::malformed("json"));
        }
        let text = std::str::from_utf8(&self.bytes[self.index..self.index + 4]).map_err(|_| ApiError::malformed("json"))?;
        self.index += 4;
        let code = u32::from_str_radix(text, 16).map_err(|_| ApiError::malformed("json"))?;
        char::from_u32(code).ok_or(ApiError::malformed("json"))
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn bump(&mut self, byte: u8) -> Result<(), ApiError> {
        if self.eat(byte) { Ok(()) } else { Err(ApiError::malformed("json")) }
    }
}

fn utf8_width(bytes: &[u8]) -> Option<usize> {
    let first = *bytes.first()?;
    let width = if first < 0x80 {
        1
    } else if first & 0xe0 == 0xc0 {
        2
    } else if first & 0xf0 == 0xe0 {
        3
    } else if first & 0xf8 == 0xf0 {
        4
    } else {
        return None;
    };
    if bytes.len() < width || !bytes[1..width].iter().all(|byte| byte & 0xc0 == 0x80) {
        return None;
    }
    Some(width)
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(bit) => out.push_str(if *bit { "true" } else { "false" }),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                out.push_str(&value.to_string());
            } else if let Some(value) = number.as_u64() {
                out.push_str(&value.to_string());
            } else {
                out.push_str(&serde_json::to_string(number).unwrap_or_else(|_| "0".to_string()));
            }
        }
        Value::String(text) => out.push_str(&serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string()));
                out.push(':');
                write_value(out, &map[*key]);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_document, parse_request};

    #[test]
    fn request_rejects_duplicate_and_waiver_keys() {
        assert!(parse_request(br#"{"a":1,"a":2}"#).is_err());
        assert!(parse_request(br#"{"force":true}"#).is_err());
        assert!(parse_request(br#"{"nested":{"waive":1}}"#).is_err());
        assert!(parse_request(br#"{"n":1.0}"#).is_err());
        assert!(parse_request(br#"{"n":1}"#).is_ok());
    }

    #[test]
    fn document_keeps_nested_authorization() {
        let value = parse_document(br#"{"provenance":{"authorization":"not_claimed"}}"#).expect("doc");
        assert_eq!(value["provenance"]["authorization"], "not_claimed");
        assert!(parse_document(br#"{"force":false,"provenance":{"authorization":"not_claimed"}}"#).is_err());
    }
}
