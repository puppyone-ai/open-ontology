//! Canonical JSON object. Keys are sorted by raw UTF-8. Numbers are exact
//! decimals with trailing fractional zeros removed, so `1`, `1.0`, and `1e0`
//! encode the same bytes. Arrays keep order. Duplicate keys are rejected.

use std::collections::HashSet;

use crate::decimal::{DecimalError, ExactDecimal};
use crate::limits::{
    CANONICAL_MAX_ARRAY_ELEMENTS, CANONICAL_MAX_COEFFICIENT_DIGITS, CANONICAL_MAX_DEPTH,
    CANONICAL_MAX_FIELD_NAME_BYTES, CANONICAL_MAX_VALUE_BYTES, JSON_MAX_NODES,
};

use super::{Buf, CanonicalError};

const JSON_NULL: u8 = 1;
const JSON_BOOL: u8 = 2;
const JSON_NUMBER: u8 = 3;
const JSON_STRING: u8 = 4;
const JSON_ARRAY: u8 = 5;
const JSON_OBJECT: u8 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonNode {
    Null,
    Bool(bool),
    Number(ExactDecimal),
    String(String),
    Array(Vec<JsonNode>),
    Object(Vec<(String, JsonNode)>),
}

pub fn parse_json(input: &str) -> Result<JsonNode, CanonicalError> {
    if input.len() > CANONICAL_MAX_VALUE_BYTES {
        return Err(CanonicalError::Budget);
    }
    let mut parser = Parser {
        bytes: input.as_bytes(),
        index: 0,
        nodes: 0,
        pending_high: None,
    };
    parser.skip();
    let node = parser.node(1)?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err(CanonicalError::InvalidValue);
    }
    Ok(node)
}

pub(super) fn write_json(
    buf: &mut Buf,
    node: &JsonNode,
    depth: usize,
) -> Result<(), CanonicalError> {
    check_json(node, depth, &mut 0)?;
    write_checked(buf, node)
}

fn check_json(node: &JsonNode, depth: usize, nodes: &mut usize) -> Result<(), CanonicalError> {
    if depth > CANONICAL_MAX_DEPTH {
        return Err(CanonicalError::Depth);
    }
    *nodes = nodes.saturating_add(1);
    if *nodes > JSON_MAX_NODES {
        return Err(CanonicalError::Budget);
    }
    match node {
        JsonNode::Null | JsonNode::Bool(_) => Ok(()),
        JsonNode::Number(value) => {
            let value = value.clone().normalize_json();
            if value.coefficient().len() > CANONICAL_MAX_COEFFICIENT_DIGITS {
                Err(CanonicalError::Budget)
            } else {
                Ok(())
            }
        }
        JsonNode::String(value) => {
            if value.len() > CANONICAL_MAX_VALUE_BYTES {
                Err(CanonicalError::Budget)
            } else {
                Ok(())
            }
        }
        JsonNode::Array(items) => {
            if items.len() > CANONICAL_MAX_ARRAY_ELEMENTS {
                return Err(CanonicalError::Budget);
            }
            for item in items {
                check_json(item, depth + 1, nodes)?;
            }
            Ok(())
        }
        JsonNode::Object(items) => {
            if items.len() > JSON_MAX_NODES {
                return Err(CanonicalError::Budget);
            }
            let mut names: Vec<&str> = items.iter().map(|(key, _)| key.as_str()).collect();
            names.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
            for pair in names.windows(2) {
                if pair[0] == pair[1] {
                    return Err(CanonicalError::InvalidValue);
                }
            }
            for (key, value) in items {
                if key.is_empty() || key.len() > CANONICAL_MAX_FIELD_NAME_BYTES {
                    return Err(CanonicalError::Budget);
                }
                check_json(value, depth + 1, nodes)?;
            }
            Ok(())
        }
    }
}

fn write_checked(buf: &mut Buf, node: &JsonNode) -> Result<(), CanonicalError> {
    match node {
        JsonNode::Null => buf.u8(JSON_NULL),
        JsonNode::Bool(value) => {
            buf.u8(JSON_BOOL)?;
            buf.u8(u8::from(*value))
        }
        JsonNode::Number(value) => {
            let value = value.clone().normalize_json();
            buf.u8(JSON_NUMBER)?;
            let scale = i16::try_from(value.scale()).map_err(|_| CanonicalError::InvalidValue)?;
            buf.i16be(scale)?;
            buf.u8(u8::from(value.is_negative()))?;
            buf.len_bytes(value.coefficient().as_bytes())
        }
        JsonNode::String(value) => {
            buf.u8(JSON_STRING)?;
            buf.len_bytes(value.as_bytes())
        }
        JsonNode::Array(items) => {
            buf.u8(JSON_ARRAY)?;
            buf.u32be(u32::try_from(items.len()).map_err(|_| CanonicalError::Budget)?)?;
            for item in items {
                write_checked(buf, item)?;
            }
            Ok(())
        }
        JsonNode::Object(items) => {
            let mut order: Vec<usize> = (0..items.len()).collect();
            order.sort_by(|&left, &right| items[left].0.as_bytes().cmp(items[right].0.as_bytes()));
            buf.u8(JSON_OBJECT)?;
            buf.u32be(u32::try_from(items.len()).map_err(|_| CanonicalError::Budget)?)?;
            for index in order {
                let (key, value) = &items[index];
                buf.len_bytes(key.as_bytes())?;
                write_checked(buf, value)?;
            }
            Ok(())
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    nodes: usize,
    pending_high: Option<u16>,
}

impl<'a> Parser<'a> {
    fn node(&mut self, depth: usize) -> Result<JsonNode, CanonicalError> {
        if depth > CANONICAL_MAX_DEPTH {
            return Err(CanonicalError::Depth);
        }
        self.nodes += 1;
        if self.nodes > JSON_MAX_NODES {
            return Err(CanonicalError::Budget);
        }
        self.skip();
        let byte = self.peek()?;
        match byte {
            b'n' => self.literal(b"null", JsonNode::Null),
            b't' => self.literal(b"true", JsonNode::Bool(true)),
            b'f' => self.literal(b"false", JsonNode::Bool(false)),
            b'"' => Ok(JsonNode::String(self.string()?)),
            b'[' => self.array(depth),
            b'{' => self.object(depth),
            b'-' | b'0'..=b'9' => Ok(JsonNode::Number(self.number()?)),
            _ => Err(CanonicalError::InvalidValue),
        }
    }

    fn literal(&mut self, text: &[u8], node: JsonNode) -> Result<JsonNode, CanonicalError> {
        if self.bytes.get(self.index..self.index + text.len()) != Some(text) {
            return Err(CanonicalError::InvalidValue);
        }
        self.index += text.len();
        Ok(node)
    }

    fn array(&mut self, depth: usize) -> Result<JsonNode, CanonicalError> {
        self.bump();
        let mut items = Vec::new();
        self.skip();
        if self.peek()? == b']' {
            self.bump();
            return Ok(JsonNode::Array(items));
        }
        loop {
            items.push(self.node(depth + 1)?);
            self.skip();
            match self.peek()? {
                b']' => {
                    self.bump();
                    return Ok(JsonNode::Array(items));
                }
                b',' => self.bump(),
                _ => return Err(CanonicalError::InvalidValue),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<JsonNode, CanonicalError> {
        self.bump();
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        self.skip();
        if self.peek()? == b'}' {
            self.bump();
            return Ok(JsonNode::Object(items));
        }
        loop {
            self.skip();
            if self.peek()? != b'"' {
                return Err(CanonicalError::InvalidValue);
            }
            let key = self.string()?;
            self.skip();
            if self.peek()? != b':' {
                return Err(CanonicalError::InvalidValue);
            }
            self.bump();
            let value = self.node(depth + 1)?;
            if !seen.insert(key.clone()) {
                return Err(CanonicalError::InvalidValue);
            }
            items.push((key, value));
            self.skip();
            match self.peek()? {
                b'}' => {
                    self.bump();
                    items.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
                    return Ok(JsonNode::Object(items));
                }
                b',' => self.bump(),
                _ => return Err(CanonicalError::InvalidValue),
            }
        }
    }

    fn number(&mut self) -> Result<ExactDecimal, CanonicalError> {
        let start = self.index;
        if self.peek()? == b'-' {
            self.bump();
        }
        let digits_start = self.index;
        if self.peek()? == b'0' {
            self.bump();
        } else if self.peek()?.is_ascii_digit() {
            while self.peek().is_ok_and(|byte| byte.is_ascii_digit()) {
                self.bump();
            }
        } else {
            return Err(CanonicalError::InvalidValue);
        }
        if self.index == digits_start {
            return Err(CanonicalError::InvalidValue);
        }
        if self.peek() == Ok(b'.') {
            self.bump();
            let fraction = self.index;
            while self.peek().is_ok_and(|byte| byte.is_ascii_digit()) {
                self.bump();
            }
            if self.index == fraction {
                return Err(CanonicalError::InvalidValue);
            }
        }
        if matches!(self.peek(), Ok(b'e' | b'E')) {
            self.bump();
            if matches!(self.peek(), Ok(b'+' | b'-')) {
                self.bump();
            }
            let exponent = self.index;
            while self.peek().is_ok_and(|byte| byte.is_ascii_digit()) {
                self.bump();
            }
            if self.index == exponent {
                return Err(CanonicalError::InvalidValue);
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index])
            .map_err(|_| CanonicalError::InvalidValue)?;
        match ExactDecimal::parse_json_number(text).map(ExactDecimal::normalize_json) {
            Ok(value) => Ok(value),
            Err(DecimalError::Budget) => Err(CanonicalError::Budget),
            Err(_) => Err(CanonicalError::InvalidValue),
        }
    }

    fn string(&mut self) -> Result<String, CanonicalError> {
        self.bump();
        let mut out = String::new();
        loop {
            let byte = self.peek()?;
            match byte {
                b'"' => {
                    self.bump();
                    if self.pending_high.is_some() {
                        return Err(CanonicalError::InvalidValue);
                    }
                    return Ok(out);
                }
                b'\\' => {
                    self.bump();
                    match self.peek()? {
                        _ if self.pending_high.is_some() && self.peek()? != b'u' => {
                            return Err(CanonicalError::InvalidValue);
                        }
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            self.bump();
                            let unit = self.utf16_unit()?;
                            if let Some(scalar) = self.push_utf16(unit, &mut out)? {
                                let _ = scalar;
                            }
                            continue;
                        }
                        _ => return Err(CanonicalError::InvalidValue),
                    }
                    self.bump();
                }
                0x00..=0x1F => return Err(CanonicalError::InvalidValue),
                _ => {
                    if self.pending_high.is_some() {
                        return Err(CanonicalError::InvalidValue);
                    }
                    let ch = self.next_char()?;
                    out.push(ch);
                }
            }
        }
    }

    fn utf16_unit(&mut self) -> Result<u32, CanonicalError> {
        let mut hex = [0u8; 4];
        for slot in &mut hex {
            let byte = self.peek()?;
            if !byte.is_ascii_hexdigit() {
                return Err(CanonicalError::InvalidValue);
            }
            *slot = byte;
            self.bump();
        }
        let text = std::str::from_utf8(&hex).map_err(|_| CanonicalError::InvalidValue)?;
        u32::from_str_radix(text, 16).map_err(|_| CanonicalError::InvalidValue)
    }

    fn push_utf16(&mut self, unit: u32, out: &mut String) -> Result<Option<char>, CanonicalError> {
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.pending_high.is_some() {
                return Err(CanonicalError::InvalidValue);
            }
            self.pending_high =
                Some(u16::try_from(unit).map_err(|_| CanonicalError::InvalidValue)?);
            return Ok(None);
        }
        if (0xDC00..=0xDFFF).contains(&unit) {
            let Some(high) = self.pending_high.take() else {
                return Err(CanonicalError::InvalidValue);
            };
            let scalar = 0x10000 + ((u32::from(high) - 0xD800) << 10) + (unit - 0xDC00);
            let ch = char::from_u32(scalar).ok_or(CanonicalError::InvalidValue)?;
            out.push(ch);
            return Ok(Some(ch));
        }
        if self.pending_high.is_some() {
            return Err(CanonicalError::InvalidValue);
        }
        let ch = char::from_u32(unit).ok_or(CanonicalError::InvalidValue)?;
        out.push(ch);
        Ok(Some(ch))
    }

    fn next_char(&mut self) -> Result<char, CanonicalError> {
        let rest = &self.bytes[self.index..];
        let text = std::str::from_utf8(rest).map_err(|_| CanonicalError::InvalidValue)?;
        let mut chars = text.chars();
        let ch = chars.next().ok_or(CanonicalError::InvalidValue)?;
        self.index += ch.len_utf8();
        Ok(ch)
    }

    fn skip(&mut self) {
        while self.peek().is_ok_and(|byte| byte.is_ascii_whitespace()) {
            self.bump();
        }
    }

    fn peek(&self) -> Result<u8, CanonicalError> {
        self.bytes
            .get(self.index)
            .copied()
            .ok_or(CanonicalError::InvalidValue)
    }

    fn bump(&mut self) {
        self.index += 1;
    }
}
