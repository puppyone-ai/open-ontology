use crate::error::{Result, err};

#[derive(Clone, Debug)]
pub enum J {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn str(value: impl Into<String>) -> Self {
        Self::Str(value.into())
    }

    pub fn obj(pairs: Vec<(String, J)>) -> Self {
        Self::Obj(pairs)
    }

    pub fn render(&self) -> Result<String> {
        let mut out = String::new();
        self.write(&mut out)?;
        out.push('\n');
        Ok(out)
    }

    fn write(&self, out: &mut String) -> Result<()> {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(true) => out.push_str("true"),
            Self::Bool(false) => out.push_str("false"),
            Self::Int(value) => out.push_str(&value.to_string()),
            Self::UInt(value) => out.push_str(&value.to_string()),
            Self::Str(value) => push_string(out, value),
            Self::Arr(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out)?;
                }
                out.push(']');
            }
            Self::Obj(pairs) => {
                let mut ordered = pairs.clone();
                ordered.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
                for window in ordered.windows(2) {
                    if window[0].0 == window[1].0 {
                        return Err(err(format!("duplicate json key {}", window[0].0)));
                    }
                }
                out.push('{');
                for (index, (key, value)) in ordered.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    push_string(out, key);
                    out.push(':');
                    value.write(out)?;
                }
                out.push('}');
            }
        }
        Ok(())
    }
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn pair(key: &str, value: J) -> (String, J) {
    (key.to_string(), value)
}

pub fn s(value: impl Into<String>) -> J {
    J::Str(value.into())
}

pub fn u(value: u64) -> J {
    J::UInt(value)
}

pub fn b(value: bool) -> J {
    J::Bool(value)
}

pub fn arr(items: Vec<J>) -> J {
    J::Arr(items)
}

pub fn obj(pairs: Vec<(&str, J)>) -> J {
    J::obj(
        pairs
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::J;
    use oo_core::canonical::parse_json;

    #[test]
    fn sorts_keys_and_escapes() {
        let value = J::obj(vec![
            ("b".into(), J::str("a\"b\\c")),
            ("a".into(), J::UInt(10)),
        ]);
        let text = value.render().expect("json");
        assert_eq!(text, "{\"a\":10,\"b\":\"a\\\"b\\\\c\"}\n");
        assert!(parse_json(text.trim_end()).is_ok());
    }
}
