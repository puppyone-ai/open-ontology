//! One JSON value per RPC record. No serde.

use crate::{Error, Kind};
use std::str;

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
        let Value::Object(fields) = self else {
            return None;
        };
        fields.iter().find(|(name, _)| name == key).map(|(_, value)| value)
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
}

pub fn parse_value(input: &[u8]) -> Result<Value, Error> {
    let (value, next) = parse_at(input, 0, 0)?;
    let next = skip(input, next)?;
    if next != input.len() {
        return Err(shape("trailing bytes"));
    }
    Ok(value)
}

fn parse_at(input: &[u8], index: usize, depth: usize) -> Result<(Value, usize), Error> {
    if depth > 32 {
        return Err(shape("nesting limit"));
    }
    let index = skip(input, index)?;
    let byte = *input.get(index).ok_or_else(|| shape("truncated"))?;
    match byte {
        b'n' => literal(input, index, b"null").map(|next| (Value::Null, next)),
        b't' => literal(input, index, b"true").map(|next| (Value::Bool(true), next)),
        b'f' => literal(input, index, b"false").map(|next| (Value::Bool(false), next)),
        b'"' => parse_string(input, index).map(|(text, next)| (Value::String(text), next)),
        b'[' => parse_array(input, index, depth),
        b'{' => parse_object(input, index, depth),
        b'-' | b'0'..=b'9' => parse_number(input, index),
        _ => Err(shape("unknown value")),
    }
}

fn parse_array(input: &[u8], index: usize, depth: usize) -> Result<(Value, usize), Error> {
    let mut index = index + 1;
    let mut items = Vec::new();
    loop {
        index = skip(input, index)?;
        if input.get(index) == Some(&b']') {
            return Ok((Value::Array(items), index + 1));
        }
        if !items.is_empty() {
            if input.get(index) != Some(&b',') {
                return Err(shape("array comma"));
            }
            index = skip(input, index + 1)?;
        }
        let (item, next) = parse_at(input, index, depth + 1)?;
        items.push(item);
        index = next;
    }
}

fn parse_object(input: &[u8], index: usize, depth: usize) -> Result<(Value, usize), Error> {
    let mut index = index + 1;
    let mut fields = Vec::new();
    loop {
        index = skip(input, index)?;
        if input.get(index) == Some(&b'}') {
            return Ok((Value::Object(fields), index + 1));
        }
        if !fields.is_empty() {
            if input.get(index) != Some(&b',') {
                return Err(shape("object comma"));
            }
            index = skip(input, index + 1)?;
        }
        if input.get(index) != Some(&b'"') {
            return Err(shape("object key"));
        }
        let (key, next) = parse_string(input, index)?;
        index = skip(input, next)?;
        if input.get(index) != Some(&b':') {
            return Err(shape("object colon"));
        }
        let (value, next) = parse_at(input, index + 1, depth + 1)?;
        fields.push((key, value));
        index = next;
    }
}

fn parse_number(input: &[u8], start: usize) -> Result<(Value, usize), Error> {
    let mut index = start;
    if input.get(index) == Some(&b'-') {
        index += 1;
    }
    let digits = index;
    while matches!(input.get(index), Some(b'0'..=b'9')) {
        index += 1;
    }
    if index == digits {
        return Err(shape("bad number"));
    }
    if input.get(index) == Some(&b'.') {
        index += 1;
        let frac = index;
        while matches!(input.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == frac {
            return Err(shape("bad fraction"));
        }
    }
    if matches!(input.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(input.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let exp = index;
        while matches!(input.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == exp {
            return Err(shape("bad exponent"));
        }
    }
    let text = str::from_utf8(&input[start..index]).map_err(|_| shape("number utf8"))?;
    Ok((Value::Number(text.to_owned()), index))
}

fn parse_string(input: &[u8], start: usize) -> Result<(String, usize), Error> {
    let mut index = start + 1;
    let mut out = String::new();
    while index < input.len() {
        let byte = input[index];
        if byte == b'"' {
            return Ok((out, index + 1));
        }
        if byte == b'\\' {
            index += 1;
            let esc = *input.get(index).ok_or_else(|| shape("truncated escape"))?;
            match esc {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'b' => out.push('\u{0008}'),
                b'f' => out.push('\u{000c}'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' => {
                    let (code, next) = hex4(input, index + 1)?;
                    let ch = char::from_u32(code).ok_or_else(|| shape("bad unicode"))?;
                    out.push(ch);
                    index = next;
                    continue;
                }
                _ => return Err(shape("bad escape")),
            }
            index += 1;
            continue;
        }
        if byte < 0x20 {
            return Err(shape("raw control in string"));
        }
        let width = utf8_width(byte).ok_or_else(|| shape("bad utf8"))?;
        if index + width > input.len() {
            return Err(shape("truncated utf8"));
        }
        let text = str::from_utf8(&input[index..index + width]).map_err(|_| shape("bad utf8"))?;
        out.push_str(text);
        index += width;
    }
    Err(shape("unterminated string"))
}

fn hex4(input: &[u8], start: usize) -> Result<(u32, usize), Error> {
    if start + 4 > input.len() {
        return Err(shape("short unicode"));
    }
    let mut code = 0u32;
    for byte in &input[start..start + 4] {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return Err(shape("bad unicode hex")),
        };
        code = (code << 4) | u32::from(digit);
    }
    Ok((code, start + 4))
}

fn utf8_width(byte: u8) -> Option<usize> {
    if byte < 0x80 {
        Some(1)
    } else if byte & 0b1110_0000 == 0b1100_0000 {
        Some(2)
    } else if byte & 0b1111_0000 == 0b1110_0000 {
        Some(3)
    } else if byte & 0b1111_1000 == 0b1111_0000 {
        Some(4)
    } else {
        None
    }
}

fn literal(input: &[u8], index: usize, lit: &[u8]) -> Result<usize, Error> {
    if input[index..].starts_with(lit) {
        Ok(index + lit.len())
    } else {
        Err(shape("bad literal"))
    }
}

fn skip(input: &[u8], mut index: usize) -> Result<usize, Error> {
    while matches!(input.get(index), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        index += 1;
    }
    Ok(index)
}

fn shape(detail: &str) -> Error {
    Error::new(Kind::Shape, detail)
}
