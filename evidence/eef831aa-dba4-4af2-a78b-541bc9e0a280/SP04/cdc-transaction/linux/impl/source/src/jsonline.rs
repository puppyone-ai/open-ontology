//! One JSON object per line, with duplicate keys rejected on the decoded spelling.

use std::collections::HashSet;

use serde::Deserialize;

use crate::model::{FaultCode, Wire, MAX_LINE_BYTES};

const MAX_DEPTH: usize = 32;

pub fn parse_wire(line: &str) -> Result<Wire, FaultCode> {
    if line.is_empty() || line.len() > MAX_LINE_BYTES {
        return Err(if line.is_empty() {
            FaultCode::Malformed
        } else {
            FaultCode::LineTooLong
        });
    }
    if line.bytes().any(|byte| byte == b'\n' || byte == b'\r') {
        return Err(FaultCode::Malformed);
    }
    reject_duplicate_keys(line)?;
    let mut deserializer = serde_json::Deserializer::from_str(line);
    let wire = Wire::deserialize(&mut deserializer).map_err(|_| FaultCode::Malformed)?;
    deserializer.end().map_err(|_| FaultCode::Malformed)?;
    Ok(wire)
}

fn reject_duplicate_keys(line: &str) -> Result<(), FaultCode> {
    let mut parser = Parser {
        bytes: line.as_bytes(),
        index: 0,
    };
    parser.value(0)?;
    parser.skip();
    if parser.index == parser.bytes.len() {
        Ok(())
    } else {
        Err(FaultCode::Malformed)
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn value(&mut self, depth: usize) -> Result<(), FaultCode> {
        if depth > MAX_DEPTH {
            return Err(FaultCode::Malformed);
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
            _ => Err(FaultCode::Malformed),
        }
    }

    fn object(&mut self, depth: usize) -> Result<(), FaultCode> {
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
                return Err(FaultCode::Malformed);
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
                _ => return Err(FaultCode::Malformed),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<(), FaultCode> {
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
                _ => return Err(FaultCode::Malformed),
            }
        }
    }

    fn string(&mut self) -> Result<String, FaultCode> {
        self.bump(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self.next()?;
            match byte {
                b'"' => return Ok(out),
                b'\\' => out.push(self.escape()?),
                raw if raw < 0x20 => return Err(FaultCode::Malformed),
                raw => {
                    let width = utf8_width(raw)?;
                    if self.index - 1 + width > self.bytes.len() {
                        return Err(FaultCode::Malformed);
                    }
                    let chunk = &self.bytes[self.index - 1..self.index - 1 + width];
                    let text = std::str::from_utf8(chunk).map_err(|_| FaultCode::Malformed)?;
                    out.push_str(text);
                    self.index = self.index - 1 + width;
                }
            }
        }
    }

    fn escape(&mut self) -> Result<char, FaultCode> {
        match self.next()? {
            b'"' => Ok('"'),
            b'\\' => Ok('\\'),
            b'/' => Ok('/'),
            b'b' => Ok('\u{0008}'),
            b'f' => Ok('\u{000c}'),
            b'n' => Ok('\n'),
            b'r' => Ok('\r'),
            b't' => Ok('\t'),
            b'u' => {
                let mut hex = [0u8; 4];
                for slot in &mut hex {
                    *slot = self.next()?;
                }
                let text = std::str::from_utf8(&hex).map_err(|_| FaultCode::Malformed)?;
                let code = u32::from_str_radix(text, 16).map_err(|_| FaultCode::Malformed)?;
                if (0xD800..=0xDFFF).contains(&code) {
                    return Err(FaultCode::Malformed);
                }
                char::from_u32(code).ok_or(FaultCode::Malformed)
            }
            _ => Err(FaultCode::Malformed),
        }
    }

    fn number(&mut self) -> Result<(), FaultCode> {
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        let start = self.index;
        self.digits()?;
        if self.index == start {
            return Err(FaultCode::Malformed);
        }
        if self.peek() == Some(b'.') {
            self.index += 1;
            let frac = self.index;
            self.digits()?;
            if self.index == frac {
                return Err(FaultCode::Malformed);
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let exp = self.index;
            self.digits()?;
            if self.index == exp {
                return Err(FaultCode::Malformed);
            }
        }
        Ok(())
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.index += 1;
        }
    }

    fn literal(&mut self, text: &[u8]) -> Result<(), FaultCode> {
        if self.bytes[self.index..].starts_with(text) {
            self.index += text.len();
            Ok(())
        } else {
            Err(FaultCode::Malformed)
        }
    }

    fn bump(&mut self, expected: u8) -> Result<(), FaultCode> {
        if self.peek() == Some(expected) {
            self.index += 1;
            Ok(())
        } else {
            Err(FaultCode::Malformed)
        }
    }

    fn next(&mut self) -> Result<u8, FaultCode> {
        let byte = self.peek().ok_or(FaultCode::Malformed)?;
        self.index += 1;
        Ok(byte)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn skip(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.index += 1;
        }
    }
}

fn utf8_width(byte: u8) -> Result<usize, FaultCode> {
    if byte < 0x80 {
        Ok(1)
    } else if byte & 0xE0 == 0xC0 {
        Ok(2)
    } else if byte & 0xF0 == 0xE0 {
        Ok(3)
    } else if byte & 0xF8 == 0xF0 {
        Ok(4)
    } else {
        Err(FaultCode::Malformed)
    }
}
