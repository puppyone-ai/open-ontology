//! Structured log lines (10 §10.1). Board WS-BE5.
//!
//! One JSON object per line on stderr. Fields are values the caller chose;
//! nothing is read from request bodies, and `message` text from sources is
//! never formatted in (error `Display` impls are static text, 4.3.1).

pub mod disk;
pub mod latency;

use serde_json::{Map, Value, json};

/// Build a log line. `level` is `info`, `warn` or `error`.
pub fn line(level: &str, event: &str, fields: Value) -> String {
    let mut obj = Map::new();
    obj.insert("ts".into(), json!(now_rfc3339()));
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

/// RFC 3339 now without a time crate: seconds precision from the system clock.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Short stable hash of a path for log lines (`path_h`): the path itself is
/// never logged (10 §10.1).
pub fn path_h(path: &str) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(path.as_bytes());
    d.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Free text from a library error (start up failures) made safe for a log
/// line: every word that is a URL (`scheme://...`, credentials and all), an
/// absolute path or a `host:port` pair becomes `<redacted>`, and the text is
/// cut at 200 characters.
pub fn scrub(text: &str) -> String {
    let out: Vec<&str> = text
        .split_whitespace()
        .map(|w| {
            let w_trim =
                w.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')' | ',' | ';' | '`'));
            let url = w_trim.contains("://");
            let path =
                w_trim.starts_with('/') || w_trim.starts_with("~/") || w_trim.contains(":\\");
            let host_port = w_trim.rsplit_once(':').is_some_and(|(h, p)| {
                !h.is_empty()
                    && !p.is_empty()
                    && p.bytes().all(|b| b.is_ascii_digit())
                    && h.contains('.')
            });
            if url || path || host_port {
                "<redacted>"
            } else {
                w
            }
        })
        .collect();
    out.join(" ").chars().take(200).collect()
}

/// `OO_DOCS_LOG=debug` turns debug lines on.
pub fn debug_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OO_DOCS_LOG").is_ok_and(|v| v == "debug"))
}

/// The fields of a log line (10 §10.1), and nothing else: there is no place to
/// put a path, a name, an endpoint or a bucket, so a line built from `Fields`
/// cannot leak one.
#[derive(Clone, Debug, Default)]
pub struct Fields<'a> {
    /// Request id.
    pub request_id: Option<&'a str>,
    /// Job id.
    pub job_id: Option<&'a str>,
    /// Job kind.
    pub kind: Option<&'a str>,
    /// Drive id.
    pub drive_id: Option<&'a str>,
    /// File id.
    pub file_id: Option<&'a str>,
    /// Hash of the path, from [`path_h`].
    pub path_h: Option<&'a str>,
    /// Stage name.
    pub stage: Option<&'a str>,
    /// Attempt number.
    pub attempt: Option<i32>,
    /// Duration.
    pub duration_ms: Option<i64>,
    /// A `DOCS_*` code.
    pub code: Option<&'a str>,
    /// A route template (`GET /v1/docs/files/{id}`) or HTTP status text.
    pub route: Option<&'a str>,
    /// HTTP status.
    pub status: Option<u16>,
    /// A count (`facts_added`, ontology version n, ...), named by `count_name`.
    pub count: Option<(&'static str, i64)>,
}

impl Fields<'_> {
    /// The JSON object of the set fields.
    pub fn json(&self) -> Value {
        let mut m = Map::new();
        let strs = [
            ("request_id", self.request_id),
            ("job_id", self.job_id),
            ("kind", self.kind),
            ("drive_id", self.drive_id),
            ("file_id", self.file_id),
            ("path_h", self.path_h),
            ("stage", self.stage),
            ("code", self.code),
            ("route", self.route),
        ];
        for (k, v) in strs {
            if let Some(v) = v {
                m.insert(k.into(), json!(v));
            }
        }
        if let Some(a) = self.attempt {
            m.insert("attempt".into(), json!(a));
        }
        if let Some(d) = self.duration_ms {
            m.insert("duration_ms".into(), json!(d));
        }
        if let Some(s) = self.status {
            m.insert("status".into(), json!(s));
        }
        if let Some((k, n)) = self.count {
            m.insert(k.into(), json!(n));
        }
        Value::Object(m)
    }
}

/// Log a line of the fixed vocabulary.
pub fn emit(level: &str, event: &str, f: &Fields<'_>) {
    log(level, event, f.json());
}

/// [`emit`] only when `OO_DOCS_LOG=debug`.
pub fn emit_debug(event: &str, f: &Fields<'_>) {
    if debug_enabled() {
        log("debug", event, f.json());
    }
}

/// Route template of a request path: ids and hashes become `{id}`.
pub fn route_template(method: &str, path: &str) -> String {
    let segs: Vec<String> = path
        .split('/')
        .map(|s| {
            let looks_id = s.len() >= 20 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
            if looks_id {
                "{id}".to_owned()
            } else if s.contains(':') && s.len() > 1 {
                // `files:retry`, `drives/<id>:sync`
                match s.split_once(':') {
                    Some((head, tail)) if head.len() >= 20 => format!("{{id}}:{tail}"),
                    _ => s.to_owned(),
                }
            } else {
                s.to_owned()
            }
        })
        .collect();
    // Under `/files/...` and `/context-packs/<id>/files/...` the rest is a
    // user path: keep the template, drop what follows.
    let joined = segs.join("/");
    let cut = joined
        .find("/files/")
        .filter(|_| joined.contains("context-packs"))
        .map(|i| format!("{}/files/{{path}}", &joined[..i]))
        .unwrap_or(joined);
    format!("{method} {cut}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_carry_a_timestamp_and_only_the_fixed_fields() {
        let f = Fields {
            job_id: Some("j"),
            kind: Some("parse"),
            path_h: Some(&path_h("a/b.md")),
            attempt: Some(2),
            duration_ms: Some(40),
            code: Some("DOCS_PARSE_TIMEOUT"),
            ..Fields::default()
        };
        let v: Value = serde_json::from_str(&line("info", "job.retry", f.json())).unwrap();
        assert!(v["ts"].as_str().is_some_and(|t| t.ends_with('Z')));
        assert_eq!(
            (v["event"].as_str(), v["kind"].as_str()),
            (Some("job.retry"), Some("parse"))
        );
        assert_eq!(v["path_h"].as_str().map(str::len), Some(12));
        assert!(!v.to_string().contains("b.md"));
        // Unset fields are absent, not null.
        assert!(v.get("file_id").is_none());
    }

    #[test]
    fn route_templates_hide_ids_and_user_paths() {
        let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
        assert_eq!(
            route_template("GET", &format!("/v1/docs/files/{id}/blocks")),
            "GET /v1/docs/files/{id}/blocks"
        );
        assert_eq!(
            route_template("POST", &format!("/v1/docs/drives/{id}:sync")),
            "POST /v1/docs/drives/{id}:sync"
        );
        assert_eq!(
            route_template("POST", "/v1/docs/files:retry"),
            "POST /v1/docs/files:retry"
        );
        assert_eq!(
            route_template(
                "GET",
                &format!("/v1/docs/context-packs/{id}/files/People/林川.md")
            ),
            "GET /v1/docs/context-packs/{id}/files/{path}"
        );
    }

    #[test]
    fn scrub_removes_urls_paths_and_hosts() {
        let t = scrub(
            "error connecting to postgres://user:pw@db.internal:5432/x: refused at /Users/a/b (127.0.0.1:5432) ok",
        );
        assert!(
            !t.contains("pw") && !t.contains("/Users") && !t.contains("db.internal"),
            "{t}"
        );
        assert!(!t.contains("127.0.0.1"), "{t}");
        assert!(t.contains("refused") && t.contains("ok"));
        assert!(scrub(&"x ".repeat(500)).chars().count() <= 200);
    }

    #[test]
    fn path_hash_is_stable_and_short() {
        assert_eq!(path_h("a.md"), path_h("a.md"));
        assert_ne!(path_h("a.md"), path_h("b.md"));
        assert_eq!(path_h("x").len(), 12);
    }

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
