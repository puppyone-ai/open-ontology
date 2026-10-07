//! Small JSON reader and writer for the operator file, the manifest, and the report.

use crate::{fail, Fail};

#[derive(Debug, Clone, PartialEq)]
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
        match self {
            Value::Object(pairs) => pairs.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn string(&self, key: &str) -> Result<&str, Fail> {
        match self.get(key) {
            Some(Value::String(text)) => Ok(text),
            Some(_) => Err(fail(2, format!("{key} is not a string"))),
            None => Err(fail(2, format!("missing {key}"))),
        }
    }

    pub fn bool_key(&self, key: &str) -> Result<bool, Fail> {
        match self.get(key) {
            Some(Value::Bool(value)) => Ok(*value),
            Some(_) => Err(fail(2, format!("{key} is not a bool"))),
            None => Err(fail(2, format!("missing {key}"))),
        }
    }

    pub fn u64_key(&self, key: &str) -> Result<u64, Fail> {
        match self.get(key) {
            Some(Value::Number(text)) => text
                .parse()
                .map_err(|_| fail(2, format!("{key} is not an integer"))),
            Some(_) => Err(fail(2, format!("{key} is not a number"))),
            None => Err(fail(2, format!("missing {key}"))),
        }
    }

    pub fn array(&self, key: &str) -> Result<&[Value], Fail> {
        match self.get(key) {
            Some(Value::Array(items)) => Ok(items),
            Some(_) => Err(fail(2, format!("{key} is not an array"))),
            None => Err(fail(2, format!("missing {key}"))),
        }
    }
}

pub fn parse(input: &str) -> Result<Value, Fail> {
    let mut parser = Parser { bytes: input.as_bytes(), index: 0 };
    let value = parser.value()?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err(fail(2, "trailing json"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn skip(&mut self) {
        while let Some(byte) = self.bytes.get(self.index) {
            if byte.is_ascii_whitespace() {
                self.index += 1;
            } else {
                break;
            }
        }
    }

    fn value(&mut self) -> Result<Value, Fail> {
        self.skip();
        match self.bytes.get(self.index).copied() {
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            _ => Err(fail(2, "invalid json value")),
        }
    }

    fn literal(&mut self, text: &[u8], value: Value) -> Result<Value, Fail> {
        if self.bytes.get(self.index..self.index + text.len()) == Some(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err(fail(2, "invalid json literal"))
        }
    }

    fn number(&mut self) -> Result<Value, Fail> {
        let start = self.index;
        if self.bytes.get(self.index) == Some(&b'-') {
            self.index += 1;
        }
        let digits = self.index;
        while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
            self.index += 1;
        }
        if self.index == digits {
            return Err(fail(2, "invalid json number"));
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index])
            .map_err(|_| fail(2, "invalid json number"))?;
        if text.starts_with("-0") || (text.starts_with('0') && text.len() > 1) {
            return Err(fail(2, "invalid json number"));
        }
        Ok(Value::Number(text.to_string()))
    }

    fn string(&mut self) -> Result<String, Fail> {
        if self.bytes.get(self.index) != Some(&b'"') {
            return Err(fail(2, "invalid json string"));
        }
        self.index += 1;
        let mut out = String::new();
        while let Some(byte) = self.bytes.get(self.index).copied() {
            self.index += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = self.bytes.get(self.index).copied().ok_or_else(|| fail(2, "bad escape"))?;
                    self.index += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self.bytes.get(self.index..self.index + 4).ok_or_else(|| fail(2, "bad unicode"))?;
                            self.index += 4;
                            let text = std::str::from_utf8(hex).map_err(|_| fail(2, "bad unicode"))?;
                            let code = u32::from_str_radix(text, 16).map_err(|_| fail(2, "bad unicode"))?;
                            let ch = char::from_u32(code).ok_or_else(|| fail(2, "bad unicode"))?;
                            out.push(ch);
                        }
                        _ => return Err(fail(2, "bad escape")),
                    }
                }
                byte if byte < 0x20 => return Err(fail(2, "raw control in string")),
                byte => out.push(byte as char),
            }
        }
        Err(fail(2, "unterminated string"))
    }

    fn array(&mut self) -> Result<Value, Fail> {
        self.index += 1;
        let mut items = Vec::new();
        self.skip();
        if self.bytes.get(self.index) == Some(&b']') {
            self.index += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip();
            match self.bytes.get(self.index).copied() {
                Some(b',') => self.index += 1,
                Some(b']') => {
                    self.index += 1;
                    break;
                }
                _ => return Err(fail(2, "invalid json array")),
            }
        }
        Ok(Value::Array(items))
    }

    fn object(&mut self) -> Result<Value, Fail> {
        self.index += 1;
        let mut pairs = Vec::new();
        self.skip();
        if self.bytes.get(self.index) == Some(&b'}') {
            self.index += 1;
            return Ok(Value::Object(pairs));
        }
        loop {
            self.skip();
            let key = self.string()?;
            self.skip();
            if self.bytes.get(self.index) != Some(&b':') {
                return Err(fail(2, "invalid json object"));
            }
            self.index += 1;
            if pairs.iter().any(|(name, _)| name == &key) {
                return Err(fail(2, "duplicate json key"));
            }
            pairs.push((key, self.value()?));
            self.skip();
            match self.bytes.get(self.index).copied() {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    break;
                }
                _ => return Err(fail(2, "invalid json object")),
            }
        }
        Ok(Value::Object(pairs))
    }
}

pub fn require_keys(object: &Value, keys: &[&str]) -> Result<(), Fail> {
    let Value::Object(pairs) = object else {
        return Err(fail(2, "json object required"));
    };
    if pairs.len() != keys.len() {
        return Err(fail(2, "unexpected json keys"));
    }
    for key in keys {
        if object.get(key).is_none() {
            return Err(fail(2, format!("missing {key}")));
        }
    }
    Ok(())
}

pub fn escape(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
