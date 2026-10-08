use serde_json::{Map, Number, Value};

const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 8192;

pub enum JsonError {
    Syntax,
    Budget,
}

pub fn parse_json(input: &str) -> Result<Value, JsonError> {
    if input.as_bytes().starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(JsonError::Syntax);
    }
    let mut parser = Parser {
        bytes: input.as_bytes(),
        index: 0,
        nodes: 0,
    };
    parser.skip();
    let value = parser.value(1)?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err(JsonError::Syntax);
    }
    Ok(value)
}

pub fn contains_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.iter().any(contains_null),
        Value::Object(map) => map.values().any(contains_null),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    nodes: usize,
}

impl<'a> Parser<'a> {
    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        if depth > MAX_DEPTH {
            return Err(JsonError::Budget);
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(JsonError::Budget);
        }
        self.skip();
        let Some(byte) = self.peek() else {
            return Err(JsonError::Syntax);
        };
        match byte {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => Ok(Value::String(self.string()?)),
            b't' => self.literal(b"true", Value::Bool(true)),
            b'f' => self.literal(b"false", Value::Bool(false)),
            b'n' => self.literal(b"null", Value::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(JsonError::Syntax),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.bump();
        let mut map = Map::new();
        self.skip();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(Value::Object(map));
        }
        loop {
            self.skip();
            if self.peek() != Some(b'"') {
                return Err(JsonError::Syntax);
            }
            let key = self.string()?;
            self.skip();
            if self.peek() != Some(b':') {
                return Err(JsonError::Syntax);
            }
            self.bump();
            let child = self.value(depth + 1)?;
            if map.contains_key(&key) {
                return Err(JsonError::Syntax);
            }
            map.insert(key, child);
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip();
                    if self.peek() == Some(b'}') {
                        return Err(JsonError::Syntax);
                    }
                }
                Some(b'}') => {
                    self.bump();
                    break;
                }
                _ => return Err(JsonError::Syntax),
            }
        }
        Ok(Value::Object(map))
    }

    fn array(&mut self, depth: usize) -> Result<Value, JsonError> {
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
                        return Err(JsonError::Syntax);
                    }
                }
                Some(b']') => {
                    self.bump();
                    break;
                }
                _ => return Err(JsonError::Syntax),
            }
        }
        Ok(Value::Array(items))
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.bump();
        let mut out = String::new();
        loop {
            let Some(byte) = self.bump() else {
                return Err(JsonError::Syntax);
            };
            match byte {
                b'"' => return Ok(out),
                b'\\' => self.escape(&mut out)?,
                0x00..=0x1F => return Err(JsonError::Syntax),
                0x20..=0x7F => out.push(byte as char),
                _ => self.push_utf8(byte, &mut out)?,
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), JsonError> {
        let Some(byte) = self.bump() else {
            return Err(JsonError::Syntax);
        };
        match byte {
            b'"' | b'\\' | b'/' => out.push(byte as char),
            b'b' => out.push('\u{0008}'),
            b'f' => out.push('\u{000c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => self.unicode(out)?,
            _ => return Err(JsonError::Syntax),
        }
        Ok(())
    }

    fn unicode(&mut self, out: &mut String) -> Result<(), JsonError> {
        let unit = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                return Err(JsonError::Syntax);
            }
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(JsonError::Syntax);
            }
            let code =
                0x1_0000 + (((u32::from(unit) - 0xD800) << 10) | (u32::from(low) - 0xDC00));
            let Some(ch) = char::from_u32(code) else {
                return Err(JsonError::Syntax);
            };
            out.push(ch);
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(JsonError::Syntax);
        } else {
            let Some(ch) = char::from_u32(u32::from(unit)) else {
                return Err(JsonError::Syntax);
            };
            out.push(ch);
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u16, JsonError> {
        let mut value = 0_u16;
        for _ in 0..4 {
            let Some(byte) = self.bump() else {
                return Err(JsonError::Syntax);
            };
            let Some(digit) = hex_digit(byte) else {
                return Err(JsonError::Syntax);
            };
            value = (value << 4) | u16::from(digit);
        }
        Ok(value)
    }

    fn push_utf8(&mut self, first: u8, out: &mut String) -> Result<(), JsonError> {
        let width = if first & 0xE0 == 0xC0 {
            2
        } else if first & 0xF0 == 0xE0 {
            3
        } else if first & 0xF8 == 0xF0 {
            4
        } else {
            return Err(JsonError::Syntax);
        };
        let start = self.index - 1;
        let end = start + width;
        if end > self.bytes.len() {
            return Err(JsonError::Syntax);
        }
        let Some(text) = std::str::from_utf8(&self.bytes[start..end]).ok() else {
            return Err(JsonError::Syntax);
        };
        if text.chars().count() != 1 {
            return Err(JsonError::Syntax);
        }
        out.push_str(text);
        self.index = end;
        Ok(())
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        let Some(first) = self.peek() else {
            return Err(JsonError::Syntax);
        };
        if !first.is_ascii_digit() {
            return Err(JsonError::Syntax);
        }
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-') {
                self.bump();
            } else {
                break;
            }
        }
        let Some(token) = std::str::from_utf8(&self.bytes[start..self.index]).ok() else {
            return Err(JsonError::Syntax);
        };
        let number = serde_json::from_str::<Number>(token).map_err(|_| JsonError::Syntax)?;
        Ok(Value::Number(number))
    }

    fn literal(&mut self, word: &[u8], value: Value) -> Result<Value, JsonError> {
        if self.bytes[self.index..].starts_with(word) {
            self.index += word.len();
            Ok(value)
        } else {
            Err(JsonError::Syntax)
        }
    }

    fn skip(&mut self) {
        while let Some(byte) = self.peek() {
            if byte == b' ' || byte == b'\n' || byte == b'\r' || byte == b'\t' {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.index += 1;
        Some(byte)
    }
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
