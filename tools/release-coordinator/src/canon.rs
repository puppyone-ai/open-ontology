//! Key-sorted JSON with no extra whitespace.

use serde_json::{Map, Number, Value};

use crate::sha256::sha256_hex;

pub fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

pub fn canonical_sha(value: &Value) -> String {
    sha256_hex(canonical(value).as_bytes())
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(bit) => out.push_str(if *bit { "true" } else { "false" }),
        Value::Number(number) => write_number(out, number),
        Value::String(text) => {
            out.push_str(&serde_json::to_string(text).expect("string"));
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    out.push('{');
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&serde_json::to_string(key).expect("key"));
        out.push(':');
        write_value(out, &map[*key]);
    }
    out.push('}');
}

fn write_number(out: &mut String, number: &Number) {
    if let Some(value) = number.as_i64() {
        out.push_str(&value.to_string());
    } else if let Some(value) = number.as_u64() {
        out.push_str(&value.to_string());
    } else {
        out.push_str(&serde_json::to_string(number).expect("number"));
    }
}

pub fn object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

pub fn reject_waiver(map: &Map<String, Value>) -> bool {
    map.contains_key("force")
        || map.contains_key("waive")
        || map.contains_key("all_accepted")
        || map.contains_key("allaccepted")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{canonical, reject_waiver};

    #[test]
    fn object_keys_are_sorted() {
        assert_eq!(canonical(&json!({"b": 1, "a": "x"})), r#"{"a":"x","b":1}"#);
    }

    #[test]
    fn waiver_keys_are_recognized() {
        let value = json!({"force": true});
        assert!(reject_waiver(value.as_object().expect("object")));
        let clean = json!({"gate_id": "G1"});
        assert!(!reject_waiver(clean.as_object().expect("object")));
    }
}
