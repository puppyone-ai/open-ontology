//! JSON structural scan.
//!
//! `serde_json` keeps the last duplicate key. This parser rejects duplicate
//! keys, depths past the budget, and trailing data before a typed model sees
//! the value. Exact amounts still must not be JSON numbers; that rule is
//! enforced by the typed deserializers, not by this scanner.

use serde_json::{Number, Value};

use crate::decimal::{DecimalError, ExactDecimal};
use crate::diagnostic::{Diagnostic, DiagnosticList};
use crate::limits::{DOCUMENT_MAX_BYTES, JSON_MAX_DEPTH, JSON_MAX_NODES};

pub fn parse_json_value(input: &str, contract: &str) -> Result<Value, DiagnosticList> {
    if input.len() > DOCUMENT_MAX_BYTES {
        return Err(DiagnosticList::one(Diagnostic::error(
            contract,
            "json.budget.bytes",
            "/",
            format!(
                "document is {} bytes; limit is {DOCUMENT_MAX_BYTES}",
                input.len()
            ),
        )));
    }
    let mut parser = Parser {
        bytes: input.as_bytes(),
        index: 0,
        line: 1,
        column: 1,
        nodes: 0,
        contract,
        pending_high: None,
    };
    parser.skip();
    let value = parser.value(1)?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err(parser.error("json.syntax", "trailing data after the JSON value"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    line: u32,
    column: u32,
    nodes: usize,
    contract: &'a str,
    pending_high: Option<u16>,
}

impl<'a> Parser<'a> {
    fn value(&mut self, depth: usize) -> Result<Value, DiagnosticList> {
        if depth > JSON_MAX_DEPTH {
            return Err(self.error("json.budget.depth", "JSON nesting exceeds the depth budget"));
        }
        self.nodes += 1;
        if self.nodes > JSON_MAX_NODES {
            return Err(self.error("json.budget.nodes", "JSON node count exceeds the budget"));
        }
        self.skip();
        let Some(byte) = self.peek() else {
            return Err(self.error("json.syntax", "unexpected end of JSON"));
        };
        match byte {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => Ok(Value::String(self.string()?)),
            b't' => self.literal(b"true", Value::Bool(true)),
            b'f' => self.literal(b"false", Value::Bool(false)),
            b'n' => self.literal(b"null", Value::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(self.error("json.syntax", "invalid JSON value")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, DiagnosticList> {
        self.bump();
        let mut map = serde_json::Map::new();
        self.skip();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(Value::Object(map));
        }
        loop {
            self.skip();
            if self.peek() != Some(b'"') {
                return Err(self.error("json.syntax", "object key must be a string"));
            }
            let key = self.string()?;
            self.skip();
            if self.peek() != Some(b':') {
                return Err(self.error("json.syntax", "expected ':' after an object key"));
            }
            self.bump();
            let child = self.value(depth + 1)?;
            if map.contains_key(&key) {
                return Err(self.error("json.duplicate_key", "duplicate JSON object key"));
            }
            map.insert(key, child);
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip();
                    if self.peek() == Some(b'}') {
                        return Err(self.error("json.syntax", "trailing comma"));
                    }
                }
                Some(b'}') => {
                    self.bump();
                    break;
                }
                _ => return Err(self.error("json.syntax", "expected ',' or '}'")),
            }
        }
        Ok(Value::Object(map))
    }

    fn array(&mut self, depth: usize) -> Result<Value, DiagnosticList> {
        self.bump();
        let mut items = Vec::new();
        self.skip();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip();
                    if self.peek() == Some(b']') {
                        return Err(self.error("json.syntax", "trailing comma"));
                    }
                }
                Some(b']') => {
                    self.bump();
                    break;
                }
                _ => return Err(self.error("json.syntax", "expected ',' or ']'")),
            }
        }
        Ok(Value::Array(items))
    }

    fn string(&mut self) -> Result<String, DiagnosticList> {
        self.bump();
        let start_line = self.line;
        let start_col = self.column;
        let mut out = String::new();
        while let Some(byte) = self.peek() {
            if byte == b'"' {
                self.bump();
                if self.pending_high.is_some() {
                    return Err(self.error("json.syntax", "truncated utf-16 surrogate pair"));
                }
                return Ok(out);
            }
            if byte == b'\\' {
                self.bump();
                let Some(escaped) = self.peek() else {
                    return Err(self.error("json.syntax", "truncated string escape"));
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
                        let mut hex = [0u8; 4];
                        for slot in &mut hex {
                            let Some(digit) = self.peek() else {
                                return Err(self.error("json.syntax", "truncated unicode escape"));
                            };
                            if !digit.is_ascii_hexdigit() {
                                return Err(self.error("json.syntax", "invalid unicode escape"));
                            }
                            *slot = digit;
                            self.bump();
                        }
                        let text = std::str::from_utf8(&hex).map_err(|_| {
                            self.error_at(
                                "json.syntax",
                                "invalid unicode escape",
                                start_line,
                                start_col,
                            )
                        })?;
                        let code = u32::from_str_radix(text, 16).map_err(|_| {
                            self.error_at(
                                "json.syntax",
                                "invalid unicode escape",
                                start_line,
                                start_col,
                            )
                        })?;
                        self.push_utf16(code, &mut out, start_line, start_col)?;
                    }
                    _ => return Err(self.error("json.syntax", "invalid string escape")),
                }
                continue;
            }
            if byte < 0x20 {
                return Err(self.error("json.syntax", "raw control in a JSON string"));
            }
            let rest = &self.bytes[self.index..];
            let ch = utf8_char(rest).ok_or_else(|| self.error("json.syntax", "invalid UTF-8"))?;
            out.push(ch);
            for _ in 0..ch.len_utf8() {
                self.bump();
            }
        }
        Err(self.error_at("json.syntax", "unterminated string", start_line, start_col))
    }

    fn number(&mut self) -> Result<Value, DiagnosticList> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        if self.peek() == Some(b'0') {
            self.bump();
        } else if self.peek().is_some_and(|b| b.is_ascii_digit()) {
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.bump();
            }
        } else {
            return Err(self.error("json.syntax", "invalid number"));
        }
        if self.peek() == Some(b'.') {
            self.bump();
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                return Err(self.error("json.syntax", "invalid fraction"));
            }
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.bump();
            }
        }
        if self.peek().is_some_and(|b| b == b'e' || b == b'E') {
            self.bump();
            if self.peek().is_some_and(|b| b == b'+' || b == b'-') {
                self.bump();
            }
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                return Err(self.error("json.syntax", "invalid exponent"));
            }
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.bump();
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index])
            .map_err(|_| self.error("json.syntax", "invalid number"))?;
        if has_leading_zero(text) {
            return Err(self.error("json.syntax", "leading zero in a JSON number"));
        }
        let number = match Number::from_str_exact(text) {
            Ok(number) => number,
            Err(DecimalError::Budget) => {
                return Err(self.error("json.budget.bytes", "JSON number exceeds the digit budget"));
            }
            Err(_) => return Err(self.error("json.syntax", "invalid number")),
        };
        Ok(Value::Number(number))
    }

    fn push_utf16(
        &mut self,
        unit: u32,
        out: &mut String,
        line: u32,
        column: u32,
    ) -> Result<(), DiagnosticList> {
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.pending_high.is_some() {
                return Err(self.error_at("json.syntax", "invalid utf-16 surrogate", line, column));
            }
            let high = u16::try_from(unit).map_err(|_| {
                self.error_at("json.syntax", "invalid utf-16 surrogate", line, column)
            })?;
            self.pending_high = Some(high);
            return Ok(());
        }
        if (0xDC00..=0xDFFF).contains(&unit) {
            let Some(high) = self.pending_high.take() else {
                return Err(self.error_at("json.syntax", "invalid utf-16 surrogate", line, column));
            };
            let scalar = 0x10000 + ((u32::from(high) - 0xD800) << 10) + (unit - 0xDC00);
            let Some(ch) = char::from_u32(scalar) else {
                return Err(self.error_at("json.syntax", "invalid utf-16 surrogate", line, column));
            };
            out.push(ch);
            return Ok(());
        }
        if self.pending_high.is_some() {
            return Err(self.error_at("json.syntax", "invalid utf-16 surrogate", line, column));
        }
        let Some(ch) = char::from_u32(unit) else {
            return Err(self.error_at("json.syntax", "invalid utf-16 surrogate", line, column));
        };
        out.push(ch);
        Ok(())
    }

    fn literal(&mut self, expected: &[u8], value: Value) -> Result<Value, DiagnosticList> {
        for byte in expected {
            if self.peek() != Some(*byte) {
                return Err(self.error("json.syntax", "invalid literal"));
            }
            self.bump();
        }
        Ok(value)
    }

    fn skip(&mut self) {
        while let Some(byte) = self.peek() {
            if byte == b' ' || byte == b'\t' || byte == b'\r' || byte == b'\n' {
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
        if let Some(byte) = self.peek() {
            self.index += 1;
            if byte == b'\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column = self.column.saturating_add(1);
            }
        }
    }

    fn error(&self, name: &'static str, message: &str) -> DiagnosticList {
        DiagnosticList::one(
            Diagnostic::error(self.contract, name, "/", message).at(self.line, self.column),
        )
    }

    fn error_at(
        &self,
        name: &'static str,
        message: &str,
        line: u32,
        column: u32,
    ) -> DiagnosticList {
        DiagnosticList::one(Diagnostic::error(self.contract, name, "/", message).at(line, column))
    }
}

fn has_leading_zero(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    digits.len() > 1 && digits.as_bytes()[0] == b'0' && digits.as_bytes()[1].is_ascii_digit()
}

fn utf8_char(bytes: &[u8]) -> Option<char> {
    let text = std::str::from_utf8(bytes).ok()?;
    text.chars().next()
}

trait NumberExact {
    fn from_str_exact(text: &str) -> Result<Number, DecimalError>;
}

impl NumberExact for Number {
    fn from_str_exact(text: &str) -> Result<Number, DecimalError> {
        let parsed = ExactDecimal::parse_json_number(text)?;
        if !text.contains(['.', 'e', 'E']) {
            if let Ok(value) = text.parse::<u64>() {
                return Ok(Number::from(value));
            }
            if let Ok(value) = text.parse::<i64>() {
                return Ok(Number::from(value));
            }
        }
        let _ = parsed;
        Ok(Number::from_string_unchecked(text.to_owned()))
    }
}
