//! Duplicate-rejecting JSON reader for operator documents and CLI stdout.

#![forbid(unsafe_code)]

use crate::Fail;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        let Value::Object(items) = self else {
            return None;
        };
        items.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(text) => text.parse().ok(),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    nodes: usize,
    depth: usize,
}

pub fn parse(bytes: &[u8]) -> Result<Value, Fail> {
    let mut parser = Parser {
        bytes,
        index: 0,
        nodes: 0,
        depth: 0,
    };
    parser.skip();
    let value = parser.value()?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err(crate::fail("json trailing"));
    }
    Ok(value)
}

pub fn parse_file(path: &std::path::Path) -> Result<Value, Fail> {
    let bytes = std::fs::read(path).map_err(|_| crate::fail(format!("read {}", path.display())))?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(crate::fail("json budget"));
    }
    parse(&bytes)
}

impl<'a> Parser<'a> {
    fn value(&mut self) -> Result<Value, Fail> {
        self.nodes += 1;
        if self.nodes > 200_000 || self.depth > 64 {
            return Err(crate::fail("json budget"));
        }
        self.skip();
        let byte = self.peek().ok_or_else(|| crate::fail("json shape"))?;
        match byte {
            b'n' => self.literal(b"null", Value::Null),
            b't' => self.literal(b"true", Value::Bool(true)),
            b'f' => self.literal(b"false", Value::Bool(false)),
            b'"' => Ok(Value::String(self.string()?)),
            b'[' => self.array(),
            b'{' => self.object(),
            b'-' | b'0'..=b'9' => Ok(Value::Number(self.number()?)),
            _ => Err(crate::fail("json shape")),
        }
    }

    fn literal(&mut self, text: &[u8], value: Value) -> Result<Value, Fail> {
        if self.bytes[self.index..].starts_with(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err(crate::fail("json shape"))
        }
    }

    fn array(&mut self) -> Result<Value, Fail> {
        self.index += 1;
        self.depth += 1;
        let mut items = Vec::new();
        self.skip();
        if self.peek() == Some(b']') {
            self.index += 1;
            self.depth -= 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.index += 1;
                }
                Some(b']') => {
                    self.index += 1;
                    self.depth -= 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(crate::fail("json shape")),
            }
        }
    }

    fn object(&mut self) -> Result<Value, Fail> {
        self.index += 1;
        self.depth += 1;
        let mut items = Vec::new();
        self.skip();
        if self.peek() == Some(b'}') {
            self.index += 1;
            self.depth -= 1;
            return Ok(Value::Object(items));
        }
        loop {
            self.skip();
            if self.peek() != Some(b'"') {
                return Err(crate::fail("json shape"));
            }
            let key = self.string()?;
            if items.iter().any(|(name, _)| name == &key) {
                return Err(crate::fail("json duplicate"));
            }
            self.skip();
            if self.peek() != Some(b':') {
                return Err(crate::fail("json shape"));
            }
            self.index += 1;
            let value = self.value()?;
            items.push((key, value));
            self.skip();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    self.depth -= 1;
                    return Ok(Value::Object(items));
                }
                _ => return Err(crate::fail("json shape")),
            }
        }
    }

    fn number(&mut self) -> Result<String, Fail> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        let digits = self.index;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.index += 1;
        }
        if self.index == digits {
            return Err(crate::fail("json shape"));
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(crate::fail("json number"));
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| crate::fail("json shape"))?;
        Ok(text.to_owned())
    }

    fn string(&mut self) -> Result<String, Fail> {
        self.index += 1;
        let mut out = String::new();
        while let Some(byte) = self.peek() {
            self.index += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = self.peek().ok_or_else(|| crate::fail("json shape"))?;
                    self.index += 1;
                    match escaped {
                        b'"' | b'\\' | b'/' => out.push(escaped as char),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.hex4()?),
                        _ => return Err(crate::fail("json shape")),
                    }
                }
                0x20..=0x7f => out.push(byte as char),
                _ => {
                    self.index -= 1;
                    let rest = &self.bytes[self.index..];
                    let text = std::str::from_utf8(rest).map_err(|_| crate::fail("json utf8"))?;
                    let ch = text.chars().next().ok_or_else(|| crate::fail("json utf8"))?;
                    if ch.is_control() {
                        return Err(crate::fail("json shape"));
                    }
                    out.push(ch);
                    self.index += ch.len_utf8();
                }
            }
        }
        Err(crate::fail("json shape"))
    }

    fn hex4(&mut self) -> Result<char, Fail> {
        if self.index + 4 > self.bytes.len() {
            return Err(crate::fail("json shape"));
        }
        let text = std::str::from_utf8(&self.bytes[self.index..self.index + 4]).map_err(|_| crate::fail("json shape"))?;
        self.index += 4;
        let code = u32::from_str_radix(text, 16).map_err(|_| crate::fail("json shape"))?;
        char::from_u32(code).ok_or_else(|| crate::fail("json shape"))
    }

    fn skip(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }
}

pub fn escape(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn render(value: &Value) -> String {
    let mut out = String::new();
    render_into(value, &mut out);
    out
}

fn render_into(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(text) => out.push_str(text),
        Value::String(text) => out.push_str(&escape(text)),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                render_into(item, out);
            }
            out.push(']');
        }
        Value::Object(items) => {
            out.push('{');
            for (index, (key, item)) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&escape(key));
                out.push(':');
                render_into(item, out);
            }
            out.push('}');
        }
    }
}

pub fn without_null_row_filter(value: &Value) -> Value {
    let Value::Object(items) = value else {
        return value.clone();
    };
    Value::Object(
        items
            .iter()
            .filter(|(key, item)| !(key == "row_filter" && matches!(item, Value::Null)))
            .cloned()
            .collect(),
    )
}

pub fn string_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, Fail> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| crate::fail(format!("missing {key}")))
}

pub fn lines(bytes: &[u8]) -> Result<Vec<Value>, Fail> {
    let text = std::str::from_utf8(bytes).map_err(|_| crate::fail("json utf8"))?;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        out.push(parse(line.as_bytes())?);
    }
    Ok(out)
}
