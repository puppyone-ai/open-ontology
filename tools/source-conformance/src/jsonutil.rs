use std::cell::RefCell;
use std::fmt;

use serde::de::{DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

pub fn require_object<'a>(value: &'a Value, where_: &str) -> Result<&'a Map<String, Value>, String> {
    value.as_object().ok_or_else(|| format!("{where_}: expected an object"))
}

pub fn only_keys(map: &Map<String, Value>, allowed: &[&str], where_: &str) -> Result<(), String> {
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{where_}: unknown field {key}"));
        }
    }
    Ok(())
}

pub fn require_str(map: &Map<String, Value>, key: &str, where_: &str) -> Result<String, String> {
    let value = map.get(key).ok_or_else(|| format!("{where_}.{key}: missing"))?;
    let text = value.as_str().ok_or_else(|| format!("{where_}.{key}: expected a string"))?;
    if text.is_empty() || text.len() > 160 || text.chars().any(|ch| ch.is_control()) {
        return Err(format!("{where_}.{key}: invalid text"));
    }
    Ok(text.to_owned())
}

pub fn require_string_array(map: &Map<String, Value>, key: &str, where_: &str) -> Result<Vec<String>, String> {
    let value = map.get(key).ok_or_else(|| format!("{where_}.{key}: missing"))?;
    let items = value.as_array().ok_or_else(|| format!("{where_}.{key}: expected a list"))?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let text = item.as_str().ok_or_else(|| format!("{where_}.{key}: expected strings"))?;
        if text.is_empty() || text.len() > 80 {
            return Err(format!("{where_}.{key}: invalid entry"));
        }
        out.push(text.to_owned());
    }
    Ok(out)
}

pub fn require_bool(map: &Map<String, Value>, key: &str, where_: &str) -> Result<bool, String> {
    let value = map.get(key).ok_or_else(|| format!("{where_}.{key}: missing"))?;
    value.as_bool().ok_or_else(|| format!("{where_}.{key}: expected a boolean"))
}

pub fn require_exit(map: &Map<String, Value>, key: &str, where_: &str) -> Result<u16, String> {
    let value = map.get(key).ok_or_else(|| format!("{where_}.{key}: missing"))?;
    let number = value.as_u64().ok_or_else(|| format!("{where_}.{key}: expected an integer"))?;
    if number > 255 {
        return Err(format!("{where_}.{key}: exit out of range"));
    }
    Ok(number as u16)
}

pub fn hex64(text: &str) -> bool {
    text.len() == 64 && text.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}

/// JSON value plus the first duplicated identity or version key.
/// Keys are compared after serde decodes string escapes, at every object.
pub struct JsonDocument {
    pub value: Value,
    pub duplicate_key: Option<String>,
}

pub fn parse_json_document(bytes: &[u8]) -> Result<JsonDocument, &'static str> {
    let state = State { duplicate: RefCell::new(None) };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = Seed { state: &state }.deserialize(&mut deserializer).map_err(|_| "json")?;
    deserializer.end().map_err(|_| "json")?;
    Ok(JsonDocument {
        value,
        duplicate_key: state.duplicate.into_inner(),
    })
}

fn identity_version_key(key: &str) -> bool {
    matches!(key, "exact_version" | "tested_versions" | "service_version" | "identity")
}

struct State {
    duplicate: RefCell<Option<String>>,
}

struct Seed<'a> {
    state: &'a State,
}

impl<'de, 'a> DeserializeSeed<'de> for Seed<'a> {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonVisitor { state: self.state })
    }
}

struct JsonVisitor<'a> {
    state: &'a State,
}

impl<'de, 'a> Visitor<'de> for JsonVisitor<'a> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value).map(Value::Number).ok_or_else(|| E::custom("number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut items = Vec::with_capacity(access.size_hint().unwrap_or(0));
        while let Some(item) = access.next_element_seed(Seed { state: self.state })? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut map = Map::new();
        while let Some(key) = access.next_key::<String>()? {
            if map.contains_key(&key) && identity_version_key(&key) {
                let mut found = self.state.duplicate.borrow_mut();
                if found.is_none() {
                    *found = Some(key.clone());
                }
            }
            let value = access.next_value_seed(Seed { state: self.state })?;
            map.insert(key, value);
        }
        Ok(Value::Object(map))
    }
}

/// Top-level string field from a JSON object, including a document serde rejects.
/// Stops at the first syntax error after earlier fields have been read.
pub fn top_level_string(bytes: &[u8], want: &str) -> Option<String> {
    let mut scan = Scan { bytes, index: 0 };
    scan.skip_ws();
    scan.eat(b'{')?;
    loop {
        scan.skip_ws();
        if scan.peek() != Some(b'"') {
            return None;
        }
        let key = scan.string()?;
        scan.skip_ws();
        scan.eat(b':')?;
        scan.skip_ws();
        if key == want && scan.peek() == Some(b'"') {
            return scan.string();
        }
        scan.skip_value()?;
        scan.skip_ws();
        scan.eat(b',')?;
    }
}

struct Scan<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Scan<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.index += 1;
        Some(byte)
    }

    fn eat(&mut self, want: u8) -> Option<()> {
        if self.peek() == Some(want) {
            self.index += 1;
            Some(())
        } else {
            None
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }

    fn string(&mut self) -> Option<String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self.bump()?;
            match byte {
                b'"' => return Some(out),
                b'\\' => out.push(self.escape()?),
                0x00..=0x1f => return None,
                byte => out.push_str(self.utf8(byte)?),
            }
        }
    }

    fn escape(&mut self) -> Option<char> {
        match self.bump()? {
            b'"' => Some('"'),
            b'\\' => Some('\\'),
            b'/' => Some('/'),
            b'b' => Some('\u{0008}'),
            b'f' => Some('\u{000c}'),
            b'n' => Some('\n'),
            b'r' => Some('\r'),
            b't' => Some('\t'),
            b'u' => {
                let mut hex = [0u8; 4];
                for slot in &mut hex {
                    *slot = self.bump()?;
                }
                let text = std::str::from_utf8(&hex).ok()?;
                char::from_u32(u32::from_str_radix(text, 16).ok()?)
            }
            _ => None,
        }
    }

    fn utf8(&mut self, first: u8) -> Option<&'a str> {
        let width = match first {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => return None,
        };
        let start = self.index - 1;
        for _ in 1..width {
            let next = self.bump()?;
            if next & 0xc0 != 0x80 {
                return None;
            }
        }
        std::str::from_utf8(&self.bytes[start..self.index]).ok()
    }

    fn skip_value(&mut self) -> Option<()> {
        self.skip_ws();
        match self.peek()? {
            b'"' => {
                self.string()?;
            }
            b'{' => self.skip_container(b'{', b'}')?,
            b'[' => self.skip_container(b'[', b']')?,
            b't' => self.literal(b"true")?,
            b'f' => self.literal(b"false")?,
            b'n' => self.literal(b"null")?,
            b'-' | b'0'..=b'9' => self.skip_number()?,
            _ => return None,
        }
        Some(())
    }

    fn skip_container(&mut self, open: u8, close: u8) -> Option<()> {
        self.eat(open)?;
        loop {
            self.skip_ws();
            if self.eat(close).is_some() {
                return Some(());
            }
            if open == b'{' {
                self.string()?;
                self.skip_ws();
                self.eat(b':')?;
            }
            self.skip_value()?;
            self.skip_ws();
            if self.peek() == Some(b',') {
                self.bump();
            }
        }
    }

    fn literal(&mut self, text: &[u8]) -> Option<()> {
        for byte in text {
            if self.bump()? != *byte {
                return None;
            }
        }
        Some(())
    }

    fn skip_number(&mut self) -> Option<()> {
        if self.peek() == Some(b'-') {
            self.bump();
        }
        let start = self.index;
        while matches!(self.peek(), Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')) {
            self.bump();
        }
        if self.index == start {
            None
        } else {
            Some(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_json_document, top_level_string};
    use serde_json::Value;

    fn parsed(text: &str) -> (Value, Option<String>) {
        let document = parse_json_document(text.as_bytes()).unwrap();
        (document.value, document.duplicate_key)
    }

    #[test]
    fn unique_json_matches_serde_and_duplicate_identity_keys_do_not() {
        let text = r#"{"exact_version":"16.9","identity":{"service_version":"16.9"},"n":1}"#;
        let (value, duplicate) = parsed(text);
        assert_eq!(duplicate, None);
        assert_eq!(value, serde_json::from_str::<Value>(text).unwrap());

        let (last, duplicate) = parsed(r#"{"service_version":"15.2","service_version":"16.9"}"#);
        assert_eq!(duplicate.as_deref(), Some("service_version"));
        assert_eq!(last["service_version"], "16.9");

        let (first, duplicate) = parsed(r#"{"service_version":"16.9","service_version":"15.2"}"#);
        assert_eq!(duplicate.as_deref(), Some("service_version"));
        assert_eq!(first["service_version"], "15.2");

        let (same, duplicate) = parsed(r#"{"exact_version":"16.9","exact_version":"16.9"}"#);
        assert_eq!(duplicate.as_deref(), Some("exact_version"));
        assert_eq!(same["exact_version"], "16.9");

        let (escaped, duplicate) = parsed("{\"service_version\":\"15.2\",\"\\u0073ervice_version\":\"16.9\"}");
        assert_eq!(duplicate.as_deref(), Some("service_version"));
        assert_eq!(escaped["service_version"], "16.9");

        let (middle, duplicate) = parsed("{\"service_\\u0076ersion\":\"15.2\",\"service_version\":\"16.9\"}");
        assert_eq!(duplicate.as_deref(), Some("service_version"));
        assert_eq!(middle["service_version"], "16.9");

        let (nested, duplicate) = parsed(
            r#"{"exact_version":"16.9","identity":{"service_version":"15.2","service_version":"16.9"}}"#,
        );
        assert_eq!(duplicate.as_deref(), Some("service_version"));
        assert_eq!(nested["identity"]["service_version"], "16.9");

        let (object_key, duplicate) = parsed(r#"{"identity":{"service_version":"15.2"},"identity":{"service_version":"16.9"}}"#);
        assert_eq!(duplicate.as_deref(), Some("identity"));
        assert_eq!(object_key["identity"]["service_version"], "16.9");

        let (versions, duplicate) = parsed(r#"{"tested_versions":["16.9"],"tested_versions":["15.2"]}"#);
        assert_eq!(duplicate.as_deref(), Some("tested_versions"));
        assert_eq!(versions["tested_versions"][0], "15.2");

        let (buried, duplicate) = parsed(r#"{"cases":[{"service_version":"15.2","service_version":"16.9"}]}"#);
        assert_eq!(duplicate.as_deref(), Some("service_version"));
        assert_eq!(buried["cases"][0]["service_version"], "16.9");

        let (other, duplicate) = parsed(r#"{"note":"15.2","note":"16.9"}"#);
        assert_eq!(duplicate, None);
        assert_eq!(other["note"], "16.9");

        assert!(parse_json_document(br#"{"topology":"review",}"#).is_err());
        let broken = br#"{"schema":"oo","evidence_id":"rev-w37-missing-0001","identity":{"topology":"review",}}"#;
        assert!(parse_json_document(broken).is_err());
        assert_eq!(top_level_string(broken, "evidence_id").as_deref(), Some("rev-w37-missing-0001"));
    }
}
