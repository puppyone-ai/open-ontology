//! One JSON value parser for producer documents. Numbers stay decimal text.

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Val>),
    Object(Vec<(String, Val)>),
}

pub fn parse(input: &str) -> Result<Val, String> {
    let mut parser = Parser {
        text: input,
        index: 0,
    };
    parser.skip();
    if parser.index < parser.text.len() && parser.text.as_bytes()[parser.index] == 0xef {
        return Err("bom".to_string());
    }
    let value = parser.value()?;
    parser.skip();
    if parser.index != parser.text.len() {
        return Err("trailing".to_string());
    }
    Ok(value)
}

pub fn parse_lines(input: &str) -> Result<Vec<Val>, String> {
    let mut rows = Vec::new();
    for (number, line) in input.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        rows.push(parse(trimmed).map_err(|err| format!("line {}: {err}", number + 1))?);
    }
    Ok(rows)
}

impl Val {
    pub fn get(&self, key: &str) -> Option<&Val> {
        let Val::Object(fields) = self else {
            return None;
        };
        fields.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Val::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        let Val::Number(text) = self else {
            return None;
        };
        if text.starts_with('-') || text.contains('.') || text.contains('e') || text.contains('E') {
            return None;
        }
        text.parse().ok()
    }

    pub fn as_array(&self) -> Option<&[Val]> {
        match self {
            Val::Array(items) => Some(items),
            _ => None,
        }
    }
}

pub fn render(value: &Val) -> String {
    let mut out = String::new();
    write_val(&mut out, value);
    out
}

fn write_val(out: &mut String, value: &Val) {
    match value {
        Val::Null => out.push_str("null"),
        Val::Bool(true) => out.push_str("true"),
        Val::Bool(false) => out.push_str("false"),
        Val::Number(text) => out.push_str(text),
        Val::String(text) => write_string(out, text),
        Val::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_val(out, item);
            }
            out.push(']');
        }
        Val::Object(fields) => {
            out.push('{');
            for (index, (key, item)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_val(out, item);
            }
            out.push('}');
        }
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    text: &'a str,
    index: usize,
}

impl<'a> Parser<'a> {
    fn skip(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == ' ' || ch == '\n' || ch == '\r' || ch == '\t' {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.text[self.index..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += ch.len_utf8();
        Some(ch)
    }

    fn value(&mut self) -> Result<Val, String> {
        self.skip();
        match self.peek() {
            Some('n') => self.literal("null", Val::Null),
            Some('t') => self.literal("true", Val::Bool(true)),
            Some('f') => self.literal("false", Val::Bool(false)),
            Some('"') => Ok(Val::String(self.string()?)),
            Some('[') => self.array(),
            Some('{') => self.object(),
            Some('-') | Some('0'..='9') => self.number(),
            _ => Err("value".to_string()),
        }
    }

    fn literal(&mut self, text: &str, value: Val) -> Result<Val, String> {
        if self.text[self.index..].starts_with(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err("literal".to_string())
        }
    }

    fn number(&mut self) -> Result<Val, String> {
        let start = self.index;
        if self.peek() == Some('-') {
            self.bump();
        }
        match self.peek() {
            Some('0') => {
                self.bump();
            }
            Some('1'..='9') => {
                while matches!(self.peek(), Some('0'..='9')) {
                    self.bump();
                }
            }
            _ => return Err("number".to_string()),
        }
        if self.peek() == Some('.') {
            self.bump();
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err("number".to_string());
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            self.bump();
            if matches!(self.peek(), Some('+') | Some('-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err("number".to_string());
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
        }
        Ok(Val::Number(self.text[start..self.index].to_string()))
    }

    fn string(&mut self) -> Result<String, String> {
        if self.bump() != Some('"') {
            return Err("string".to_string());
        }
        let mut out = String::new();
        loop {
            match self.bump() {
                Some('"') => return Ok(out),
                Some('\\') => {
                    let escaped = self.bump().ok_or_else(|| "escape".to_string())?;
                    match escaped {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => out.push(self.hex4()?),
                        _ => return Err("escape".to_string()),
                    }
                }
                Some(ch) if (ch as u32) >= 0x20 => out.push(ch),
                _ => return Err("string".to_string()),
            }
        }
    }

    fn hex4(&mut self) -> Result<char, String> {
        let mut code = 0u32;
        for _ in 0..4 {
            let ch = self.bump().ok_or_else(|| "hex".to_string())?;
            let digit = ch.to_digit(16).ok_or_else(|| "hex".to_string())?;
            code = (code << 4) | digit;
        }
        char::from_u32(code).ok_or_else(|| "hex".to_string())
    }

    fn array(&mut self) -> Result<Val, String> {
        self.bump();
        let mut items = Vec::new();
        loop {
            self.skip();
            if self.peek() == Some(']') {
                self.bump();
                return Ok(Val::Array(items));
            }
            if !items.is_empty() {
                if self.peek() != Some(',') {
                    return Err("array".to_string());
                }
                self.bump();
                self.skip();
            }
            items.push(self.value()?);
        }
    }

    fn object(&mut self) -> Result<Val, String> {
        self.bump();
        let mut fields = Vec::new();
        loop {
            self.skip();
            if self.peek() == Some('}') {
                self.bump();
                return Ok(Val::Object(fields));
            }
            if !fields.is_empty() {
                if self.peek() != Some(',') {
                    return Err("object".to_string());
                }
                self.bump();
                self.skip();
            }
            if self.peek() != Some('"') {
                return Err("key".to_string());
            }
            let key = self.string()?;
            self.skip();
            if self.bump() != Some(':') {
                return Err("colon".to_string());
            }
            if fields.iter().any(|(name, _)| name == &key) {
                return Err("duplicate".to_string());
            }
            fields.push((key, self.value()?));
        }
    }
}
