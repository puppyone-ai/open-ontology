use crate::limits::{MAX_JSON_DEPTH, MAX_JSON_NODES};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(u64),
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
            if fields.iter().any(|(name, _)| name == &key) {
                return Err(JsonError::Syntax);
            }
            self.skip();
            if self.peek() != Some(b':') {
                return Err(JsonError::Syntax);
            }
            self.bump()?;
            let child = self.value(depth + 1)?;
            fields.push((key, child));
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.bump()?;
                    self.skip();
                    if self.peek() == Some(b'}') {
                        return Err(JsonError::Syntax);
                    }
                }
                Some(b'}') => {
                    self.bump()?;
                    break;
                }
                _ => return Err(JsonError::Syntax),
            }
        }
        Ok(Json::Object(fields))
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
                    self.skip();
                    if self.peek() == Some(b']') {
                        return Err(JsonError::Syntax);
                    }
                }
                Some(b']') => {
                    self.bump()?;
                    break;
                }
                _ => return Err(JsonError::Syntax),
            }
        }
        Ok(Json::Array(items))
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.bump()?;
        let mut out = String::new();
        loop {
            if self.index >= self.bytes.len() {
                return Err(JsonError::Syntax);
            }
            let byte = self.bytes[self.index];
            if byte == b'"' {
                self.index += 1;
                return Ok(out);
            }
            if byte == b'\\' {
                self.index += 1;
                let escape = self.next_byte()?;
                match escape {
                    b'"' | b'\\' | b'/' => out.push(escape as char),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => out.push(self.unicode_escape()?),
                    _ => return Err(JsonError::Syntax),
                }
                continue;
            }
            if byte < 0x20 {
                return Err(JsonError::Syntax);
            }
            out.push(self.decode_char()?);
        }
    }

    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let unit = self.hex4()?;
        if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(JsonError::Syntax);
        }
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.next_byte()? != b'\\' || self.next_byte()? != b'u' {
                return Err(JsonError::Syntax);
            }
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(JsonError::Syntax);
            }
            let combined = 0x1_0000
                + (((u32::from(unit) - 0xD800) << 10) | (u32::from(low) - 0xDC00));
            return char::from_u32(combined).ok_or(JsonError::Syntax);
        }
        char::from_u32(u32::from(unit)).ok_or(JsonError::Syntax)
    }

    fn hex4(&mut self) -> Result<u16, JsonError> {
        let mut value = 0_u16;
        for _ in 0..4 {
            let byte = self.next_byte()?;
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
        let first = self.next_byte()?;
        if first == b'0' {
            if matches!(self.peek(), Some(b'0'..=b'9' | b'.' | b'e' | b'E')) {
                return Err(JsonError::Syntax);
            }
            return Ok(0);
        }
        if !first.is_ascii_digit() {
            return Err(JsonError::Syntax);
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump()?;
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E' | b'+' | b'-')) {
            return Err(JsonError::Syntax);
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| JsonError::Syntax)?;
        text.parse::<u64>().map_err(|_| JsonError::Syntax)
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), JsonError> {
        if !self.bytes[self.index..].starts_with(word) {
            return Err(JsonError::Syntax);
        }
        self.index += word.len();
        Ok(())
    }

    fn decode_char(&mut self) -> Result<char, JsonError> {
        let rest = &self.bytes[self.index..];
        if rest.is_empty() {
            return Err(JsonError::Syntax);
        }
        let width = match rest[0] {
            0x00..=0x7F => 1,
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => return Err(JsonError::Syntax),
        };
        if rest.len() < width {
            return Err(JsonError::Syntax);
        }
        let text = std::str::from_utf8(&rest[..width]).map_err(|_| JsonError::Syntax)?;
        let ch = text.chars().next().ok_or(JsonError::Syntax)?;
        self.index += width;
        Ok(ch)
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
        self.next_byte().map(|_| ())
    }

    fn next_byte(&mut self) -> Result<u8, JsonError> {
        let byte = self.peek().ok_or(JsonError::Syntax)?;
        self.index += 1;
        Ok(byte)
    }
}
