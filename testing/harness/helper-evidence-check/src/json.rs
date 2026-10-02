//! Bounded JSON subset used by the helper evidence checker.
//!
//! Container depth counts objects and arrays only. [`parse`] starts at depth 0,
//! before it reads a value.
//!
//! - A scalar root (`null`, boolean, number, or string) stays at depth 0.
//! - An object or array root is depth 1. Each container directly inside another
//!   container adds one. A scalar inside a container does not add a level.
//! - [`MAX_DEPTH`] is 64. The next depth is checked before that container's
//!   function is called, so a 65th container does not recurse.
//! - Depth 64 is the deepest accepted value. Depth 65, and any greater depth
//!   including 20000, returns [`Fail`] code `bound` (exit 22). Parsing does not
//!   panic, abort, or raise a signal.
//!
//! Arrays still stop at 10_000 elements and objects at 4_000 keys. Those limits
//! are separate from depth.

use crate::fail::{self, Fail, BOUND, MALFORMED};

/// Deepest accepted container nesting, counting the root container as 1.
pub const MAX_DEPTH: usize = 64;

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
    let value = parser.value(0)?;
    parser.skip();
    if parser.index != input.len() {
        return Err(fail::fail("malformed-record", MALFORMED, "trailing json"));
    }
    Ok(value)
}

fn next_depth(depth: usize) -> Result<usize, Fail> {
    match depth.checked_add(1) {
        Some(next) if next <= MAX_DEPTH => Ok(next),
        _ => Err(fail::fail("bound", BOUND, "json depth exceeds 64")),
    }
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

    fn value(&mut self, depth: usize) -> Result<Val, Fail> {
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
            b'[' => {
                let nested = next_depth(depth)?;
                self.array(nested)
            }
            b'{' => {
                let nested = next_depth(depth)?;
                self.object(nested)
            }
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

    fn array(&mut self, depth: usize) -> Result<Val, Fail> {
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
            items.push(self.value(depth)?);
            if items.len() > 10_000 {
                return Err(fail::fail("bound", fail::BOUND, "array too large"));
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Val, Fail> {
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
            pairs.push((key, self.value(depth)?));
            if pairs.len() > 4_000 {
                return Err(fail::fail("bound", fail::BOUND, "object too large"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn nested(depth: usize, kind: &str) -> String {
        let mut text = String::new();
        for index in 0..depth {
            match kind {
                "object" => text.push_str("{\"a\":"),
                "array" => text.push('['),
                "mixed" => {
                    if index % 2 == 0 {
                        text.push_str("{\"a\":");
                    } else {
                        text.push('[');
                    }
                }
                _ => panic!("kind"),
            }
        }
        text.push('0');
        for index in (0..depth).rev() {
            let close_object = match kind {
                "object" => true,
                "array" => false,
                "mixed" => index % 2 == 0,
                _ => panic!("kind"),
            };
            text.push(if close_object { '}' } else { ']' });
        }
        text
    }

    fn assert_bound(text: &str) {
        let err = parse(text).expect_err(text);
        assert_eq!(err.code, "bound", "{}", err.detail);
        assert_eq!(err.exit_code, 22, "{}", err.detail);
    }

    #[test]
    fn scalar_root_stays_at_depth_zero() {
        for text in ["0", "\"x\"", "true", "false", "null"] {
            assert!(parse(text).is_ok(), "{text}");
        }
        assert!(parse("{}").is_ok());
        assert!(parse("[]").is_ok());
        assert!(parse("{\"a\":0}").is_ok());
    }

    #[test]
    fn depth_64_accepts_and_65_is_bound() {
        for kind in ["object", "array", "mixed"] {
            assert!(parse(&nested(64, kind)).is_ok(), "{kind} 64");
            assert_bound(&nested(65, kind));
        }
    }

    #[test]
    fn malformed_escape_and_duplicate_key_fail_closed() {
        let escape = parse("\"\\q\"").expect_err("escape");
        assert_eq!(escape.code, "malformed-record");
        assert_eq!(escape.exit_code, 21);
        let truncated = parse("\"\\u12\"").expect_err("truncated");
        assert_eq!(truncated.code, "malformed-record");
        let surrogate = parse("\"\\uD800\"").expect_err("surrogate");
        assert_eq!(surrogate.code, "malformed-record");
        let duplicate = parse("{\"a\":1,\"a\":2}").expect_err("duplicate");
        assert_eq!(duplicate.code, "duplicate");
        assert_eq!(duplicate.exit_code, 14);
        let unicode = parse("{\"a\":1,\"\\u0061\":2}").expect_err("unicode duplicate");
        assert_eq!(unicode.code, "duplicate");
    }
}
