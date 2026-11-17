//! Structured log lines (10 §10.1). Board WS-BE5.
//!
//! One JSON object per line on stderr. Fields are values the caller chose;
//! nothing is read from request bodies, and `message` text from sources is
//! never formatted in (error `Display` impls are static text, 4.3.1).

use serde_json::{Map, Value, json};

/// Build a log line. `level` is `info`, `warn` or `error`.
pub fn line(level: &str, event: &str, fields: Value) -> String {
    let mut obj = Map::new();
    obj.insert("level".into(), json!(level));
    obj.insert("event".into(), json!(event));
    if let Value::Object(f) = fields {
        for (k, v) in f {
            // The envelope keys win over caller fields.
            obj.entry(k).or_insert(v);
        }
    }
    Value::Object(obj).to_string()
}

/// Write a log line to stderr.
pub fn log(level: &str, event: &str, fields: Value) {
    eprintln!("{}", line(level, event, fields));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_keys_win() {
        let l = line(
            "info",
            "engine.started",
            json!({"level": "x", "port": 8700}),
        );
        let v: Value = serde_json::from_str(&l).unwrap();
        assert_eq!(v["level"], "info");
        assert_eq!(v["event"], "engine.started");
        assert_eq!(v["port"], 8700);
    }
}
