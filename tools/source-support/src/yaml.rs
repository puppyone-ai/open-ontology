//! Parser for the support-matrix YAML subset: block keys, block lists, and flow nodes.
//! Full-line `#` comments are ignored. This is not a general YAML loader.

#[derive(Clone, Debug, PartialEq)]
pub enum Yaml {
    Null,
    Bool(bool),
    Int(i64),
    String(String),
    Array(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YamlError;

impl Yaml {
    pub fn get<'a>(&'a self, key: &str) -> Option<&'a Yaml> {
        match self {
            Yaml::Map(items) => items.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn string(&self) -> Option<&str> {
        match self {
            Yaml::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn int(&self) -> Option<i64> {
        match self {
            Yaml::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn bool(&self) -> Option<bool> {
        match self {
            Yaml::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn array(&self) -> Option<&[Yaml]> {
        match self {
            Yaml::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn map(&self) -> Option<&[(String, Yaml)]> {
        match self {
            Yaml::Map(items) => Some(items),
            _ => None,
        }
    }
}

pub static PARSE_INDEX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn parse(text: &str) -> Result<Yaml, YamlError> {
    let mut parser = Parser { bytes: text.as_bytes(), index: 0 };
    let value = match parser.block_map() {
        Ok(value) => value,
        Err(error) => {
            PARSE_INDEX.store(parser.index, std::sync::atomic::Ordering::Relaxed);
            return Err(error);
        }
    };
    parser.skip();
    if parser.index != parser.bytes.len() {
        PARSE_INDEX.store(parser.index, std::sync::atomic::Ordering::Relaxed);
        return Err(YamlError);
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn block_map(&mut self) -> Result<Yaml, YamlError> {
        let mut items = Vec::new();
        loop {
            self.skip();
            if self.eof() {
                break;
            }
            if self.peek() == Some(b'-') {
                return Err(YamlError);
            }
            let key = self.plain_until(b':')?;
            self.bump(b':')?;
            self.skip_spaces();
            let value = if self.peek().is_none() || self.peek() == Some(b'\n') || self.peek() == Some(b'\r') {
                self.skip();
                if self.at_list_item() {
                    self.block_list()?
                } else {
                    return Err(YamlError);
                }
            } else {
                self.value()?
            };
            if items.iter().any(|(name, _)| name == &key) {
                return Err(YamlError);
            }
            items.push((key, value));
        }
        Ok(Yaml::Map(items))
    }

    fn block_list(&mut self) -> Result<Yaml, YamlError> {
        let mut items = Vec::new();
        loop {
            self.skip();
            if !self.at_list_item() {
                break;
            }
            self.bump(b'-')?;
            self.skip_spaces();
            items.push(self.value()?);
        }
        Ok(Yaml::Array(items))
    }

    fn at_list_item(&self) -> bool {
        self.peek() == Some(b'-') && matches!(self.bytes.get(self.index + 1).copied(), Some(b' ' | b'\t' | b'{' | b'[' | b'"' | b'\''))
    }

    fn value(&mut self) -> Result<Yaml, YamlError> {
        self.skip_spaces();
        match self.peek() {
            Some(b'{') => self.flow_map(),
            Some(b'[') => self.flow_seq(),
            Some(b'"') => Ok(Yaml::String(self.double_quote()?)),
            Some(b'\'') => Ok(Yaml::String(self.single_quote()?)),
            Some(_) => self.plain(),
            None => Err(YamlError),
        }
    }

    fn flow_map(&mut self) -> Result<Yaml, YamlError> {
        self.bump(b'{')?;
        let mut items = Vec::new();
        self.skip();
        if self.eat(b'}') {
            return Ok(Yaml::Map(items));
        }
        loop {
            self.skip();
            let key = if self.peek() == Some(b'"') {
                self.double_quote()?
            } else if self.peek() == Some(b'\'') {
                self.single_quote()?
            } else {
                self.plain_until(b':')?
            };
            self.skip_spaces();
            self.bump(b':')?;
            self.skip();
            let value = self.value()?;
            if items.iter().any(|(name, _)| name == &key) {
                return Err(YamlError);
            }
            items.push((key, value));
            self.skip();
            if self.eat(b',') {
                self.skip();
                if self.eat(b'}') {
                    break;
                }
                continue;
            }
            self.bump(b'}')?;
            break;
        }
        Ok(Yaml::Map(items))
    }

    fn flow_seq(&mut self) -> Result<Yaml, YamlError> {
        self.bump(b'[')?;
        let mut items = Vec::new();
        self.skip();
        if self.eat(b']') {
            return Ok(Yaml::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip();
            if self.eat(b',') {
                self.skip();
                if self.eat(b']') {
                    break;
                }
                continue;
            }
            self.bump(b']')?;
            break;
        }
        Ok(Yaml::Array(items))
    }

    fn double_quote(&mut self) -> Result<String, YamlError> {
        self.bump(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self.next().ok_or(YamlError)?;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = self.next().ok_or(YamlError)?;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        other => out.push(other as char),
                    }
                }
                _ => {
                    self.index -= 1;
                    out.push(self.utf8_char()?);
                }
            }
        }
    }

    fn single_quote(&mut self) -> Result<String, YamlError> {
        self.bump(b'\'')?;
        let mut out = String::new();
        loop {
            let byte = self.next().ok_or(YamlError)?;
            if byte == b'\'' {
                if self.eat(b'\'') {
                    out.push('\'');
                    continue;
                }
                return Ok(out);
            }
            self.index -= 1;
            out.push(self.utf8_char()?);
        }
    }

    fn plain(&mut self) -> Result<Yaml, YamlError> {
        let text = self.plain_text(|byte| matches!(byte, b',' | b'}' | b']' | b'#' | b'\n' | b'\r'))?;
        Ok(classify(&text))
    }

    fn plain_until(&mut self, stop: u8) -> Result<String, YamlError> {
        self.plain_text(|byte| byte == stop || matches!(byte, b',' | b'}' | b']' | b'#' | b'\n' | b'\r' | b'{' | b'['))
    }

    fn plain_text(&mut self, stop: impl Fn(u8) -> bool) -> Result<String, YamlError> {
        let start = self.index;
        if self.eof() || stop(self.peek().unwrap_or(0)) {
            return Err(YamlError);
        }
        while let Some(byte) = self.peek() {
            if stop(byte) {
                break;
            }
            self.utf8_char()?;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| YamlError)?.trim().to_string();
        if text.is_empty() {
            return Err(YamlError);
        }
        Ok(text)
    }

    fn utf8_char(&mut self) -> Result<char, YamlError> {
        let start = self.index;
        let byte = self.next().ok_or(YamlError)?;
        let width = if byte < 0x80 {
            1
        } else if byte & 0b1110_0000 == 0b1100_0000 {
            2
        } else if byte & 0b1111_0000 == 0b1110_0000 {
            3
        } else if byte & 0b1111_1000 == 0b1111_0000 {
            4
        } else {
            return Err(YamlError);
        };
        if self.index + width - 1 > self.bytes.len() {
            return Err(YamlError);
        }
        self.index += width - 1;
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| YamlError)?;
        text.chars().next().ok_or(YamlError)
    }

    fn skip(&mut self) {
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.index += 1;
            }
            if self.peek() == Some(b'#') {
                while let Some(byte) = self.peek() {
                    self.index += 1;
                    if byte == b'\n' {
                        break;
                    }
                }
                continue;
            }
            break;
        }
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.index += 1;
        }
    }

    fn eof(&self) -> bool {
        self.index >= self.bytes.len()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.index += 1;
        Some(byte)
    }

    fn eat(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn bump(&mut self, expected: u8) -> Result<(), YamlError> {
        if self.eat(expected) { Ok(()) } else { Err(YamlError) }
    }
}

fn classify(text: &str) -> Yaml {
    match text {
        "true" => Yaml::Bool(true),
        "false" => Yaml::Bool(false),
        "null" | "~" => Yaml::Null,
        "0" => Yaml::Int(0),
        _ => {
            if text.bytes().all(|byte| byte.is_ascii_digit()) && !text.starts_with('0') {
                if let Ok(number) = text.parse::<i64>() {
                    return Yaml::Int(number);
                }
            }
            if let Some(rest) = text.strip_prefix('-') {
                if rest.bytes().all(|byte| byte.is_ascii_digit()) && !rest.is_empty() && !rest.starts_with('0') {
                    if let Ok(number) = text.parse::<i64>() {
                        return Yaml::Int(number);
                    }
                }
            }
            Yaml::String(text.to_string())
        }
    }
}
