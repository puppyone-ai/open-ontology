//! Small JSON reader and writer for oracle documents. Duplicate keys are rejected.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<Val>),
    Obj(BTreeMap<String, Val>),
}

pub fn parse(input: &str) -> Result<Val, String> {
    let mut parser = Parser { bytes: input.as_bytes(), index: 0 };
    parser.skip();
    let value = parser.value()?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err("trailing".to_owned());
    }
    Ok(value)
}

impl Val {
    pub fn obj(&self) -> Option<&BTreeMap<String, Val>> {
        match self {
            Self::Obj(map) => Some(map),
            _ => None,
        }
    }

    pub fn get<'a>(&'a self, key: &str) -> Option<&'a Val> {
        self.obj()?.get(key)
    }

    pub fn string(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Self::Str(value) => Some(value),
            _ => None,
        }
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        match self.get(key)? {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn arr(&self, key: &str) -> Option<&[Val]> {
        match self.get(key)? {
            Self::Arr(value) => Some(value),
            _ => None,
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn skip(&mut self) {
        while matches!(self.bytes.get(self.index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }

    fn value(&mut self) -> Result<Val, String> {
        self.skip();
        match self.bytes.get(self.index).copied() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Val::Str(self.string()?)),
            Some(b't') => self.literal(b"true", Val::Bool(true)),
            Some(b'f') => self.literal(b"false", Val::Bool(false)),
            Some(b'n') => self.literal(b"null", Val::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err("value".to_owned()),
        }
    }

    fn literal(&mut self, text: &[u8], value: Val) -> Result<Val, String> {
        if self.bytes.get(self.index..self.index + text.len()) == Some(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err("literal".to_owned())
        }
    }

    fn object(&mut self) -> Result<Val, String> {
        self.index += 1;
        let mut map = BTreeMap::new();
        loop {
            self.skip();
            if self.bytes.get(self.index) == Some(&b'}') {
                self.index += 1;
                return Ok(Val::Obj(map));
            }
            if !map.is_empty() {
                self.comma()?;
            }
            self.skip();
            if self.bytes.get(self.index) != Some(&b'"') {
                return Err("key".to_owned());
            }
            let key = self.string()?;
            self.skip();
            if self.bytes.get(self.index) != Some(&b':') {
                return Err("colon".to_owned());
            }
            self.index += 1;
            let value = self.value()?;
            if map.insert(key, value).is_some() {
                return Err("duplicate".to_owned());
            }
            self.skip();
        }
    }

    fn array(&mut self) -> Result<Val, String> {
        self.index += 1;
        let mut items = Vec::new();
        loop {
            self.skip();
            if self.bytes.get(self.index) == Some(&b']') {
                self.index += 1;
                return Ok(Val::Arr(items));
            }
            if !items.is_empty() {
                self.comma()?;
            }
            items.push(self.value()?);
            self.skip();
        }
    }

    fn comma(&mut self) -> Result<(), String> {
        if self.bytes.get(self.index) != Some(&b',') {
            return Err("comma".to_owned());
        }
        self.index += 1;
        Ok(())
    }

    fn number(&mut self) -> Result<Val, String> {
        let start = self.index;
        if self.bytes.get(self.index) == Some(&b'-') {
            self.index += 1;
        }
        let digits = self.index;
        while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
            self.index += 1;
        }
        if self.index == digits {
            return Err("number".to_owned());
        }
        if matches!(self.bytes.get(self.index), Some(b'.' | b'e' | b'E')) {
            return Err("integer".to_owned());
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| "number".to_owned())?;
        if text != "0" && text != "-0" && (text.starts_with('0') || text.starts_with("-0")) {
            return Err("number".to_owned());
        }
        let value = text.parse::<i64>().map_err(|_| "number".to_owned())?;
        Ok(Val::Int(value))
    }

    fn string(&mut self) -> Result<String, String> {
        self.index += 1;
        let mut out = String::new();
        while let Some(byte) = self.bytes.get(self.index).copied() {
            self.index += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = self.bytes.get(self.index).copied().ok_or_else(|| "escape".to_owned())?;
                    self.index += 1;
                    match escaped {
                        b'"' | b'\\' | b'/' => out.push(escaped as char),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode()?),
                        _ => return Err("escape".to_owned()),
                    }
                }
                0x20..=0x7e => out.push(byte as char),
                _ => return Err("string".to_owned()),
            }
        }
        Err("string".to_owned())
    }

    fn unicode(&mut self) -> Result<char, String> {
        if self.index + 4 > self.bytes.len() {
            return Err("unicode".to_owned());
        }
        let text = std::str::from_utf8(&self.bytes[self.index..self.index + 4]).map_err(|_| "unicode".to_owned())?;
        self.index += 4;
        let code = u32::from_str_radix(text, 16).map_err(|_| "unicode".to_owned())?;
        char::from_u32(code).ok_or_else(|| "unicode".to_owned())
    }
}

pub fn escape(text: &str) -> String {
    let mut out = String::from("\"");
    for char in text.chars() {
        match char {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_ascii() && !c.is_control() => out.push(c),
            c => out.push_str(&format!("\\u{:04x}", u32::from(c))),
        }
    }
    out.push('"');
    out
}

pub fn string_list(items: &[String]) -> String {
    let mut out = String::from("[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&escape(item));
    }
    out.push(']');
    out
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn duplicate_key_is_rejected() {
        assert_eq!(parse(r#"{"a":1,"a":2}"#).unwrap_err(), "duplicate");
    }
}
