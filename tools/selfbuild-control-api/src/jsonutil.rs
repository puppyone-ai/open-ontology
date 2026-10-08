//! One JSON value. Duplicate object keys and trailing bytes are rejected.

use std::str::FromStr;

use serde_json::Value;

use crate::error::ApiError;

pub fn parse_exact(bytes: &[u8]) -> Result<Value, ApiError> {
    if bytes.len() > 65_536 {
        return Err(ApiError::malformed());
    }
    let mut parser = Parser { bytes, index: 0 };
    parser.skip_ws();
    let value = parser.value().map_err(|_| ApiError::malformed())?;
    parser.skip_ws();
    if parser.index != parser.bytes.len() {
        return Err(ApiError::malformed());
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn value(&mut self) -> Result<Value, ()> {
        self.skip_ws();
        let byte = self.peek().ok_or(())?;
        match byte {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Ok(Value::String(self.string()?)),
            b't' => self.literal(b"true", Value::Bool(true)),
            b'f' => self.literal(b"false", Value::Bool(false)),
            b'n' => self.literal(b"null", Value::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(()),
        }
    }

    fn object(&mut self) -> Result<Value, ()> {
        self.bump(b'{')?;
        let mut map = serde_json::Map::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.index += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(());
            }
            let key = self.string()?;
            if map.contains_key(&key) {
                return Err(());
            }
            self.skip_ws();
            self.bump(b':')?;
            let value = self.value()?;
            map.insert(key, value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    break;
                }
                _ => return Err(()),
            }
        }
        Ok(Value::Object(map))
    }

    fn array(&mut self) -> Result<Value, ()> {
        self.bump(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.index += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b']') => {
                    self.index += 1;
                    break;
                }
                _ => return Err(()),
            }
        }
        Ok(Value::Array(items))
    }

    fn number(&mut self) -> Result<Value, ()> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        let digits = self.index;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.index += 1;
        }
        if self.index == digits {
            return Err(());
        }
        if self.peek() == Some(b'.') {
            self.index += 1;
            let fraction = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == fraction {
                return Err(());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let exponent = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == exponent {
                return Err(());
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| ())?;
        let number = serde_json::Number::from_str(text).map_err(|_| ())?;
        Ok(Value::Number(number))
    }

    fn string(&mut self) -> Result<String, ()> {
        self.bump(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self.next().ok_or(())?;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = self.next().ok_or(())?;
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
                                *slot = self.next().ok_or(())?;
                            }
                            let text = std::str::from_utf8(&hex).map_err(|_| ())?;
                            let code = u32::from_str_radix(text, 16).map_err(|_| ())?;
                            let ch = char::from_u32(code).ok_or(())?;
                            out.push(ch);
                        }
                        _ => return Err(()),
                    }
                }
                byte if byte < 0x20 => return Err(()),
                byte => {
                    let width = utf8_width(byte)?;
                    let start = self.index - 1;
                    if self.index + width - 1 > self.bytes.len() {
                        return Err(());
                    }
                    self.index += width - 1;
                    let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| ())?;
                    out.push_str(text);
                }
            }
        }
    }

    fn literal(&mut self, expected: &[u8], value: Value) -> Result<Value, ()> {
        if self.bytes[self.index..].starts_with(expected) {
            self.index += expected.len();
            Ok(value)
        } else {
            Err(())
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }

    fn bump(&mut self, expected: u8) -> Result<(), ()> {
        if self.peek() == Some(expected) {
            self.index += 1;
            Ok(())
        } else {
            Err(())
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
}

fn utf8_width(byte: u8) -> Result<usize, ()> {
    if byte < 0x80 {
        Ok(1)
    } else if byte & 0b1110_0000 == 0b1100_0000 {
        Ok(2)
    } else if byte & 0b1111_0000 == 0b1110_0000 {
        Ok(3)
    } else if byte & 0b1111_1000 == 0b1111_0000 {
        Ok(4)
    } else {
        Err(())
    }
}
