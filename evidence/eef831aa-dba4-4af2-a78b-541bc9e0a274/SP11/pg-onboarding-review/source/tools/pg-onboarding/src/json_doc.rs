//! Bounded JSON visitor.
//!
//! `serde_json` keeps the last duplicate key. This visitor rejects a repeated
//! key, a document past the byte cap, nesting past 32, more than 50000 values,
//! and trailing non-whitespace bytes. Only a document that passes is handed to
//! `serde_json` for the value.

#![forbid(unsafe_code)]

use std::collections::HashSet;

use serde_json::Value;

pub const OPERATOR_JSON_MAX: usize = 1_048_576;
pub const JSON_MAX_DEPTH: usize = 32;
pub const JSON_MAX_NODES: usize = 50_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonFault {
    Budget,
    Shape,
}

impl JsonFault {
    pub fn detail(self) -> &'static str {
        match self {
            JsonFault::Budget => "budget",
            JsonFault::Shape => "shape",
        }
    }
}

pub fn parse(bytes: &[u8], max_bytes: usize) -> Result<Value, JsonFault> {
    if bytes.len() > max_bytes {
        return Err(JsonFault::Budget);
    }
    let mut parser = Parser { bytes, index: 0, nodes: 0 };
    parser.skip();
    parser.value(1)?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err(JsonFault::Shape);
    }
    serde_json::from_slice(bytes).map_err(|_| JsonFault::Shape)
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    nodes: usize,
}

impl<'a> Parser<'a> {
    fn value(&mut self, depth: usize) -> Result<(), JsonFault> {
        if depth > JSON_MAX_DEPTH {
            return Err(JsonFault::Budget);
        }
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > JSON_MAX_NODES {
            return Err(JsonFault::Budget);
        }
        self.skip();
        let Some(byte) = self.peek() else {
            return Err(JsonFault::Shape);
        };
        match byte {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => {
                let _ = self.string()?;
                Ok(())
            }
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'n' => self.literal(b"null"),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(JsonFault::Shape),
        }
    }

    fn object(&mut self, depth: usize) -> Result<(), JsonFault> {
        self.bump();
        self.skip();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(());
        }
        let mut keys = HashSet::new();
        loop {
            self.skip();
            if self.peek() != Some(b'"') {
                return Err(JsonFault::Shape);
            }
            let key = self.string()?;
            if !keys.insert(key) {
                return Err(JsonFault::Shape);
            }
            self.skip();
            if self.peek() != Some(b':') {
                return Err(JsonFault::Shape);
            }
            self.bump();
            self.value(depth + 1)?;
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip();
                    if self.peek() == Some(b'}') {
                        return Err(JsonFault::Shape);
                    }
                }
                Some(b'}') => {
                    self.bump();
                    break;
                }
                _ => return Err(JsonFault::Shape),
            }
        }
        Ok(())
    }

    fn array(&mut self, depth: usize) -> Result<(), JsonFault> {
        self.bump();
        self.skip();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(());
        }
        loop {
            self.value(depth + 1)?;
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip();
                    if self.peek() == Some(b']') {
                        return Err(JsonFault::Shape);
                    }
                }
                Some(b']') => {
                    self.bump();
                    break;
                }
                _ => return Err(JsonFault::Shape),
            }
        }
        Ok(())
    }

    fn string(&mut self) -> Result<String, JsonFault> {
        self.bump();
        let mut out = String::new();
        let mut pending_high = None;
        loop {
            let Some(byte) = self.peek() else {
                return Err(JsonFault::Shape);
            };
            if byte == b'"' {
                self.bump();
                if pending_high.is_some() {
                    return Err(JsonFault::Shape);
                }
                return Ok(out);
            }
            if byte == b'\\' {
                self.bump();
                let Some(escaped) = self.peek() else {
                    return Err(JsonFault::Shape);
                };
                self.bump();
                match escaped {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let unit = self.hex4()?;
                        push_utf16(unit, &mut out, &mut pending_high)?;
                    }
                    _ => return Err(JsonFault::Shape),
                }
                continue;
            }
            if byte < 0x20 {
                return Err(JsonFault::Shape);
            }
            let rest = &self.bytes[self.index..];
            let text = std::str::from_utf8(rest).map_err(|_| JsonFault::Shape)?;
            let Some(ch) = text.chars().next() else {
                return Err(JsonFault::Shape);
            };
            out.push(ch);
            for _ in 0..ch.len_utf8() {
                self.bump();
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonFault> {
        let mut hex = [0u8; 4];
        for slot in &mut hex {
            let Some(digit) = self.peek() else {
                return Err(JsonFault::Shape);
            };
            if !digit.is_ascii_hexdigit() {
                return Err(JsonFault::Shape);
            }
            *slot = digit;
            self.bump();
        }
        let text = std::str::from_utf8(&hex).map_err(|_| JsonFault::Shape)?;
        u32::from_str_radix(text, 16).map_err(|_| JsonFault::Shape)
    }

    fn number(&mut self) -> Result<(), JsonFault> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        if self.peek() == Some(b'0') {
            self.bump();
        } else if self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.bump();
            }
        } else {
            return Err(JsonFault::Shape);
        }
        if self.peek() == Some(b'.') {
            self.bump();
            if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                return Err(JsonFault::Shape);
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.bump();
            }
        }
        if self.peek().is_some_and(|byte| matches!(byte, b'e' | b'E')) {
            self.bump();
            if self.peek().is_some_and(|byte| matches!(byte, b'+' | b'-')) {
                self.bump();
            }
            if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                return Err(JsonFault::Shape);
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.bump();
            }
        }
        let token = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| JsonFault::Shape)?;
        if leading_zero(token) {
            return Err(JsonFault::Shape);
        }
        let parsed: Value = serde_json::from_str(token).map_err(|_| JsonFault::Shape)?;
        if !parsed.is_number() {
            return Err(JsonFault::Shape);
        }
        Ok(())
    }

    fn literal(&mut self, expected: &[u8]) -> Result<(), JsonFault> {
        for byte in expected {
            if self.peek() != Some(*byte) {
                return Err(JsonFault::Shape);
            }
            self.bump();
        }
        Ok(())
    }

    fn skip(&mut self) {
        while let Some(byte) = self.peek() {
            if matches!(byte, b' ' | b'\t' | b'\r' | b'\n') {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn bump(&mut self) {
        if self.peek().is_some() {
            self.index += 1;
        }
    }
}

fn push_utf16(unit: u32, out: &mut String, pending_high: &mut Option<u16>) -> Result<(), JsonFault> {
    if (0xD800..=0xDBFF).contains(&unit) {
        if pending_high.is_some() {
            return Err(JsonFault::Shape);
        }
        let high = u16::try_from(unit).map_err(|_| JsonFault::Shape)?;
        *pending_high = Some(high);
        return Ok(());
    }
    if (0xDC00..=0xDFFF).contains(&unit) {
        let Some(high) = pending_high.take() else {
            return Err(JsonFault::Shape);
        };
        let scalar = 0x10000 + ((u32::from(high) - 0xD800) << 10) + (unit - 0xDC00);
        let Some(ch) = char::from_u32(scalar) else {
            return Err(JsonFault::Shape);
        };
        out.push(ch);
        return Ok(());
    }
    if pending_high.is_some() {
        return Err(JsonFault::Shape);
    }
    let Some(ch) = char::from_u32(unit) else {
        return Err(JsonFault::Shape);
    };
    out.push(ch);
    Ok(())
}

fn leading_zero(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let bytes = digits.as_bytes();
    bytes.len() > 1 && bytes[0] == b'0' && bytes[1].is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::{parse, JsonFault, JSON_MAX_NODES, OPERATOR_JSON_MAX};

    #[test]
    fn duplicate_root_nested_and_escaped_keys_refuse() {
        let err = parse(br#"{"a":1,"a":2}"#, 64).unwrap_err();
        assert_eq!(err, JsonFault::Shape);
        let err = parse(br#"{"outer":{"source_cut":1,"source_cut":2}}"#, 128).unwrap_err();
        assert_eq!(err, JsonFault::Shape);
        let err = parse(br#"{"roles":[{"slug":"clerk","slug":"peek"}]}"#, 128).unwrap_err();
        assert_eq!(err, JsonFault::Shape);
        let err = parse(br#"{"a":1,"\u0061":2}"#, 64).unwrap_err();
        assert_eq!(err, JsonFault::Shape);
        let err = parse(br#"[{"keep":true,"keep":false}]"#, 64).unwrap_err();
        assert_eq!(err, JsonFault::Shape);
    }

    #[test]
    fn unique_document_keeps_every_key() {
        let value = parse(br#"{"a":1,"b":[true,null],"c":"ok"}"#, 64).unwrap();
        assert_eq!(value["a"].as_i64(), Some(1));
        assert_eq!(value["b"][0].as_bool(), Some(true));
        assert!(value["b"][1].is_null());
        assert_eq!(value["c"].as_str(), Some("ok"));
        let escaped = parse(br#""\uD800\uDC00""#, 32).unwrap();
        assert_eq!(escaped.as_str(), Some("\u{10000}"));
    }

    #[test]
    fn trailing_bytes_and_whitespace() {
        assert_eq!(parse(b"{}{}", 16).unwrap_err(), JsonFault::Shape);
        assert_eq!(parse(b"{} x", 16).unwrap_err(), JsonFault::Shape);
        let value = parse(b"{}\n", 16).unwrap();
        assert!(value.as_object().unwrap().is_empty());
    }

    #[test]
    fn depth_and_node_and_byte_budgets() {
        assert!(parse(&wrapped(31), 4096).is_ok());
        assert_eq!(parse(&wrapped(32), 4096).unwrap_err(), JsonFault::Budget);
        assert!(parse(&nulls(JSON_MAX_NODES - 1), OPERATOR_JSON_MAX).is_ok());
        assert_eq!(
            parse(&nulls(JSON_MAX_NODES), OPERATOR_JSON_MAX).unwrap_err(),
            JsonFault::Budget
        );
        let mut at_cap = vec![b' '; OPERATOR_JSON_MAX];
        assert_eq!(parse(&at_cap, OPERATOR_JSON_MAX).unwrap_err(), JsonFault::Shape);
        at_cap.push(b' ');
        assert_eq!(parse(&at_cap, OPERATOR_JSON_MAX).unwrap_err(), JsonFault::Budget);
    }

    fn wrapped(objects: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for _ in 0..objects {
            out.extend_from_slice(br#"{"a":"#);
        }
        out.push(b'0');
        out.resize(out.len() + objects, b'}');
        out
    }

    fn nulls(count: usize) -> Vec<u8> {
        let mut out = Vec::from(&b"["[..]);
        for index in 0..count {
            if index > 0 {
                out.push(b',');
            }
            out.extend_from_slice(b"null");
        }
        out.push(b']');
        out
    }
}
