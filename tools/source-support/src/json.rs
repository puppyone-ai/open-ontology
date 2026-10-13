//! Closed JSON. Duplicate keys and credential names are refused. Values are not logged by this parser.

use std::fmt::Write as _;

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

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub detail: &'static str,
}

impl Json {
    pub fn object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(items) => Some(items),
            _ => None,
        }
    }

    pub fn get<'a>(&'a self, key: &str) -> Option<&'a Json> {
        self.object()?.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    pub fn string(&self) -> Option<&str> {
        match self {
            Json::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn int(&self) -> Option<i64> {
        match self {
            Json::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }
}

pub fn parse(bytes: &[u8]) -> Result<Json, ParseError> {
    if bytes.len() > MAX_BYTES {
        return Err(ParseError { detail: "bytes" });
    }
    let mut parser = Parser { bytes, index: 0, nodes: 0 };
    parser.skip_ws();
    let value = parser.value(1)?;
    parser.skip_ws();
    if parser.index != parser.bytes.len() {
        return Err(ParseError { detail: "json" });
    }
    Ok(value)
}

pub fn stringify(value: &Json) -> String {
    let mut out = String::new();
    write_json(&mut out, value);
    out
}

fn write_json(out: &mut String, value: &Json) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Int(number) => {
            let _ = write!(out, "{number}");
        }
        Json::String(text) => write_string(out, text),
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json(out, item);
            }
            out.push(']');
        }
        Json::Object(items) => {
            out.push('{');
            for (index, (key, item)) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_json(out, item);
            }
            out.push('}');
        }
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for char in text.chars() {
        match char {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", char as u32);
            }
            _ => out.push(char),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    nodes: usize,
}

impl<'a> Parser<'a> {
    fn value(&mut self, depth: usize) -> Result<Json, ParseError> {
        if depth > MAX_DEPTH || self.nodes >= MAX_NODES {
            return Err(ParseError { detail: "json" });
        }
        self.nodes += 1;
        self.skip_ws();
        let Some(byte) = self.peek() else {
            return Err(ParseError { detail: "json" });
        };
        match byte {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => Ok(Json::String(self.string()?)),
            b't' => self.literal(b"true", Json::Bool(true)),
            b'f' => self.literal(b"false", Json::Bool(false)),
            b'n' => self.literal(b"null", Json::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(ParseError { detail: "json" }),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.bump(b'{')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.eat(b'}') {
            return Ok(Json::Object(items));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(ParseError { detail: "json" });
            }
            let key = self.string()?;
            if items.iter().any(|(name, _)| name == &key) {
                return Err(ParseError { detail: "json" });
            }
            if FORBIDDEN.contains(&key.as_str()) || key == "token" {
                return Err(ParseError { detail: "credential" });
            }
            self.skip_ws();
            self.bump(b':')?;
            let child = self.value(depth + 1)?;
            items.push((key, child));
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            self.bump(b'}')?;
            break;
        }
        Ok(Json::Object(items))
    }

    fn array(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.bump(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.eat(b']') {
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            self.bump(b']')?;
            break;
        }
        Ok(Json::Array(items))
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.bump(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self.next().ok_or(ParseError { detail: "json" })?;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = self.next().ok_or(ParseError { detail: "json" })?;
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
                        _ => return Err(ParseError { detail: "json" }),
                    }
                }
                0x00..=0x1f => return Err(ParseError { detail: "json" }),
                _ => {
                    let width = utf8_width(byte).ok_or(ParseError { detail: "json" })?;
                    let start = self.index - 1;
                    if self.index + width - 1 > self.bytes.len() {
                        return Err(ParseError { detail: "json" });
                    }
                    self.index += width - 1;
                    let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| ParseError { detail: "json" })?;
                    out.push_str(text);
                }
            }
        }
    }

    fn unicode(&mut self) -> Result<char, ParseError> {
        let mut code = 0_u32;
        for _ in 0..4 {
            let byte = self.next().ok_or(ParseError { detail: "json" })?;
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(ParseError { detail: "json" }),
            };
            code = (code << 4) | u32::from(digit);
        }
        char::from_u32(code).ok_or(ParseError { detail: "json" })
    }

    fn number(&mut self) -> Result<Json, ParseError> {
        let start = self.index;
        if self.eat(b'-') && !matches!(self.peek(), Some(b'0'..=b'9')) {
            return Err(ParseError { detail: "json" });
        }
        if self.peek() == Some(b'0') {
            self.index += 1;
            if matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(ParseError { detail: "json" });
            }
        } else {
            if !matches!(self.peek(), Some(b'1'..=b'9')) {
                return Err(ParseError { detail: "json" });
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(ParseError { detail: "json" });
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| ParseError { detail: "json" })?;
        let number = text.parse::<i64>().map_err(|_| ParseError { detail: "json" })?;
        Ok(Json::Int(number))
    }

    fn literal(&mut self, expected: &[u8], value: Json) -> Result<Json, ParseError> {
        if self.bytes[self.index..].starts_with(expected) {
            self.index += expected.len();
            Ok(value)
        } else {
            Err(ParseError { detail: "json" })
        }
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

    fn bump(&mut self, expected: u8) -> Result<(), ParseError> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(ParseError { detail: "json" })
        }
    }
}

fn utf8_width(byte: u8) -> Option<usize> {
    if byte < 0x80 {
        Some(1)
    } else if byte & 0b1110_0000 == 0b1100_0000 {
        Some(2)
    } else if byte & 0b1111_0000 == 0b1110_0000 {
        Some(3)
    } else if byte & 0b1111_1000 == 0b1111_0000 {
        Some(4)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn unit_duplicate_key_and_password_are_malformed() {
        assert_eq!(parse(br#"{"a":1,"a":2}"#).unwrap_err().detail, "json");
        assert_eq!(parse(br#"{"password":"x"}"#).unwrap_err().detail, "credential");
    }
}
