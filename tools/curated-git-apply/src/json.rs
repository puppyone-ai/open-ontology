//! Strict JSON for the operator request.
//!
//! Duplicate keys, deep nesting, and trailing bytes are rejected. The inner
//! review command is a single string and is not interpreted here.

use serde_json::{Map, Number, Value};

use crate::error::CuratedGitError;

const MAX_BYTES: usize = 2_097_152;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 200_000;

pub fn parse_request_json(input: &str) -> Result<Value, CuratedGitError> {
    if input.len() > MAX_BYTES {
        return Err(CuratedGitError::new(
            "json.budget.bytes",
            "document exceeds the byte budget",
        ));
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
        return Err(syntax());
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    nodes: usize,
}

impl<'a> Parser<'a> {
    fn value(&mut self, depth: usize) -> Result<Value, CuratedGitError> {
        if depth > MAX_DEPTH {
            return Err(syntax());
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(CuratedGitError::new(
                "json.budget.bytes",
                "document exceeds the byte budget",
            ));
        }
        self.skip();
        let Some(byte) = self.peek() else {
            return Err(syntax());
        };
        match byte {
            b'n' => self.literal(b"null", Value::Null),
            b't' => self.literal(b"true", Value::Bool(true)),
            b'f' => self.literal(b"false", Value::Bool(false)),
            b'"' => Ok(Value::String(self.string()?)),
            b'[' => self.array(depth),
            b'{' => self.object(depth),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(syntax()),
        }
    }

    fn literal(&mut self, text: &[u8], value: Value) -> Result<Value, CuratedGitError> {
        if self.bytes[self.index..].starts_with(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err(syntax())
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, CuratedGitError> {
        self.index += 1;
        let mut items = Vec::new();
        self.skip();
        if self.peek() == Some(b']') {
            self.index += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.index += 1;
                    self.skip();
                }
                Some(b']') => {
                    self.index += 1;
                    break;
                }
                _ => return Err(syntax()),
            }
        }
        Ok(Value::Array(items))
    }

    fn object(&mut self, depth: usize) -> Result<Value, CuratedGitError> {
        self.index += 1;
        let mut map = Map::new();
        self.skip();
        if self.peek() == Some(b'}') {
            self.index += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.skip();
            if self.peek() != Some(b'"') {
                return Err(syntax());
            }
            let key = self.string()?;
            self.skip();
            if self.peek() != Some(b':') {
                return Err(syntax());
            }
            self.index += 1;
            let value = self.value(depth + 1)?;
            if map.insert(key, value).is_some() {
                return Err(CuratedGitError::new(
                    "schema.invalid",
                    "the request is not the curated git apply command",
                ));
            }
            self.skip();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    break;
                }
                _ => return Err(syntax()),
            }
        }
        Ok(Value::Object(map))
    }

    fn number(&mut self) -> Result<Value, CuratedGitError> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        match self.peek() {
            Some(b'0') => self.index += 1,
            Some(b'1'..=b'9') => {
                self.index += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.index += 1;
                }
            }
            _ => return Err(syntax()),
        }
        if self.peek() == Some(b'.') {
            self.index += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(syntax());
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(syntax());
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        }
        let lexeme = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| syntax())?;
        let number = lexeme.parse::<Number>().map_err(|_| syntax())?;
        Ok(Value::Number(number))
    }

    fn string(&mut self) -> Result<String, CuratedGitError> {
        self.index += 1;
        let mut out = String::new();
        while let Some(byte) = self.peek() {
            match byte {
                b'"' => {
                    self.index += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.index += 1;
                    out.push(self.escape()?);
                }
                0x00..=0x1f => return Err(syntax()),
                _ => {
                    let ch = self.utf8()?;
                    out.push(ch);
                }
            }
        }
        Err(syntax())
    }

    fn escape(&mut self) -> Result<char, CuratedGitError> {
        let Some(byte) = self.peek() else {
            return Err(syntax());
        };
        self.index += 1;
        match byte {
            b'"' => Ok('"'),
            b'\\' => Ok('\\'),
            b'/' => Ok('/'),
            b'b' => Ok('\u{0008}'),
            b'f' => Ok('\u{000c}'),
            b'n' => Ok('\n'),
            b'r' => Ok('\r'),
            b't' => Ok('\t'),
            b'u' => self.unicode(),
            _ => Err(syntax()),
        }
    }

    fn unicode(&mut self) -> Result<char, CuratedGitError> {
        let unit = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.peek() != Some(b'\\') {
                return Err(syntax());
            }
            self.index += 1;
            if self.peek() != Some(b'u') {
                return Err(syntax());
            }
            self.index += 1;
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(syntax());
            }
            let point = 0x10000 + (((unit - 0xD800) as u32) << 10) + (low - 0xDC00) as u32;
            return char::from_u32(point).ok_or_else(syntax);
        }
        if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(syntax());
        }
        char::from_u32(unit as u32).ok_or_else(syntax)
    }

    fn hex4(&mut self) -> Result<u16, CuratedGitError> {
        if self.index + 4 > self.bytes.len() {
            return Err(syntax());
        }
        let text = std::str::from_utf8(&self.bytes[self.index..self.index + 4]).map_err(|_| syntax())?;
        self.index += 4;
        u16::from_str_radix(text, 16).map_err(|_| syntax())
    }

    fn utf8(&mut self) -> Result<char, CuratedGitError> {
        let rest = &self.bytes[self.index..];
        let text = std::str::from_utf8(rest).map_err(|_| syntax())?;
        let ch = text.chars().next().ok_or_else(syntax)?;
        self.index += ch.len_utf8();
        Ok(ch)
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

fn syntax() -> CuratedGitError {
    CuratedGitError::new("json.syntax", "the request is not JSON")
}

#[cfg(test)]
mod tests {
    use super::parse_request_json;
    use serde_json::json;

    #[test]
    fn duplicate_key_and_trailing_data_fail() {
        assert_eq!(
            parse_request_json(r#"{"a":1,"a":2}"#).unwrap_err().code,
            "schema.invalid"
        );
        assert_eq!(
            parse_request_json(r#"{"a":1}{}"#).unwrap_err().code,
            "json.syntax"
        );
        assert_eq!(parse_request_json(r#"{"b":1,"a":"x"}"#).unwrap(), json!({"b": 1, "a": "x"}));
    }
}
