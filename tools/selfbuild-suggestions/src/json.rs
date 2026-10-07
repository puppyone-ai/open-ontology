//! Bounded JSON with duplicate-key rejection and canonical SHA-256.

use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

pub const DOCUMENT_MAX_BYTES: usize = 2_097_152;
const MAX_DEPTH: usize = 40;
const MAX_NODES: usize = 200_000;

pub fn parse_document(bytes: &[u8]) -> Result<Value, &'static str> {
    if bytes.is_empty() {
        return Err("empty");
    }
    if bytes.len() > DOCUMENT_MAX_BYTES {
        return Err("stdin_too_large");
    }
    if !bytes.iter().any(|byte| !byte.is_ascii_whitespace()) {
        return Err("empty");
    }
    let mut parser = Parser {
        bytes,
        index: 0,
        depth: 0,
        nodes: 0,
    };
    parser.skip_ws()?;
    let value = parser.value()?;
    parser.skip_ws()?;
    if parser.index != parser.bytes.len() {
        return Err("trailing");
    }
    Ok(value)
}

pub fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(&mut out, value);
    out
}

pub fn canonical_sha256(value: &Value) -> String {
    sha256_hex(canonical(value).as_bytes())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(hex_digit(byte >> 4));
        hex.push(hex_digit(byte & 0x0f));
    }
    hex
}

pub fn candidate_id(project: &str, kind: &str, key: &str) -> String {
    let mut preimage = String::new();
    preimage.push_str("oo.selfbuild-suggestions.candidate/v1\n");
    preimage.push_str(project);
    preimage.push('\n');
    preimage.push_str(kind);
    preimage.push('\n');
    preimage.push_str(key);
    preimage.push('\n');
    let hex = sha256_hex(preimage.as_bytes());
    format!("cand_{}", &hex[..32])
}

fn hex_digit(nibble: u8) -> char {
    char::from(b"0123456789abcdef"[usize::from(nibble)])
}

fn write_canonical(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                push_string(out, key);
                out.push(':');
                write_canonical(out, &map[*key]);
            }
            out.push('}');
        }
    }
}

fn push_string(out: &mut String, text: &str) {
    out.push_str(&serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string()));
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    depth: usize,
    nodes: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) -> Result<(), &'static str> {
        while let Some(byte) = self.peek() {
            if byte.is_ascii_whitespace() {
                self.index += 1;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn bump(&mut self) -> Result<u8, &'static str> {
        let byte = self.peek().ok_or("json")?;
        self.index += 1;
        Ok(byte)
    }

    fn enter(&mut self) -> Result<(), &'static str> {
        self.nodes += 1;
        if self.nodes > MAX_NODES || self.depth > MAX_DEPTH {
            return Err("budget");
        }
        Ok(())
    }

    fn value(&mut self) -> Result<Value, &'static str> {
        self.enter()?;
        match self.peek().ok_or("json")? {
            b'n' => self.literal(b"null", Value::Null),
            b't' => self.literal(b"true", Value::Bool(true)),
            b'f' => self.literal(b"false", Value::Bool(false)),
            b'"' => Ok(Value::String(self.string()?)),
            b'[' => self.array(),
            b'{' => self.object(),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err("json"),
        }
    }

    fn literal(&mut self, text: &[u8], value: Value) -> Result<Value, &'static str> {
        for expected in text {
            if self.bump()? != *expected {
                return Err("json");
            }
        }
        Ok(value)
    }

    fn array(&mut self) -> Result<Value, &'static str> {
        self.bump()?;
        let mut items = Vec::new();
        self.depth += 1;
        self.skip_ws()?;
        if self.peek() == Some(b']') {
            self.bump()?;
            self.depth -= 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws()?;
            match self.bump()? {
                b']' => break,
                b',' => self.skip_ws()?,
                _ => return Err("json"),
            }
        }
        self.depth -= 1;
        Ok(Value::Array(items))
    }

    fn object(&mut self) -> Result<Value, &'static str> {
        self.bump()?;
        let mut map = Map::new();
        let mut keys = Vec::new();
        self.depth += 1;
        self.skip_ws()?;
        if self.peek() == Some(b'}') {
            self.bump()?;
            self.depth -= 1;
            return Ok(Value::Object(map));
        }
        loop {
            if self.peek() != Some(b'"') {
                return Err("json");
            }
            let key = self.string()?;
            if keys.iter().any(|seen| seen == &key) {
                return Err("duplicate_key");
            }
            keys.push(key.clone());
            self.skip_ws()?;
            if self.bump()? != b':' {
                return Err("json");
            }
            self.skip_ws()?;
            map.insert(key, self.value()?);
            self.skip_ws()?;
            match self.bump()? {
                b'}' => break,
                b',' => self.skip_ws()?,
                _ => return Err("json"),
            }
        }
        self.depth -= 1;
        Ok(Value::Object(map))
    }

    fn string(&mut self) -> Result<String, &'static str> {
        if self.bump()? != b'"' {
            return Err("json");
        }
        let start = self.index;
        let mut escaped = false;
        while let Some(byte) = self.peek() {
            self.index += 1;
            if escaped {
                escaped = false;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == b'"' {
                let raw = &self.bytes[start..self.index - 1];
                let encoded = String::from_utf8(raw.to_vec()).map_err(|_| "json")?;
                let wrapped = format!("\"{encoded}\"");
                let value: Value = serde_json::from_str(&wrapped).map_err(|_| "json")?;
                return match value {
                    Value::String(text) => Ok(text),
                    _ => Err("json"),
                };
            }
            if byte < 0x20 {
                return Err("json");
            }
        }
        Err("json")
    }

    fn number(&mut self) -> Result<Value, &'static str> {
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
            _ => return Err("json"),
        }
        if self.peek() == Some(b'.') {
            self.index += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err("json");
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
                return Err("json");
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| "json")?;
        let number = text.parse::<Number>().map_err(|_| "json")?;
        Ok(Value::Number(number))
    }
}

#[cfg(test)]
mod tests {
    use super::{candidate_id, canonical_sha256, parse_document};
    use serde_json::json;

    #[test]
    fn canonical_sha_matches_the_independent_oracle() {
        let value = json!({"b": "x", "a": 1});
        assert_eq!(
            canonical_sha256(&value),
            "ecf9e98ec0641e23113ff3ce8bdc78d0ddd249886517fd4a7f68cc83d4e65667"
        );
    }

    #[test]
    fn candidate_id_matches_the_independent_oracle() {
        assert_eq!(
            candidate_id("public", "metric", "logistics.shipment.amount"),
            "cand_5544a04dbfef7e7def73d6ac050b29d3"
        );
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let error = parse_document(br#"{"a":1,"a":2}"#).unwrap_err();
        assert_eq!(error, "duplicate_key");
    }
}
