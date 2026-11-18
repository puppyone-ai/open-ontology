//! The `oo.suggest/v1` record an Agent writes into `.context/suggest/` or sends
//! through the MCP `suggest` tool (10 §11.1). Pure: parsing and validation only.
//!
//! A file is JSON (one record or an array) or Markdown whose frontmatter holds
//! the record (`key: value` lines between `---` lines, the body is the
//! rationale when the frontmatter has none). Limits: 64 KiB per file, 50
//! records per file; the 200 files per share are the collector's.

use serde_json::{Map, Value, json};

pub const PROTOCOL: &str = "oo.suggest/v1";
pub const MAX_FILE_BYTES: usize = 64 * 1024;
pub const MAX_RECORDS_PER_FILE: usize = 50;
pub const MAX_RATIONALE_CHARS: usize = 500;
pub const KINDS: [&str; 5] = [
    "new_fact",
    "supersede",
    "correction",
    "preference",
    "type_change",
];

/// A record that passed validation.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub kind: String,
    pub subject: String,
    pub predicate: String,
    pub object: Option<String>,
    pub value: Option<String>,
    pub valid_from: Option<String>,
    pub supersedes: Option<String>,
    pub rationale: String,
    pub evidence: Vec<Evidence>,
    pub idempotency_key: Option<String>,
}

/// One evidence reference of a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    /// `brief_line`, `ref` or `external`.
    pub kind: String,
    pub reference: String,
}

impl Suggestion {
    /// The stored payload: the record as written, normalised.
    pub fn payload(&self) -> Value {
        let mut o = Map::new();
        o.insert("protocol".into(), json!(PROTOCOL));
        o.insert("kind".into(), json!(self.kind));
        o.insert("subject".into(), json!(self.subject));
        o.insert("predicate".into(), json!(self.predicate));
        for (k, v) in [
            ("object", &self.object),
            ("value", &self.value),
            ("valid_from", &self.valid_from),
            ("supersedes", &self.supersedes),
        ] {
            if let Some(v) = v {
                o.insert(k.into(), json!(v));
            }
        }
        o.insert("rationale".into(), json!(self.rationale));
        o.insert(
            "evidence".into(),
            Value::Array(
                self.evidence
                    .iter()
                    .map(|e| json!({"kind": e.kind, "ref": e.reference}))
                    .collect(),
            ),
        );
        Value::Object(o)
    }

    /// Key that makes a resubmission of the same record a no-op: the
    /// caller's `idempotency_key` or a hash of the payload.
    pub fn dedupe_key(&self) -> String {
        match &self.idempotency_key {
            Some(k) => k.clone(),
            None => blake3::hash(oo_builder::canonical_json(&self.payload()).as_bytes())
                .to_hex()
                .to_string(),
        }
    }
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && matches!(s[5..7].parse::<u32>(), Ok(1..=12))
        && matches!(s[8..10].parse::<u32>(), Ok(1..=31))
}

fn predicate_ok(p: &str) -> bool {
    matches!(
        p,
        "owns"
            | "member_of"
            | "decides"
            | "supersedes"
            | "rejects"
            | "mentions"
            | "about"
            | "tried_and_failed"
    ) || p.strip_prefix("attr.").is_some_and(|a| {
        (1..=40).contains(&a.len()) && a.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
    })
}

fn text<'a>(o: &'a Map<String, Value>, key: &str, max: usize) -> Result<Option<&'a str>, String> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.trim().is_empty() && s.chars().count() <= max => {
            Ok(Some(s.trim()))
        }
        Some(_) => Err(format!(
            "/{key}: expected a non-empty string of at most {max} characters"
        )),
    }
}

/// Validate one JSON record. The error is `"<json pointer>: <reason>"`.
pub fn validate(v: &Value) -> Result<Suggestion, String> {
    let o = v.as_object().ok_or("/: expected an object")?;
    if let Some(k) = o.keys().find(|k| {
        ![
            "protocol",
            "kind",
            "subject",
            "predicate",
            "object",
            "value",
            "valid_from",
            "supersedes",
            "rationale",
            "evidence",
            "idempotency_key",
            "share_id",
        ]
        .contains(&k.as_str())
    }) {
        return Err(format!("/{k}: unknown field"));
    }
    if let Some(p) = o.get("protocol")
        && p.as_str() != Some(PROTOCOL)
    {
        return Err(format!("/protocol: expected {PROTOCOL}"));
    }
    let kind = text(o, "kind", 32)?.ok_or("/kind: required")?;
    if !KINDS.contains(&kind) {
        return Err(format!("/kind: expected one of {}", KINDS.join(", ")));
    }
    let subject = text(o, "subject", 256)?.ok_or("/subject: required")?;
    let predicate = text(o, "predicate", 64)?.ok_or("/predicate: required")?;
    let object = text(o, "object", 256)?;
    let value = text(o, "value", 2000)?;
    let is_fact_kind = matches!(kind, "new_fact" | "supersede" | "correction");
    if is_fact_kind {
        if !predicate_ok(predicate) {
            return Err("/predicate: expected a core relation or attr.<name>".into());
        }
        if object.is_some() == value.is_some() {
            return Err("/object: give exactly one of object and value".into());
        }
    }
    let valid_from = text(o, "valid_from", 10)?;
    if valid_from.is_some_and(|d| !is_date(d)) {
        return Err("/valid_from: expected YYYY-MM-DD".into());
    }
    let supersedes = text(o, "supersedes", 512)?;
    if matches!(kind, "supersede" | "correction") && supersedes.is_none() {
        return Err("/supersedes: required for supersede and correction".into());
    }
    if kind == "supersede" && valid_from.is_none() {
        return Err("/valid_from: required for supersede".into());
    }
    let rationale = text(o, "rationale", MAX_RATIONALE_CHARS)?
        .ok_or("/rationale: required, at most 500 characters")?;
    let mut evidence = Vec::new();
    match o.get("evidence") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) if a.len() <= 20 => {
            for (i, e) in a.iter().enumerate() {
                let e = e
                    .as_object()
                    .ok_or(format!("/evidence/{i}: expected an object"))?;
                let k = e.get("kind").and_then(Value::as_str).unwrap_or_default();
                if !["brief_line", "ref", "external"].contains(&k) {
                    return Err(format!(
                        "/evidence/{i}/kind: expected brief_line, ref or external"
                    ));
                }
                let r = e
                    .get("ref")
                    .and_then(Value::as_str)
                    .filter(|r| !r.trim().is_empty() && r.len() <= 1024)
                    .ok_or(format!("/evidence/{i}/ref: required"))?;
                evidence.push(Evidence {
                    kind: k.into(),
                    reference: r.trim().into(),
                });
            }
        }
        Some(_) => return Err("/evidence: expected an array of at most 20".into()),
    }
    let idempotency_key = text(o, "idempotency_key", 128)?
        .map(|k| {
            if k.len() >= 8
                && k.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
            {
                Ok(k.to_owned())
            } else {
                Err("/idempotency_key: expected 8 to 128 characters of [A-Za-z0-9_.:-]".to_owned())
            }
        })
        .transpose()?;
    Ok(Suggestion {
        kind: kind.into(),
        subject: subject.into(),
        predicate: predicate.into(),
        object: object.map(Into::into),
        value: value.map(Into::into),
        valid_from: valid_from.map(Into::into),
        supersedes: supersedes.map(Into::into),
        rationale: rationale.into(),
        evidence,
        idempotency_key,
    })
}

/// Records of a JSON document (one object or an array of at most 50).
pub fn parse_json(text: &str) -> Result<Vec<Value>, String> {
    if text.len() > MAX_FILE_BYTES {
        return Err("/: file is larger than 64 KiB".into());
    }
    match serde_json::from_str::<Value>(text).map_err(|e| format!("/: not JSON ({e})"))? {
        Value::Array(a) if a.len() <= MAX_RECORDS_PER_FILE && !a.is_empty() => Ok(a),
        Value::Array(_) => Err("/: expected 1 to 50 records".into()),
        o @ Value::Object(_) => Ok(vec![o]),
        _ => Err("/: expected an object or an array".into()),
    }
}

/// Record of a Markdown file: frontmatter lines `key: value`; `evidence` lines
/// are `evidence: kind:ref` (repeatable); the body becomes the rationale when
/// the frontmatter has none.
pub fn parse_markdown(text: &str) -> Result<Vec<Value>, String> {
    if text.len() > MAX_FILE_BYTES {
        return Err("/: file is larger than 64 KiB".into());
    }
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err("/: a Markdown suggestion starts with a --- frontmatter".into());
    }
    let mut o = Map::new();
    let mut evidence = Vec::new();
    let mut closed = false;
    for l in lines.by_ref() {
        if l.trim() == "---" {
            closed = true;
            break;
        }
        let Some((k, v)) = l.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim().trim_matches('"'));
        if k == "evidence" {
            let (kind, r) = v.split_once(':').ok_or("/evidence: expected kind:ref")?;
            evidence.push(json!({"kind": kind.trim(), "ref": r.trim()}));
        } else if !k.is_empty() {
            o.insert(k.into(), json!(v));
        }
    }
    if !closed {
        return Err("/: the frontmatter is not closed".into());
    }
    if !o.contains_key("rationale") {
        let body: String = lines.collect::<Vec<_>>().join("\n");
        if !body.trim().is_empty() {
            o.insert("rationale".into(), json!(body.trim()));
        }
    }
    if !evidence.is_empty() {
        o.insert("evidence".into(), Value::Array(evidence));
    }
    Ok(vec![Value::Object(o)])
}

/// Records of a document by its format (`json` or `md`).
pub fn parse_document(format: &str, text: &str) -> Result<Vec<Value>, String> {
    match format {
        "json" => parse_json(text),
        "md" => parse_markdown(text),
        _ => Err("/format: expected json or md".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact() -> Value {
        json!({"protocol": PROTOCOL, "kind": "new_fact", "subject": "星火计划", "predicate": "attr.status",
               "value": "进行中", "rationale": "周会确认",
               "evidence": [{"kind": "brief_line", "ref": "[src:Projects/a/周会.md#L3-L3]"}]})
    }

    #[test]
    fn a_complete_record_validates_and_hashes_stably() {
        let s = validate(&fact()).unwrap();
        assert_eq!(s.kind, "new_fact");
        assert_eq!(s.evidence.len(), 1);
        assert_eq!(s.dedupe_key(), validate(&fact()).unwrap().dedupe_key());
        let mut other = fact();
        other["value"] = json!("已完成");
        assert_ne!(s.dedupe_key(), validate(&other).unwrap().dedupe_key());
    }

    #[test]
    fn rejects_with_a_pointer() {
        let mut f = fact();
        f["kind"] = json!("delete_everything");
        assert!(validate(&f).unwrap_err().starts_with("/kind"));
        let mut f = fact();
        f.as_object_mut().unwrap().remove("rationale");
        assert!(validate(&f).unwrap_err().starts_with("/rationale"));
        let mut f = fact();
        f["object"] = json!("某人");
        assert!(validate(&f).unwrap_err().starts_with("/object"));
        let mut f = fact();
        f["kind"] = json!("supersede");
        assert!(validate(&f).unwrap_err().starts_with("/supersedes"));
        let mut f = fact();
        f["rationale"] = json!("长".repeat(501));
        assert!(validate(&f).unwrap_err().starts_with("/rationale"));
        let mut f = fact();
        f["zzz"] = json!(1);
        assert!(validate(&f).unwrap_err().contains("unknown field"));
        let mut f = fact();
        f["valid_from"] = json!("2026-13-01");
        assert!(validate(&f).unwrap_err().starts_with("/valid_from"));
    }

    #[test]
    fn preference_and_type_change_need_no_fact_shape() {
        let p = json!({"kind": "preference", "subject": "团队", "predicate": "prefers", "value": "短句",
                       "rationale": "负责人说的"});
        assert!(validate(&p).is_ok());
    }

    #[test]
    fn files_have_limits_and_both_formats() {
        assert!(parse_json(&"x".repeat(MAX_FILE_BYTES + 1)).is_err());
        let many = Value::Array(vec![fact(); 51]).to_string();
        assert!(parse_json(&many).is_err());
        assert_eq!(
            parse_json(&Value::Array(vec![fact(); 50]).to_string())
                .unwrap()
                .len(),
            50
        );
        let md = "---\nkind: new_fact\nsubject: 星火计划\npredicate: attr.status\nvalue: 进行中\n\
                  evidence: ref:Projects/a/周会.md#L3-L3\n---\n周会上确认了状态。\n";
        let recs = parse_markdown(md).unwrap();
        let s = validate(&recs[0]).unwrap();
        assert_eq!(s.rationale, "周会上确认了状态。");
        assert_eq!(s.evidence[0].kind, "ref");
        assert!(parse_markdown("no frontmatter").is_err());
    }
}
