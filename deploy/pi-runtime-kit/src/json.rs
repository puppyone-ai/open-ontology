//! One JSON value parser for the Pi records this kit reads.
//! Strings accept the usual escapes. Numbers stay text.

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        let Json::Object(fields) = self else {
            return None;
        };
        fields.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn compact(&self) -> String {
        let mut out = String::new();
        write_compact(self, &mut out);
        out
    }
}

pub fn parse(input: &str) -> Result<Json, String> {
    let mut parser = Parser { bytes: input.as_bytes(), index: 0 };
    parser.skip();
    let value = parser.value()?;
    parser.skip();
    if parser.index != parser.bytes.len() {
        return Err("trailing json".to_owned());
    }
    Ok(value)
}

pub fn obj(fields: Vec<(&str, Json)>) -> Json {
    Json::Object(fields.into_iter().map(|(key, value)| (key.to_owned(), value)).collect())
}

pub fn array(items: Vec<Json>) -> Json {
    Json::Array(items)
}

pub fn string(text: impl Into<String>) -> Json {
    Json::String(text.into())
}

pub fn boolean(value: bool) -> Json {
    Json::Bool(value)
}

pub fn escape(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
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

    fn value(&mut self) -> Result<Json, String> {
        self.skip();
        match self.bytes.get(self.index).copied() {
            Some(b'n') => self.literal(b"null", Json::Null),
            Some(b't') => self.literal(b"true", Json::Bool(true)),
            Some(b'f') => self.literal(b"false", Json::Bool(false)),
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err("expected json value".to_owned()),
        }
    }

    fn literal(&mut self, text: &[u8], value: Json) -> Result<Json, String> {
        if self.bytes.get(self.index..self.index + text.len()) == Some(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err("bad literal".to_owned())
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.index;
        if self.bytes.get(self.index) == Some(&b'-') {
            self.index += 1;
        }
        let digits = self.index;
        while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
            self.index += 1;
        }
        if self.index == digits {
            return Err("bad number".to_owned());
        }
        if self.bytes.get(self.index) == Some(&b'.') {
            self.index += 1;
            let fraction = self.index;
            while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == fraction {
                return Err("bad fraction".to_owned());
            }
        }
        if matches!(self.bytes.get(self.index), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.bytes.get(self.index), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let exponent = self.index;
            while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == exponent {
                return Err("bad exponent".to_owned());
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| "number utf-8".to_owned())?;
        Ok(Json::Number(text.to_owned()))
    }

    fn string(&mut self) -> Result<String, String> {
        if self.bytes.get(self.index) != Some(&b'"') {
            return Err("expected string".to_owned());
        }
        self.index += 1;
        let mut out = String::new();
        while let Some(byte) = self.bytes.get(self.index).copied() {
            self.index += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => out.push(self.escape()?),
                byte if byte < 0x20 => return Err("raw control in string".to_owned()),
                byte => {
                    let width = utf8_width(byte)?;
                    let start = self.index - 1;
                    let end = start + width;
                    let slice = self.bytes.get(start..end).ok_or("truncated utf-8")?;
                    let text = std::str::from_utf8(slice).map_err(|_| "bad utf-8".to_owned())?;
                    out.push_str(text);
                    self.index = end;
                }
            }
        }
        Err("unterminated string".to_owned())
    }

    fn escape(&mut self) -> Result<char, String> {
        let byte = self.bytes.get(self.index).copied().ok_or("truncated escape")?;
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
            b'u' => self.unicode_escape(),
            _ => Err("bad escape".to_owned()),
        }
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let start = self.index;
        let end = start + 4;
        let hex = self.bytes.get(start..end).ok_or("truncated unicode")?;
        self.index = end;
        let text = std::str::from_utf8(hex).map_err(|_| "unicode utf-8".to_owned())?;
        let code = u32::from_str_radix(text, 16).map_err(|_| "bad unicode".to_owned())?;
        char::from_u32(code).ok_or_else(|| "unicode scalar".to_owned())
    }

    fn array(&mut self) -> Result<Json, String> {
        self.index += 1;
        let mut items = Vec::new();
        loop {
            self.skip();
            if self.bytes.get(self.index) == Some(&b']') {
                self.index += 1;
                return Ok(Json::Array(items));
            }
            if !items.is_empty() && self.bytes.get(self.index) != Some(&b',') {
                return Err("expected comma".to_owned());
            }
            if !items.is_empty() {
                self.index += 1;
            }
            items.push(self.value()?);
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.index += 1;
        let mut fields = Vec::new();
        loop {
            self.skip();
            if self.bytes.get(self.index) == Some(&b'}') {
                self.index += 1;
                return Ok(Json::Object(fields));
            }
            if !fields.is_empty() && self.bytes.get(self.index) != Some(&b',') {
                return Err("expected comma".to_owned());
            }
            if !fields.is_empty() {
                self.index += 1;
                self.skip();
            }
            let key = self.string()?;
            self.skip();
            if self.bytes.get(self.index) != Some(&b':') {
                return Err("expected colon".to_owned());
            }
            self.index += 1;
            fields.push((key, self.value()?));
        }
    }
}

fn utf8_width(byte: u8) -> Result<usize, String> {
    if byte < 0x80 {
        Ok(1)
    } else if byte & 0b1110_0000 == 0b1100_0000 {
        Ok(2)
    } else if byte & 0b1111_0000 == 0b1110_0000 {
        Ok(3)
    } else if byte & 0b1111_1000 == 0b1111_0000 {
        Ok(4)
    } else {
        Err("bad utf-8 lead".to_owned())
    }
}

fn write_compact(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(text) => out.push_str(text),
        Json::String(text) => out.push_str(&escape(text)),
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_compact(item, out);
            }
            out.push(']');
        }
        Json::Object(fields) => {
            out.push('{');
            for (index, (key, item)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&escape(key));
                out.push(':');
                write_compact(item, out);
            }
            out.push('}');
        }
    }
}
