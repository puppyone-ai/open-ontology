//! One JSON document. Duplicate keys and credential fields are refused.
//! `serde_json` is not the duplicate-key authority.

use std::collections::HashSet;
use std::str::FromStr;

use serde_json::{Map, Number, Value};

use crate::error::ApiError;

const MAX_BYTES: usize = 1_048_576;
const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 10_000;

const FORBIDDEN: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "authorization",
    "credential",
    "private_key",
    "ca_pem",
    "api_key",
];

pub fn parse_exact(bytes: &[u8]) -> Result<Value, ApiError> {
    parse(bytes, true)
}

/// Producer outcome documents use `authorization` as a non-claim. Request bodies still refuse that key.
pub fn parse_producer_document(bytes: &[u8]) -> Result<Value, ApiError> {
    parse(bytes, false)
}

fn parse(bytes: &[u8], refuse_names: bool) -> Result<Value, ApiError> {
    if bytes.len() > MAX_BYTES {
        return Err(ApiError::too_large());
    }
    let mut parser = Parser { bytes, index: 0, nodes: 0, refuse_names };
    parser.skip_ws();
    let value = parser.value(1, None)?;
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
    refuse_names: bool,
}

impl<'a> Parser<'a> {
    fn value(&mut self, depth: usize, parent_key: Option<&str>) -> Result<Value, ApiError> {
        if depth > MAX_DEPTH || self.nodes >= MAX_NODES {
            return Err(ApiError::malformed("json"));
        }
        self.nodes += 1;
        self.skip_ws();
        let Some(byte) = self.peek() else {
            return Err(ApiError::malformed("json"));
        };
        match byte {
            b'{' => self.object(depth, parent_key),
            b'[' => self.array(depth),
            b'"' => Ok(Value::String(self.string()?)),
            b't' => self.literal(b"true", Value::Bool(true)),
            b'f' => self.literal(b"false", Value::Bool(false)),
            b'n' => self.literal(b"null", Value::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(ApiError::malformed("json")),
        }
    }

    fn object(&mut self, depth: usize, parent_key: Option<&str>) -> Result<Value, ApiError> {
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
            if self.refuse_names && FORBIDDEN.contains(&key.as_str()) {
                return Err(ApiError::malformed("credential"));
            }
            if self.refuse_names && key == "token" && parent_key != Some("null") {
                return Err(ApiError::malformed("credential"));
            }
            self.skip_ws();
            self.bump(b':')?;
            self.skip_ws();
            let child = self.value(depth + 1, Some(key.as_str()))?;
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
            items.push(self.value(depth + 1, None)?);
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            self.bump(b']')?;
            break;
        }
        Ok(Value::Array(items))
    }

    fn string(&mut self) -> Result<String, ApiError> {
        self.bump(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self.next().ok_or_else(|| ApiError::malformed("json"))?;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = self.next().ok_or_else(|| ApiError::malformed("json"))?;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode()?),
                        _ => return Err(ApiError::malformed("json")),
                    }
                }
                0x00..=0x1f => return Err(ApiError::malformed("json")),
                _ => {
                    let width = utf8_width(byte).ok_or_else(|| ApiError::malformed("json"))?;
                    let start = self.index - 1;
                    if self.index + width - 1 > self.bytes.len() {
                        return Err(ApiError::malformed("json"));
                    }
                    self.index += width - 1;
                    let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| ApiError::malformed("json"))?;
                    out.push_str(text);
                }
            }
        }
    }

    fn unicode(&mut self) -> Result<char, ApiError> {
        let unit = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.bytes.get(self.index..self.index + 2) != Some(br"\u") {
                return Err(ApiError::malformed("json"));
            }
            self.index += 2;
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(ApiError::malformed("json"));
            }
            let code = 0x10000 + (((unit - 0xD800) as u32) << 10) + (low - 0xDC00) as u32;
            return char::from_u32(code).ok_or_else(|| ApiError::malformed("json"));
        }
        if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(ApiError::malformed("json"));
        }
        char::from_u32(unit as u32).ok_or_else(|| ApiError::malformed("json"))
    }

    fn hex4(&mut self) -> Result<u16, ApiError> {
        let mut value = 0_u16;
        for _ in 0..4 {
            let byte = self.next().ok_or_else(|| ApiError::malformed("json"))?;
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(ApiError::malformed("json")),
            };
            value = (value << 4) | u16::from(digit);
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Value, ApiError> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        match self.peek() {
            Some(b'0') => self.index += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.index += 1;
                }
            }
            _ => return Err(ApiError::malformed("json")),
        }
        if self.peek() == Some(b'.') {
            self.index += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(ApiError::malformed("json"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(ApiError::malformed("json"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| ApiError::malformed("json"))?;
        let number = Number::from_str(text).map_err(|_| ApiError::malformed("json"))?;
        Ok(Value::Number(number))
    }

    fn literal(&mut self, expected: &[u8], value: Value) -> Result<Value, ApiError> {
        if self.bytes.get(self.index..self.index + expected.len()) != Some(expected) {
            return Err(ApiError::malformed("json"));
        }
        self.index += expected.len();
        Ok(value)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.index += 1;
        Some(byte)
    }

    fn eat(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn bump(&mut self, expected: u8) -> Result<(), ApiError> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(ApiError::malformed("json"))
        }
    }
}

fn utf8_width(byte: u8) -> Option<usize> {
    if byte < 0x80 {
        Some(1)
    } else if byte & 0xE0 == 0xC0 {
        Some(2)
    } else if byte & 0xF0 == 0xE0 {
        Some(3)
    } else if byte & 0xF8 == 0xF0 {
        Some(4)
    } else {
        None
    }
}

pub fn object<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, ApiError> {
    let map = value.as_object().ok_or_else(|| ApiError::malformed("json"))?;
    if map.len() != keys.len() || keys.iter().any(|key| !map.contains_key(*key)) {
        return Err(ApiError::malformed("json"));
    }
    Ok(map)
}

pub fn allow_keys(value: &Value, keys: &[&str]) -> Result<(), ApiError> {
    let map = value.as_object().ok_or_else(|| ApiError::malformed("json"))?;
    if map.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(ApiError::malformed("json"));
    }
    Ok(())
}

pub fn require_str<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, ApiError> {
    map.get(key).and_then(Value::as_str).ok_or_else(|| ApiError::malformed("json"))
}

pub fn positive_i64(value: &Value) -> Result<i64, ApiError> {
    let number = value.as_i64().filter(|number| *number >= 1).ok_or_else(|| ApiError::malformed("json"))?;
    if value.as_u64().is_none() {
        return Err(ApiError::malformed("json"));
    }
    Ok(number)
}

pub fn bounded_u64(value: &Value, min: u64, max: u64) -> Result<u64, ApiError> {
    let number = value.as_u64().ok_or_else(|| ApiError::malformed("json"))?;
    if number < min || number > max {
        return Err(ApiError::closed("budget"));
    }
    Ok(number)
}

#[cfg(test)]
mod tests {
    use super::{parse_exact, parse_producer_document};

    #[test]
    fn duplicate_key_is_malformed() {
        let error = parse_exact(br#"{"a":1,"a":2}"#).expect_err("duplicate");
        assert_eq!(error.detail, "json");
    }

    #[test]
    fn password_key_is_malformed() {
        let error = parse_exact(br#"{"password":"x"}"#).expect_err("password");
        assert_eq!(error.code, "malformed");
        assert_eq!(error.detail, "credential");
    }

    #[test]
    fn secret_ref_is_not_a_password_key() {
        let value = parse_exact(br#"{"secret_ref":"secret://pg/a383/reader"}"#).expect("secret ref");
        assert_eq!(value["secret_ref"], "secret://pg/a383/reader");
    }

    #[test]
    fn producer_outcome_keeps_the_authorization_non_claim() {
        let raw = br#"{"authorization":"not_claimed","status":"awaiting_review"}"#;
        assert!(parse_exact(raw).is_err());
        let value = parse_producer_document(raw).expect("outcome");
        assert_eq!(value["authorization"], "not_claimed");
        assert_eq!(value["status"], "awaiting_review");
    }

    #[test]
    fn null_token_is_allowed_only_under_null() {
        assert!(parse_exact(br#"{"profile":{"null":{"token":"na"}}}"#).is_ok());
        assert!(parse_exact(br#"{"token":"na"}"#).is_err());
    }
}
