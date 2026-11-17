//! `fm.type` and `fm.fields`: frontmatter to entities, facts and aliases
//! (10 §5.1). Pure.

use std::collections::BTreeMap;

use serde_json::Value;

use super::kv::{Field, field};

/// Frontmatter values as lists of strings, keyed by field. Keys the rules do
/// not know are dropped; when two keys map to the same field (`owner` and
/// `负责人`) the lexicographically first key wins, so the result is
/// deterministic.
pub fn fields(frontmatter: &BTreeMap<String, Value>) -> Vec<(Field, Vec<String>)> {
    let mut out: BTreeMap<u8, (Field, Vec<String>)> = BTreeMap::new();
    for (key, value) in frontmatter {
        let Some(f) = field(key) else { continue };
        let values = values(value);
        if values.is_empty() {
            continue;
        }
        out.entry(order(f)).or_insert((f, values));
    }
    out.into_values().collect()
}

fn order(f: Field) -> u8 {
    match f {
        Field::Type => 0,
        Field::Date => 1,
        Field::Owner => 2,
        Field::DecidedBy => 3,
        Field::Members => 4,
        Field::Project => 5,
        Field::Status => 6,
        Field::Supersedes => 7,
        Field::Rejected => 8,
        Field::Aliases => 9,
        Field::Tags => 10,
    }
}

fn values(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.trim().to_owned()],
        Value::Number(n) => vec![n.to_string()],
        Value::Bool(b) => vec![b.to_string()],
        Value::Array(items) => items.iter().flat_map(values).collect(),
        Value::Null | Value::Object(_) => Vec::new(),
    }
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn maps_known_keys_in_fixed_order() {
        let mut fm = BTreeMap::new();
        fm.insert("status".to_owned(), json!("active"));
        fm.insert("负责人".to_owned(), json!("张三"));
        fm.insert("owner".to_owned(), json!("李四"));
        fm.insert("members".to_owned(), json!(["甲", "乙"]));
        fm.insert("unknown".to_owned(), json!("x"));
        fm.insert("tags".to_owned(), json!([]));
        let got = fields(&fm);
        let kinds: Vec<Field> = got.iter().map(|(f, _)| *f).collect();
        assert_eq!(kinds, [Field::Owner, Field::Members, Field::Status]);
        // "owner" < "负责人" in byte order.
        assert_eq!(got[0].1, ["李四"]);
        assert_eq!(got[1].1, ["甲", "乙"]);
    }
}
