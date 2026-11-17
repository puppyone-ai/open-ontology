//! `oo.docs.llm-extract/v1` (10 §5.2.3): strict parsing of the model's reply.
//!
//! A reply that is not one JSON object of exactly this shape is
//! `schema_invalid` as a whole batch; the caller retries once.

use serde_json::Value;

/// Confidence word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub const fn bp(self) -> u16 {
        match self {
            Self::High => 8500,
            Self::Medium => 7000,
            Self::Low => 5000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlmEntity {
    pub id: String,
    pub ty: String,
    pub label: Option<String>,
    pub name: String,
    pub aliases: Vec<String>,
    pub block_id: String,
    pub quote: String,
    pub confidence: Confidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlmRelation {
    pub subject: String,
    pub predicate: String,
    pub object: Option<String>,
    pub value: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub block_id: String,
    pub quote: String,
    pub confidence: Confidence,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LlmOutput {
    pub entities: Vec<LlmEntity>,
    pub relations: Vec<LlmRelation>,
}

const TYPES: &[&str] = &[
    "Person",
    "Organization",
    "Project",
    "Decision",
    "Document",
    "Topic",
    "Event",
    "Term",
    "Thing",
];
const PREDICATES: &[&str] = &[
    "owns",
    "member_of",
    "decides",
    "supersedes",
    "rejects",
    "mentions",
    "about",
    "tried_and_failed",
];

fn only_keys(o: &serde_json::Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    match o.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("unknown field {k}")),
        None => Ok(()),
    }
}

fn string<'a>(
    o: &'a serde_json::Map<String, Value>,
    k: &str,
    min: usize,
    max: usize,
) -> Result<&'a str, String> {
    let s = o
        .get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{k} must be a string"))?;
    let n = s.chars().count();
    if n < min || n > max {
        return Err(format!("{k} length {n} outside {min}..={max}"));
    }
    Ok(s)
}

fn opt_string(
    o: &serde_json::Map<String, Value>,
    k: &str,
    max: usize,
) -> Result<Option<String>, String> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.chars().count() <= max => Ok(Some(s.clone())),
        _ => Err(format!("{k} must be a string of at most {max} characters")),
    }
}

fn date(o: &serde_json::Map<String, Value>, k: &str) -> Result<Option<String>, String> {
    match opt_string(o, k, 10)? {
        None => Ok(None),
        Some(s) => {
            let b = s.as_bytes();
            let ok = b.len() == 10
                && b.iter().enumerate().all(|(i, c)| {
                    if i == 4 || i == 7 {
                        *c == b'-'
                    } else {
                        c.is_ascii_digit()
                    }
                });
            if ok {
                Ok(Some(s))
            } else {
                Err(format!("{k} must be YYYY-MM-DD"))
            }
        }
    }
}

fn confidence(o: &serde_json::Map<String, Value>) -> Result<Confidence, String> {
    match o.get("confidence").and_then(Value::as_str) {
        Some("high") => Ok(Confidence::High),
        Some("medium") => Ok(Confidence::Medium),
        Some("low") => Ok(Confidence::Low),
        _ => Err("confidence must be high, medium or low".into()),
    }
}

fn entity_id(s: &str) -> bool {
    s.strip_prefix('e')
        .is_some_and(|d| (1..=3).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()))
}

/// Parse and validate the model's text.
pub fn parse(text: &str) -> Result<LlmOutput, String> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| format!("not JSON: {e}"))?;
    let root = v.as_object().ok_or("the reply is not an object")?;
    only_keys(root, &["entities", "relations"])?;
    let ents = root
        .get("entities")
        .and_then(Value::as_array)
        .ok_or("entities is required")?;
    let rels = root
        .get("relations")
        .and_then(Value::as_array)
        .ok_or("relations is required")?;
    if ents.len() > 200 || rels.len() > 300 {
        return Err("too many items".into());
    }
    let mut out = LlmOutput::default();
    for e in ents {
        let o = e.as_object().ok_or("entity is not an object")?;
        only_keys(
            o,
            &[
                "id",
                "type",
                "label",
                "name",
                "aliases",
                "block_id",
                "quote",
                "confidence",
            ],
        )?;
        let id = string(o, "id", 1, 4)?;
        if !entity_id(id) {
            return Err("entity id must match ^e[0-9]{1,3}$".into());
        }
        let ty = string(o, "type", 1, 20)?;
        if !TYPES.contains(&ty) {
            return Err(format!("unknown type {ty}"));
        }
        let aliases: Vec<String> = match o.get("aliases") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) if a.len() <= 10 => a
                .iter()
                .map(|x| {
                    x.as_str()
                        .filter(|s| s.chars().count() <= 100)
                        .map(str::to_owned)
                        .ok_or_else(|| {
                            "alias must be a string of at most 100 characters".to_owned()
                        })
                })
                .collect::<Result<_, _>>()?,
            _ => return Err("aliases must be an array of at most 10".into()),
        };
        out.entities.push(LlmEntity {
            id: id.to_owned(),
            ty: ty.to_owned(),
            label: opt_string(o, "label", 40)?,
            name: string(o, "name", 1, 200)?.to_owned(),
            aliases,
            block_id: string(o, "block_id", 1, 64)?.to_owned(),
            quote: string(o, "quote", 0, 240)?.to_owned(),
            confidence: confidence(o)?,
        });
    }
    for r in rels {
        let o = r.as_object().ok_or("relation is not an object")?;
        only_keys(
            o,
            &[
                "subject",
                "predicate",
                "object",
                "value",
                "valid_from",
                "valid_to",
                "block_id",
                "quote",
                "confidence",
            ],
        )?;
        let predicate = string(o, "predicate", 1, 20)?;
        if !PREDICATES.contains(&predicate) {
            return Err(format!("unknown predicate {predicate}"));
        }
        out.relations.push(LlmRelation {
            subject: string(o, "subject", 1, 8)?.to_owned(),
            predicate: predicate.to_owned(),
            object: opt_string(o, "object", 8)?,
            value: opt_string(o, "value", 500)?,
            valid_from: date(o, "valid_from")?,
            valid_to: date(o, "valid_to")?,
            block_id: string(o, "block_id", 1, 64)?.to_owned(),
            quote: string(o, "quote", 0, 240)?.to_owned(),
            confidence: confidence(o)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"entities":[
      {"id":"e1","type":"Person","name":"陈子墨","aliases":["小陈"],"block_id":"blk_00000000000000000001","quote":"陈子墨","confidence":"high"},
      {"id":"e2","type":"Project","name":"星图","block_id":"blk_00000000000000000001","quote":"星图","confidence":"medium"}],
     "relations":[{"subject":"e1","predicate":"owns","object":"e2","valid_from":"2026-01-01",
      "block_id":"blk_00000000000000000001","quote":"陈子墨负责星图","confidence":"high"}]}"#;

    #[test]
    fn a_good_reply_parses() {
        let o = parse(GOOD).unwrap();
        assert_eq!((o.entities.len(), o.relations.len()), (2, 1));
        assert_eq!(o.entities[0].aliases, ["小陈"]);
        assert_eq!(o.relations[0].valid_from.as_deref(), Some("2026-01-01"));
        assert_eq!(o.entities[1].confidence.bp(), 7000);
        assert!(parse("  {\"entities\":[],\"relations\":[]}\n").is_ok());
    }

    #[test]
    fn every_rule_of_the_schema_is_enforced() {
        for bad in [
            "not json",
            "[]",
            r#"{"entities":[]}"#,
            r#"{"entities":[],"relations":[],"extra":1}"#,
            r#"{"entities":[{"id":"x1","type":"Person","name":"a","block_id":"b","quote":"","confidence":"high"}],"relations":[]}"#,
            r#"{"entities":[{"id":"e1","type":"Robot","name":"a","block_id":"b","quote":"","confidence":"high"}],"relations":[]}"#,
            r#"{"entities":[{"id":"e1","type":"Person","name":"","block_id":"b","quote":"","confidence":"high"}],"relations":[]}"#,
            r#"{"entities":[{"id":"e1","type":"Person","name":"a","block_id":"b","quote":"","confidence":"sure"}],"relations":[]}"#,
            r#"{"entities":[{"id":"e1","type":"Person","name":"a","block_id":"b","quote":"","confidence":"high","x":1}],"relations":[]}"#,
            r#"{"entities":[],"relations":[{"subject":"e1","predicate":"likes","block_id":"b","quote":"","confidence":"high"}]}"#,
            r#"{"entities":[],"relations":[{"subject":"e1","predicate":"owns","valid_from":"2026-1-1","block_id":"b","quote":"","confidence":"high"}]}"#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        let long = "字".repeat(241);
        let q = format!(
            r#"{{"entities":[{{"id":"e1","type":"Person","name":"a","block_id":"b","quote":"{long}","confidence":"high"}}],"relations":[]}}"#
        );
        assert!(parse(&q).is_err(), "quote over 240");
    }
}
