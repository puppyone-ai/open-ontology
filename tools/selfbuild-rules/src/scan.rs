//! JSON document scan. Duplicate keys, depth, node count, and trailing bytes are `json`.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;

use crate::{Fail, MAX_INPUT_BYTES};

const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 100_000;

pub fn parse_json(bytes: &[u8]) -> Result<Value, Fail> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(Fail::new("bound.input"));
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(Fail::new("json"));
    }
    check_structure(bytes)?;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = Value::deserialize(&mut deserializer).map_err(|_| Fail::new("json"))?;
    deserializer.end().map_err(|_| Fail::new("json"))?;
    Ok(value)
}

fn check_structure(bytes: &[u8]) -> Result<(), Fail> {
    let mut parser = Parser { bytes, index: 0, nodes: 0 };
    parser.value(1)?;
    parser.skip();
    if parser.index == parser.bytes.len() {
        Ok(())
    } else {
        Err(Fail::new("json"))
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    nodes: usize,
}

impl<'a> Parser<'a> {
    fn value(&mut self, depth: usize) -> Result<(), Fail> {
        if depth > MAX_DEPTH {
            return Err(Fail::new("json"));
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(Fail::new("json"));
        }
        self.skip();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => {
                self.string()?;
                Ok(())
            }
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            _ => Err(Fail::new("json")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<(), Fail> {
        self.bump(b'{')?;
        self.skip();
        if self.peek() == Some(b'}') {
            self.index += 1;
            return Ok(());
        }
        let mut keys = HashSet::new();
        loop {
            self.skip();
            let key = self.string()?;
            if !keys.insert(key) {
                return Err(Fail::new("json"));
            }
            self.skip();
            self.bump(b':')?;
            self.value(depth + 1)?;
            self.skip();
            match self.peek() {
                Some(b'}') => {
                    self.index += 1;
                    return Ok(());
                }
                Some(b',') => self.index += 1,
                _ => return Err(Fail::new("json")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<(), Fail> {
        self.bump(b'[')?;
        self.skip();
        if self.peek() == Some(b']') {
            self.index += 1;
            return Ok(());
        }
        loop {
            self.value(depth + 1)?;
            self.skip();
            match self.peek() {
                Some(b']') => {
                    self.index += 1;
                    return Ok(());
                }
                Some(b',') => self.index += 1,
                _ => return Err(Fail::new("json")),
            }
        }
    }

    fn string(&mut self) -> Result<String, Fail> {
        self.bump(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self.next()?;
            match byte {
                b'"' => return Ok(out),
                b'\\' => out.push(self.escape()?),
                raw if raw < 0x20 => return Err(Fail::new("json")),
                raw => {
                    let width = utf8_width(raw)?;
                    if self.index - 1 + width > self.bytes.len() {
                        return Err(Fail::new("json"));
                    }
                    let chunk = &self.bytes[self.index - 1..self.index - 1 + width];
                    let text = std::str::from_utf8(chunk).map_err(|_| Fail::new("json"))?;
                    out.push_str(text);
                    self.index = self.index - 1 + width;
                }
            }
        }
    }

    fn escape(&mut self) -> Result<char, Fail> {
        match self.next()? {
            b'"' => Ok('"'),
            b'\\' => Ok('\\'),
            b'/' => Ok('/'),
            b'b' => Ok('\u{0008}'),
            b'f' => Ok('\u{000c}'),
            b'n' => Ok('\n'),
            b'r' => Ok('\r'),
            b't' => Ok('\t'),
            b'u' => self.unicode(),
            _ => Err(Fail::new("json")),
        }
    }

    fn unicode(&mut self) -> Result<char, Fail> {
        let unit = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.next()? != b'\\' || self.next()? != b'u' {
                return Err(Fail::new("json"));
            }
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(Fail::new("json"));
            }
            let code = 0x1_0000 + (((u32::from(unit) - 0xD800) << 10) | (u32::from(low) - 0xDC00));
            char::from_u32(code).ok_or(Fail::new("json"))
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            Err(Fail::new("json"))
        } else {
            char::from_u32(u32::from(unit)).ok_or(Fail::new("json"))
        }
    }

    fn hex4(&mut self) -> Result<u16, Fail> {
        let mut value = 0u16;
        for _ in 0..4 {
            let byte = self.next()?;
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(Fail::new("json")),
            };
            value = (value << 4) | u16::from(digit);
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<(), Fail> {
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        let start = self.index;
        self.digits();
        if self.index == start {
            return Err(Fail::new("json"));
        }
        if self.peek() == Some(b'.') {
            self.index += 1;
            let frac = self.index;
            self.digits();
            if self.index == frac {
                return Err(Fail::new("json"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let exp = self.index;
            self.digits();
            if self.index == exp {
                return Err(Fail::new("json"));
            }
        }
        Ok(())
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.index += 1;
        }
    }

    fn literal(&mut self, text: &[u8]) -> Result<(), Fail> {
        if self.bytes[self.index..].starts_with(text) {
            self.index += text.len();
            Ok(())
        } else {
            Err(Fail::new("json"))
        }
    }

    fn bump(&mut self, expected: u8) -> Result<(), Fail> {
        if self.peek() == Some(expected) {
            self.index += 1;
            Ok(())
        } else {
            Err(Fail::new("json"))
        }
    }

    fn next(&mut self) -> Result<u8, Fail> {
        let byte = self.peek().ok_or(Fail::new("json"))?;
        self.index += 1;
        Ok(byte)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn skip(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.index += 1;
        }
    }
}

fn utf8_width(byte: u8) -> Result<usize, Fail> {
    if byte < 0x80 {
        Ok(1)
    } else if byte & 0xE0 == 0xC0 {
        Ok(2)
    } else if byte & 0xF0 == 0xE0 {
        Ok(3)
    } else if byte & 0xF8 == 0xF0 {
        Ok(4)
    } else {
        Err(Fail::new("json"))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_json;

    #[test]
    fn duplicate_key_and_unicode_alias() {
        assert_eq!(parse_json(br#"{"a":1,"a":2}"#).unwrap_err().code, "json");
        assert_eq!(parse_json(br#"{"a":1,"\u0061":2}"#).unwrap_err().code, "json");
        assert!(parse_json(b"{\"name\":\"\xE7\x94\xB2\"}\n").is_ok());
    }
}
