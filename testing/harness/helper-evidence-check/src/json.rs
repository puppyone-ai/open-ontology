use crate::fail::{self, Fail, MALFORMED};

#[derive(Clone, Debug)]
pub enum Val {
    Null,
    Bool(bool),
    Int(i64),
    Frac,
    String(String),
    Array(Vec<Val>),
    Object(Vec<(String, Val)>),
}

impl Val {
    pub fn get(&self, key: &str) -> Option<&Val> {
        self.as_object()
            .ok()
            .and_then(|pairs| pairs.iter().find(|(name, _)| name == key).map(|(_, value)| value))
    }

    pub fn as_object(&self) -> Result<&[(String, Val)], Fail> {
        match self {
            Val::Object(pairs) => Ok(pairs),
            _ => Err(fail::fail("malformed-record", MALFORMED, "expected object")),
        }
    }

    pub fn as_array(&self) -> Result<&[Val], Fail> {
        match self {
            Val::Array(items) => Ok(items),
            _ => Err(fail::fail("malformed-record", MALFORMED, "expected array")),
        }
    }

    pub fn as_str(&self) -> Result<&str, Fail> {
        match self {
            Val::String(text) => Ok(text),
            _ => Err(fail::fail("malformed-record", MALFORMED, "expected string")),
        }
    }

    pub fn as_bool(&self) -> Result<bool, Fail> {
        match self {
            Val::Bool(value) => Ok(*value),
            _ => Err(fail::fail("malformed-record", MALFORMED, "expected bool")),
        }
    }

    pub fn as_int(&self) -> Result<i64, Fail> {
        match self {
            Val::Int(value) => Ok(*value),
            Val::Bool(_) | Val::Frac => Err(fail::fail("age-floor", fail::AGE_FLOOR, "age is not an integer")),
            _ => Err(fail::fail("malformed-record", MALFORMED, "expected integer")),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Val::Null)
    }
}

pub fn parse(input: &str) -> Result<Val, Fail> {
    let mut parser = Parser { input, index: 0 };
    let value = parser.value()?;
    parser.skip();
    if parser.index != input.len() {
        return Err(fail::fail("malformed-record", MALFORMED, "trailing json"));
    }
    Ok(value)
}

struct Parser<'a> {
    input: &'a str,
    index: usize,
}

impl<'a> Parser<'a> {
    fn skip(&mut self) {
        while let Some(byte) = self.input.as_bytes().get(self.index) {
            if byte.is_ascii_whitespace() {
                self.index += 1;
            } else {
                break;
            }
        }
    }

    fn value(&mut self) -> Result<Val, Fail> {
        self.skip();
        let byte = self
            .input
            .as_bytes()
            .get(self.index)
            .copied()
            .ok_or_else(|| fail::fail("malformed-record", MALFORMED, "truncated json"))?;
        match byte {
            b'n' => self.literal("null", Val::Null),
            b't' => self.literal("true", Val::Bool(true)),
            b'f' => self.literal("false", Val::Bool(false)),
            b'"' => Ok(Val::String(self.string()?)),
            b'[' => self.array(),
            b'{' => self.object(),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(fail::fail("malformed-record", MALFORMED, "unsupported json value")),
        }
    }

    fn literal(&mut self, text: &str, value: Val) -> Result<Val, Fail> {
        if self.input[self.index..].starts_with(text) {
            self.index += text.len();
            Ok(value)
        } else {
            Err(fail::fail("malformed-record", MALFORMED, "bad literal"))
        }
    }

    fn number(&mut self) -> Result<Val, Fail> {
        let start = self.index;
        if self.input.as_bytes().get(self.index) == Some(&b'-') {
            self.index += 1;
        }
        let digits = self.index;
        if self.input.as_bytes().get(self.index) == Some(&b'0') {
            self.index += 1;
        } else {
            while matches!(self.input.as_bytes().get(self.index), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        }
        if self.index == digits {
            return Err(fail::fail("malformed-record", MALFORMED, "bad number"));
        }
        let mut fractional = false;
        if self.input.as_bytes().get(self.index) == Some(&b'.') {
            fractional = true;
            self.index += 1;
            let mark = self.index;
            while matches!(self.input.as_bytes().get(self.index), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == mark {
                return Err(fail::fail("malformed-record", MALFORMED, "bad fraction"));
            }
        }
        if matches!(self.input.as_bytes().get(self.index), Some(b'e' | b'E')) {
            fractional = true;
            self.index += 1;
            if matches!(self.input.as_bytes().get(self.index), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let mark = self.index;
            while matches!(self.input.as_bytes().get(self.index), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == mark {
                return Err(fail::fail("malformed-record", MALFORMED, "bad exponent"));
            }
        }
        if fractional {
            return Ok(Val::Frac);
        }
        let text = &self.input[start..self.index];
        let parsed = text
            .parse::<i64>()
            .map_err(|_| fail::fail("malformed-record", MALFORMED, "integer overflow"))?;
        Ok(Val::Int(parsed))
    }

    fn string(&mut self) -> Result<String, Fail> {
        if self.input.as_bytes().get(self.index) != Some(&b'"') {
            return Err(fail::fail("malformed-record", MALFORMED, "expected string"));
        }
        self.index += 1;
        let mut out = String::new();
        while let Some(byte) = self.input.as_bytes().get(self.index).copied() {
            if byte == b'"' {
                self.index += 1;
                return Ok(out);
            }
            if byte == b'\\' {
                self.index += 1;
                let escape = self
                    .input
                    .as_bytes()
                    .get(self.index)
                    .copied()
                    .ok_or_else(|| fail::fail("malformed-record", MALFORMED, "truncated escape"))?;
                self.index += 1;
                match escape {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex = self.input.get(self.index..self.index + 4).ok_or_else(|| {
                            fail::fail("malformed-record", MALFORMED, "truncated unicode escape")
                        })?;
                        self.index += 4;
                        let code = u32::from_str_radix(hex, 16).map_err(|_| {
                            fail::fail("malformed-record", MALFORMED, "bad unicode escape")
                        })?;
                        let ch = char::from_u32(code).ok_or_else(|| {
                            fail::fail("malformed-record", MALFORMED, "bad unicode scalar")
                        })?;
                        out.push(ch);
                    }
                    _ => return Err(fail::fail("malformed-record", MALFORMED, "bad escape")),
                }
            } else if byte < 0x20 {
                return Err(fail::fail("malformed-record", MALFORMED, "raw control in string"));
            } else {
                let ch = self.input[self.index..].chars().next().ok_or_else(|| {
                    fail::fail("malformed-record", MALFORMED, "bad utf-8")
                })?;
                out.push(ch);
                self.index += ch.len_utf8();
            }
        }
        Err(fail::fail("malformed-record", MALFORMED, "unterminated string"))
    }

    fn array(&mut self) -> Result<Val, Fail> {
        self.index += 1;
        let mut items = Vec::new();
        loop {
            self.skip();
            if self.input.as_bytes().get(self.index) == Some(&b']') {
                self.index += 1;
                return Ok(Val::Array(items));
            }
            if !items.is_empty() {
                if self.input.as_bytes().get(self.index) != Some(&b',') {
                    return Err(fail::fail("malformed-record", MALFORMED, "expected comma"));
                }
                self.index += 1;
            }
            items.push(self.value()?);
            if items.len() > 10_000 {
                return Err(fail::fail("bound", fail::BOUND, "array too large"));
            }
        }
    }

    fn object(&mut self) -> Result<Val, Fail> {
        self.index += 1;
        let mut pairs = Vec::new();
        loop {
            self.skip();
            if self.input.as_bytes().get(self.index) == Some(&b'}') {
                self.index += 1;
                return Ok(Val::Object(pairs));
            }
            if !pairs.is_empty() {
                if self.input.as_bytes().get(self.index) != Some(&b',') {
                    return Err(fail::fail("malformed-record", MALFORMED, "expected comma"));
                }
                self.index += 1;
                self.skip();
            }
            if self.input.as_bytes().get(self.index) != Some(&b'"') {
                return Err(fail::fail("malformed-record", MALFORMED, "expected key"));
            }
            let key = self.string()?;
            if pairs.iter().any(|(name, _)| name == &key) {
                return Err(fail::fail("duplicate", fail::DUPLICATE, "duplicate json key"));
            }
            self.skip();
            if self.input.as_bytes().get(self.index) != Some(&b':') {
                return Err(fail::fail("malformed-record", MALFORMED, "expected colon"));
            }
            self.index += 1;
            pairs.push((key, self.value()?));
            if pairs.len() > 4_000 {
                return Err(fail::fail("bound", fail::BOUND, "object too large"));
            }
        }
    }
}
