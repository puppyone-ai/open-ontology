//! A small JSON reader and writer for verdicts and producer objects.

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
        let Value::Object(pairs) = self else {
            return None;
        };
        pairs.iter().rev().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
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

pub fn parse(input: &str) -> Result<Value, String> {
    let mut parser = Parser { bytes: input.as_bytes(), index: 0 };
    let value = parser.value()?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err("trailing json".to_owned());
    }
    Ok(value)
}

pub fn stringify(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn skip(&mut self) {
        while self.index < self.bytes.len() && self.bytes[self.index].is_ascii_whitespace() {
            self.index += 1;
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.skip();
        let byte = *self.bytes.get(self.index).ok_or_else(|| "truncated json".to_owned())?;
        match byte {
            b'n' => self.literal(b"null", Value::Null),
            b't' => self.literal(b"true", Value::Bool(true)),
            b'f' => self.literal(b"false", Value::Bool(false)),
            b'"' => Ok(Value::String(self.string()?)),
            b'[' => self.array(),
            b'{' => self.object(),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err("invalid json".to_owned()),
        }
    }

    fn literal(&mut self, text: &[u8], value: Value) -> Result<Value, String> {
        if self.bytes.get(self.index..self.index + text.len()) == Some(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err("invalid json".to_owned())
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.index += 1;
        let mut items = Vec::new();
        loop {
            self.skip();
            if self.bytes.get(self.index) == Some(&b']') {
                self.index += 1;
                return Ok(Value::Array(items));
            }
            if !items.is_empty() {
                if self.bytes.get(self.index) != Some(&b',') {
                    return Err("invalid json".to_owned());
                }
                self.index += 1;
            }
            items.push(self.value()?);
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.index += 1;
        let mut pairs = Vec::new();
        loop {
            self.skip();
            if self.bytes.get(self.index) == Some(&b'}') {
                self.index += 1;
                return Ok(Value::Object(pairs));
            }
            if !pairs.is_empty() {
                if self.bytes.get(self.index) != Some(&b',') {
                    return Err("invalid json".to_owned());
                }
                self.index += 1;
                self.skip();
            }
            if self.bytes.get(self.index) != Some(&b'"') {
                return Err("invalid json".to_owned());
            }
            let key = self.string()?;
            self.skip();
            if self.bytes.get(self.index) != Some(&b':') {
                return Err("invalid json".to_owned());
            }
            self.index += 1;
            pairs.push((key, self.value()?));
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.index;
        if self.bytes.get(self.index) == Some(&b'-') {
            self.index += 1;
        }
        let digits = self.index;
        while self.bytes.get(self.index).is_some_and(|byte| byte.is_ascii_digit()) {
            self.index += 1;
        }
        if self.index == digits {
            return Err("invalid json".to_owned());
        }
        if self.bytes.get(self.index) == Some(&b'.') {
            self.index += 1;
            let frac = self.index;
            while self.bytes.get(self.index).is_some_and(|byte| byte.is_ascii_digit()) {
                self.index += 1;
            }
            if self.index == frac {
                return Err("invalid json".to_owned());
            }
        }
        if self.bytes.get(self.index).is_some_and(|byte| *byte == b'e' || *byte == b'E') {
            self.index += 1;
            if self.bytes.get(self.index).is_some_and(|byte| *byte == b'+' || *byte == b'-') {
                self.index += 1;
            }
            let exp = self.index;
            while self.bytes.get(self.index).is_some_and(|byte| byte.is_ascii_digit()) {
                self.index += 1;
            }
            if self.index == exp {
                return Err("invalid json".to_owned());
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| "invalid json".to_owned())?;
        Ok(Value::Number(text.to_owned()))
    }

    fn string(&mut self) -> Result<String, String> {
        self.index += 1;
        let mut raw = Vec::new();
        while self.index < self.bytes.len() {
            let byte = self.bytes[self.index];
            if byte == b'"' {
                self.index += 1;
                return String::from_utf8(raw).map_err(|_| "invalid json".to_owned());
            }
            if byte == b'\\' {
                self.index += 1;
                let escaped = *self.bytes.get(self.index).ok_or_else(|| "truncated json".to_owned())?;
                self.index += 1;
                let ch = match escaped {
                    b'"' => '"',
                    b'\\' => '\\',
                    b'/' => '/',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    b'u' => {
                        let hex = self.bytes.get(self.index..self.index + 4).ok_or_else(|| "truncated json".to_owned())?;
                        self.index += 4;
                        let text = std::str::from_utf8(hex).map_err(|_| "invalid json".to_owned())?;
                        let code = u32::from_str_radix(text, 16).map_err(|_| "invalid json".to_owned())?;
                        char::from_u32(code).ok_or_else(|| "invalid json".to_owned())?
                    }
                    _ => return Err("invalid json".to_owned()),
                };
                raw.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
                continue;
            }
            if byte < 0x20 {
                return Err("invalid json".to_owned());
            }
            raw.push(byte);
            self.index += 1;
        }
        Err("truncated json".to_owned())
    }
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(text) => out.push_str(text),
        Value::String(text) => write_string(out, text),
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
        Value::Object(pairs) => {
            out.push('{');
            for (index, (key, item)) in pairs.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_value(out, item);
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
            char if (char as u32) < 0x20 => out.push_str(&format!("\\u{code:04x}", code = char as u32)),
            char => out.push(char),
        }
    }
    out.push('"');
}

pub fn object(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(pairs.into_iter().map(|(key, value)| (key.to_owned(), value)).collect())
}

#[cfg(test)]
mod tests {
    use super::{parse, stringify};

    #[test]
    fn roundtrip_preserves_decimal_lexemes() {
        let value = parse(r#"{"n":1234567890.1234,"s":"a\"b","b":true,"z":null,"a":[1]}"#).expect("parse");
        assert_eq!(value.get("n").and_then(|item| match item {
            super::Value::Number(text) => Some(text.as_str()),
            _ => None,
        }), Some("1234567890.1234"));
        let again = parse(&stringify(&value)).expect("again");
        assert_eq!(again, value);
    }
}
