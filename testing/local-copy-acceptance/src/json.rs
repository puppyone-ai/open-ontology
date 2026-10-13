//! Bounded JSON subset. Duplicate keys and secret-shaped keys are refused.
//! Objects keep source order. Numbers are integers only.

use crate::fail::{fail, Fail};

pub const MAX_DEPTH: usize = 16;
const MAX_KEYS: usize = 256;
const MAX_ARRAY: usize = 100_000;
const MAX_STRING: usize = 1_048_576;

const FORBIDDEN: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "authorization",
    "credential",
    "private_key",
    "ca_pem",
    "ca_body",
    "api_key",
    "token",
];

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Int(i64),
    String(String),
    Array(Vec<Val>),
    Object(Vec<(String, Val)>),
}

pub fn parse(input: &str) -> Result<Val, Fail> {
    let mut parser = Parser { input, index: 0 };
    let value = parser.value(0)?;
    parser.skip();
    if parser.index != input.len() {
        return Err(fail("malformed", "trailing json"));
    }
    Ok(value)
}

struct Parser<'a> {
    input: &'a str,
    index: usize,
}

impl<'a> Parser<'a> {
    fn skip(&mut self) {
        while let Some(byte) = self.input.as_bytes().get(self.index) {
            if byte.is_ascii_whitespace() {
                self.index += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.index).copied()
    }

    fn bump(&mut self) -> Result<u8, Fail> {
        let byte = self
            .peek()
            .ok_or_else(|| fail("malformed", "truncated json"))?;
        self.index += 1;
        Ok(byte)
    }

    fn value(&mut self, depth: usize) -> Result<Val, Fail> {
        self.skip();
        match self.peek() {
            Some(b'n') => self.literal("null", Val::Null),
            Some(b't') => self.literal("true", Val::Bool(true)),
            Some(b'f') => self.literal("false", Val::Bool(false)),
            Some(b'"') => Ok(Val::String(self.string()?)),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            _ => Err(fail("malformed", "expected json value")),
        }
    }

    fn literal(&mut self, text: &str, value: Val) -> Result<Val, Fail> {
        if self.input[self.index..].starts_with(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err(fail("malformed", "bad literal"))
        }
    }

    fn number(&mut self) -> Result<Val, Fail> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.index += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(fail("malformed", "leading zero"));
                }
            }
            Some(b'1'..=b'9') => {
                self.index += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.index += 1;
                }
            }
            _ => return Err(fail("malformed", "expected digit")),
        }
        if matches!(self.peek(), Some(b'.') | Some(b'e') | Some(b'E')) {
            return Err(fail("malformed", "integer only"));
        }
        let text = &self.input[start..self.index];
        if text == "-" || text == "-0" {
            return Err(fail("malformed", "integer"));
        }
        let value = text
            .parse::<i64>()
            .map_err(|_| fail("malformed", "integer range"))?;
        Ok(Val::Int(value))
    }

    fn string(&mut self) -> Result<String, Fail> {
        if self.bump()? != b'"' {
            return Err(fail("malformed", "expected string"));
        }
        let mut out = String::new();
        loop {
            if out.len() > MAX_STRING {
                return Err(fail("bound", "string length"));
            }
            match self.bump()? {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = self.bump()?;
                    match escaped {
                        b'"' | b'\\' | b'/' => out.push(escaped as char),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode()?),
                        _ => return Err(fail("malformed", "bad escape")),
                    }
                }
                byte if byte < 0x20 => return Err(fail("malformed", "raw control")),
                _ => {
                    self.index -= 1;
                    let rest = &self.input[self.index..];
                    let ch = rest
                        .chars()
                        .next()
                        .ok_or_else(|| fail("malformed", "utf8"))?;
                    if ch == '"' || ch == '\\' {
                        return Err(fail("malformed", "string"));
                    }
                    out.push(ch);
                    self.index += ch.len_utf8();
                }
            }
        }
    }

    fn unicode(&mut self) -> Result<char, Fail> {
        let unit = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.bump()? != b'\\' || self.bump()? != b'u' {
                return Err(fail("malformed", "surrogate"));
            }
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(fail("malformed", "surrogate"));
            }
            let code = 0x10000 + (((unit - 0xD800) as u32) << 10) + (low - 0xDC00) as u32;
            return char::from_u32(code).ok_or_else(|| fail("malformed", "unicode"));
        }
        if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(fail("malformed", "surrogate"));
        }
        char::from_u32(unit as u32).ok_or_else(|| fail("malformed", "unicode"))
    }

    fn hex4(&mut self) -> Result<u16, Fail> {
        let mut value = 0_u16;
        for _ in 0..4 {
            let byte = self.bump()?;
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(fail("malformed", "hex")),
            };
            value = (value << 4) | u16::from(digit);
        }
        Ok(value)
    }

    fn array(&mut self, depth: usize) -> Result<Val, Fail> {
        let next = depth
            .checked_add(1)
            .filter(|depth| *depth <= MAX_DEPTH)
            .ok_or_else(|| fail("bound", "depth"))?;
        if self.bump()? != b'[' {
            return Err(fail("malformed", "expected array"));
        }
        self.skip();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.index += 1;
            return Ok(Val::Array(items));
        }
        loop {
            if items.len() >= MAX_ARRAY {
                return Err(fail("bound", "array length"));
            }
            items.push(self.value(next)?);
            self.skip();
            match self.bump()? {
                b']' => return Ok(Val::Array(items)),
                b',' => {
                    self.skip();
                    if self.peek() == Some(b']') {
                        return Err(fail("malformed", "trailing comma"));
                    }
                }
                _ => return Err(fail("malformed", "expected comma")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Val, Fail> {
        let next = depth
            .checked_add(1)
            .filter(|depth| *depth <= MAX_DEPTH)
            .ok_or_else(|| fail("bound", "depth"))?;
        if self.bump()? != b'{' {
            return Err(fail("malformed", "expected object"));
        }
        self.skip();
        let mut pairs = Vec::new();
        if self.peek() == Some(b'}') {
            self.index += 1;
            return Ok(Val::Object(pairs));
        }
        loop {
            if pairs.len() >= MAX_KEYS {
                return Err(fail("bound", "object keys"));
            }
            self.skip();
            if self.peek() != Some(b'"') {
                return Err(fail("malformed", "expected key"));
            }
            let key = self.string()?;
            if FORBIDDEN.iter().any(|name| *name == key) {
                return Err(fail("secret_key", "secret-shaped key"));
            }
            if pairs.iter().any(|(name, _)| name == &key) {
                return Err(fail("duplicate_key", "duplicate key"));
            }
            self.skip();
            if self.bump()? != b':' {
                return Err(fail("malformed", "expected colon"));
            }
            pairs.push((key, self.value(next)?));
            self.skip();
            match self.bump()? {
                b'}' => return Ok(Val::Object(pairs)),
                b',' => {
                    self.skip();
                    if self.peek() == Some(b'}') {
                        return Err(fail("malformed", "trailing comma"));
                    }
                }
                _ => return Err(fail("malformed", "expected comma")),
            }
        }
    }
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out
}
