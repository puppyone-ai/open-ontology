use crate::limits::{MAX_JSON_DEPTH, MAX_JSON_NODES};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(u64),
    NegInt(u64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonError {
    Syntax,
    Budget,
}

pub fn parse_json(input: &str) -> Result<Json, JsonError> {
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

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    nodes: usize,
}

impl<'a> Parser<'a> {
    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_JSON_DEPTH {
            return Err(JsonError::Budget);
        }
        self.nodes += 1;
        if self.nodes > MAX_JSON_NODES {
            return Err(JsonError::Budget);
        }
        self.skip();
        let Some(byte) = self.peek() else {
            return Err(JsonError::Syntax);
        };
        match byte {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => Ok(Json::String(self.string()?)),
            b't' => {
                self.literal(b"true")?;
                Ok(Json::Bool(true))
            }
            b'f' => {
                self.literal(b"false")?;
                Ok(Json::Bool(false))
            }
            b'n' => {
                self.literal(b"null")?;
                Ok(Json::Null)
            }
            b'0'..=b'9' => Ok(Json::Number(self.number()?)),
            b'-' => Ok(Json::NegInt(self.neg_int()?)),
            _ => Err(JsonError::Syntax),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.bump()?;
        let mut fields = Vec::new();
        self.skip();
        if self.peek() == Some(b'}') {
            self.bump()?;
            return Ok(Json::Object(fields));
        }
        loop {
            self.skip();
            if self.peek() != Some(b'"') {
                return Err(JsonError::Syntax);
            }
            let key = self.string()?;
            if fields.iter().any(|(existing, _)| existing == &key) {
                return Err(JsonError::Syntax);
            }
            self.skip();
            if self.peek() != Some(b':') {
                return Err(JsonError::Syntax);
            }
            self.bump()?;
            let value = self.value(depth + 1)?;
            fields.push((key, value));
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.bump()?;
                }
                Some(b'}') => {
                    self.bump()?;
                    return Ok(Json::Object(fields));
                }
                _ => return Err(JsonError::Syntax),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.bump()?;
        let mut items = Vec::new();
        self.skip();
        if self.peek() == Some(b']') {
            self.bump()?;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.bump()?;
                }
                Some(b']') => {
                    self.bump()?;
                    return Ok(Json::Array(items));
                }
                _ => return Err(JsonError::Syntax),
            }
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.bump()?;
        let mut out = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(JsonError::Syntax);
            };
            match byte {
                b'"' => {
                    self.bump()?;
                    return Ok(out);
                }
                b'\\' => {
                    self.bump()?;
                    out.push(self.escape()?);
                }
                0x00..=0x1F => return Err(JsonError::Syntax),
                _ => {
                    let rest = std::str::from_utf8(&self.bytes[self.index..])
                        .map_err(|_| JsonError::Syntax)?;
                    let ch = rest.chars().next().ok_or(JsonError::Syntax)?;
                    if out.len().saturating_add(ch.len_utf8()) > 1_048_576 {
                        return Err(JsonError::Budget);
                    }
                    out.push(ch);
                    self.index += ch.len_utf8();
                }
            }
        }
    }

    fn escape(&mut self) -> Result<char, JsonError> {
        let Some(byte) = self.bump_byte() else {
            return Err(JsonError::Syntax);
        };
        match byte {
            b'"' => Ok('"'),
            b'\\' => Ok('\\'),
            b'/' => Ok('/'),
            b'b' => Ok('\u{0008}'),
            b'f' => Ok('\u{000c}'),
            b'n' => Ok('\n'),
            b'r' => Ok('\r'),
            b't' => Ok('\t'),
            b'u' => {
                let unit = self.hex4()?;
                char::from_u32(u32::from(unit)).ok_or(JsonError::Syntax)
            }
            _ => Err(JsonError::Syntax),
        }
    }

    fn hex4(&mut self) -> Result<u16, JsonError> {
        let mut value = 0_u16;
        for _ in 0..4 {
            let Some(byte) = self.bump_byte() else {
                return Err(JsonError::Syntax);
            };
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(JsonError::Syntax),
            };
            value = (value << 4) | u16::from(digit);
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<u64, JsonError> {
        let start = self.index;
        let Some(first) = self.bump_byte() else {
            return Err(JsonError::Syntax);
        };
        if !first.is_ascii_digit() {
            return Err(JsonError::Syntax);
        }
        if first == b'0' {
            if matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(JsonError::Syntax);
            }
            return Ok(0);
        }
        let mut value = u64::from(first - b'0');
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            let Some(byte) = self.bump_byte() else {
                return Err(JsonError::Syntax);
            };
            value = value
                .checked_mul(10)
                .and_then(|acc| acc.checked_add(u64::from(byte - b'0')))
                .ok_or(JsonError::Syntax)?;
        }
        if self.index == start {
            return Err(JsonError::Syntax);
        }
        Ok(value)
    }

    fn neg_int(&mut self) -> Result<u64, JsonError> {
        self.bump()?;
        let value = self.number()?;
        if value == 0 {
            return Err(JsonError::Syntax);
        }
        Ok(value)
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), JsonError> {
        for expected in word {
            if self.peek() != Some(*expected) {
                return Err(JsonError::Syntax);
            }
            self.bump()?;
        }
        Ok(())
    }

    fn skip(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.index += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn bump(&mut self) -> Result<(), JsonError> {
        self.bump_byte().map(|_| ()).ok_or(JsonError::Syntax)
    }

    fn bump_byte(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.index += 1;
        Some(byte)
    }
}

pub fn object(value: Json) -> Result<Vec<(String, Json)>, ()> {
    match value {
        Json::Object(fields) => Ok(fields),
        _ => Err(()),
    }
}

pub fn take(fields: &mut Vec<(String, Json)>, key: &str) -> Result<Json, ()> {
    let Some(index) = fields.iter().position(|(name, _)| name == key) else {
        return Err(());
    };
    Ok(fields.remove(index).1)
}

pub fn finish(fields: &[(String, Json)]) -> Result<(), ()> {
    if fields.is_empty() { Ok(()) } else { Err(()) }
}

pub fn as_str(value: &Json) -> Result<&str, ()> {
    match value {
        Json::String(text) => Ok(text),
        _ => Err(()),
    }
}

pub fn as_bool(value: &Json) -> Result<bool, ()> {
    match value {
        Json::Bool(bit) => Ok(*bit),
        _ => Err(()),
    }
}

pub fn as_u64(value: &Json) -> Result<u64, ()> {
    match value {
        Json::Number(number) => Ok(*number),
        _ => Err(()),
    }
}

pub fn as_i32(value: &Json) -> Result<i32, ()> {
    match value {
        Json::Number(number) => i32::try_from(*number).map_err(|_| ()),
        Json::NegInt(number) => {
            let positive = i32::try_from(*number).map_err(|_| ())?;
            positive.checked_neg().ok_or(())
        }
        _ => Err(()),
    }
}

pub fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                let code = ch as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
