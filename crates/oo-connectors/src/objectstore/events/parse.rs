//! Event bodies of the clouds, as their documentation describes them, turned
//! into [`ChangeHint`]s. A body that does not parse yields no hints and an
//! error: an event is only ever an acceleration (D10), so refusing one is safe.
//!
//! * S3 (SQS or SNS body): `{"Records":[{"eventName":"ObjectCreated:Put","eventTime":..,
//!   "s3":{"object":{"key":<url encoded>,"versionId":..,"sequencer":..}}}]}`
//! * R2 (Queues message): `{"action":"PutObject","eventTime":..,"object":{"key":..}}`
//! * OSS (MNS body or EventBridge CloudEvent): `{"events":[{"eventName":"ObjectCreated:PutObject",
//!   "eventTime":..,"oss":{"object":{"key":..}}}]}` or `{"type":"oss:ObjectCreated:PutObject",
//!   "time":..,"data":{"oss":{"object":{"key":..}}}}`
//! * COS (event notification delivered to a queue or a function, as Tencent documents it):
//!   `{"Records":[{"event":{"eventName":"cos:ObjectCreated:Put","eventTime":<unix s>},"cos":{"cosObject":
//!   {"key":"/<appid>/<bucket>/<key>","vid":..},"cosBucket":{..}}}]}`. Built from the public documentation
//!   and synthetic fixtures; no real bucket has confirmed it (10 §4.3.7), the key is taken as plain text.
//! * neutral (local event source, tests): `{"hints":[{"key","op":"put|delete","version_id",
//!   "sequencer","at_ms"}]}`

use serde_json::Value;

use crate::objectstore::{ChangeHint, ChangeOp};

/// Why a body gave nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    NotJson,
    UnknownShape,
    BadKey,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ParseError::NotJson => "event body is not json",
            ParseError::UnknownShape => "event body has no known shape",
            ParseError::BadKey => "event object key is not usable",
        })
    }
}

impl std::error::Error for ParseError {}

/// Percent decoding as S3 events use it: `%XX` and `+` for a space.
pub fn url_decode_key(text: &str) -> Option<Vec<u8>> {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Some(out)
}

/// `2026-11-17T08:49:37.123Z` to Unix milliseconds. Only `Z` is accepted.
pub fn rfc3339_ms(text: &str) -> Option<i64> {
    let t = text.trim().strip_suffix('Z')?;
    let (date, time) = t.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let (hms, frac) = time.split_once('.').unwrap_or((time, "0"));
    let mut p = hms.split(':');
    let (h, mi, s): (i64, i64, i64) = (
        p.next()?.parse().ok()?,
        p.next()?.parse().ok()?,
        p.next()?.parse().ok()?,
    );
    let mut ms: i64 = frac.chars().take(3).collect::<String>().parse().ok()?;
    for _ in frac.chars().take(3).count()..3 {
        ms *= 10;
    }
    let yy = if m <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((((days * 24 + h) * 60 + mi) * 60 + s) * 1000 + ms)
}

fn op_of(name: &str) -> Option<ChangeOp> {
    let n = name.to_ascii_lowercase();
    if n.contains("removed") || n.contains("delete") || n.contains("lifecycleexpiration") {
        Some(ChangeOp::Delete)
    } else if n.contains("created")
        || n.starts_with("put")
        || n.contains("copy")
        || n.contains("multipartupload")
        || n.contains("restore")
    {
        Some(ChangeOp::Put)
    } else {
        None
    }
}

fn text<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = v;
    for p in path {
        cur = cur.get(p)?;
    }
    cur.as_str()
}

/// `/1253970026/examplebkt/dir/a.md` to `dir/a.md`.
fn cos_object_key(full: &str) -> Option<&str> {
    let mut it = full.trim_start_matches('/').splitn(3, '/');
    let (appid, bucket, key) = (it.next()?, it.next()?, it.next()?);
    (!appid.is_empty() && !bucket.is_empty() && !key.is_empty()).then_some(key)
}

/// COS gives the event time in Unix seconds, as a number or as text.
fn cos_time_ms(v: Option<&Value>) -> Option<i64> {
    let secs = match v? {
        Value::Number(n) => n.as_i64()?,
        Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    secs.checked_mul(1000)
}

fn key_in(prefix: &str, raw: &str, decode: bool) -> Result<Vec<u8>, ParseError> {
    let bytes = if decode {
        url_decode_key(raw).ok_or(ParseError::BadKey)?
    } else {
        raw.as_bytes().to_vec()
    };
    // The Drive sees keys below its prefix only.
    let rest = bytes
        .strip_prefix(prefix.as_bytes())
        .ok_or(ParseError::BadKey)?;
    if rest.is_empty() || rest.contains(&0) {
        return Err(ParseError::BadKey);
    }
    Ok(rest.to_vec())
}

/// Parse any supported body. `prefix` is the Drive's key prefix (empty or
/// ending in `/`); events of other keys are dropped, not errors. `now_ms` is
/// used when an event carries no time.
pub fn parse_hints(body: &str, prefix: &str, now_ms: i64) -> Result<Vec<ChangeHint>, ParseError> {
    let v: Value = serde_json::from_str(body).map_err(|_| ParseError::NotJson)?;
    let mut out = Vec::new();
    let mut push = |key: Result<Vec<u8>, ParseError>,
                    op: Option<ChangeOp>,
                    version: Option<&str>,
                    seq: Option<&str>,
                    at: Option<i64>| {
        // Another prefix, or an event kind that is no change, gives nothing.
        if let (Ok(raw_key), Some(op)) = (key, op) {
            out.push(ChangeHint {
                raw_key,
                op,
                version_id: version
                    .filter(|s| !s.is_empty() && *s != "null")
                    .map(str::to_owned),
                sequencer: seq.filter(|s| !s.is_empty()).map(str::to_owned),
                observed_at_ms: at.unwrap_or(now_ms),
            });
        }
    };
    let mut known = false;
    if let Some(records) = v.get("Records").and_then(Value::as_array) {
        known = true;
        for r in records {
            if r.get("cos").is_some() {
                // COS: the key reads `/<appid>/<bucket>/<object key>`.
                let key = text(r, &["cos", "cosObject", "key"])
                    .and_then(cos_object_key)
                    .map(|k| key_in(prefix, k, false));
                if let Some(k) = key {
                    push(
                        k,
                        text(r, &["event", "eventName"]).and_then(op_of),
                        text(r, &["cos", "cosObject", "vid"]),
                        None,
                        cos_time_ms(r.pointer("/event/eventTime")),
                    );
                }
                continue;
            }
            let k = text(r, &["s3", "object", "key"]).map(|k| key_in(prefix, k, true));
            if let Some(k) = k {
                push(
                    k,
                    text(r, &["eventName"]).and_then(op_of),
                    text(r, &["s3", "object", "versionId"]),
                    text(r, &["s3", "object", "sequencer"]),
                    text(r, &["eventTime"]).and_then(rfc3339_ms),
                );
            }
        }
    }
    if let (Some(action), Some(key)) = (text(&v, &["action"]), text(&v, &["object", "key"])) {
        known = true; // R2
        push(
            key_in(prefix, key, false),
            op_of(action),
            None,
            None,
            text(&v, &["eventTime"]).and_then(rfc3339_ms),
        );
    }
    if let Some(events) = v.get("events").and_then(Value::as_array) {
        known = true; // OSS MNS
        for e in events {
            if let Some(key) = text(e, &["oss", "object", "key"]) {
                push(
                    key_in(prefix, key, false),
                    text(e, &["eventName"]).and_then(op_of),
                    text(e, &["oss", "object", "versionId"]),
                    text(e, &["oss", "object", "sequencer"]),
                    text(e, &["eventTime"]).and_then(rfc3339_ms),
                );
            }
        }
    }
    if let (Some(ty), Some(key)) = (
        text(&v, &["type"]),
        text(&v, &["data", "oss", "object", "key"]),
    ) {
        known = true; // OSS EventBridge CloudEvent
        push(
            key_in(prefix, key, false),
            op_of(ty.trim_start_matches("oss:")),
            text(&v, &["data", "oss", "object", "versionId"]),
            None,
            text(&v, &["time"]).and_then(rfc3339_ms),
        );
    }
    if let Some(hints) = v.get("hints").and_then(Value::as_array) {
        known = true; // neutral
        for h in hints {
            let op = match text(h, &["op"]) {
                Some("put") => Some(ChangeOp::Put),
                Some("delete") => Some(ChangeOp::Delete),
                _ => None,
            };
            if let Some(key) = text(h, &["key"]) {
                push(
                    key_in(prefix, key, false),
                    op,
                    text(h, &["version_id"]),
                    text(h, &["sequencer"]),
                    h.get("at_ms").and_then(Value::as_i64),
                );
            }
        }
    }
    if known {
        Ok(out)
    } else {
        Err(ParseError::UnknownShape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s3_records_are_url_decoded_ordered_by_sequencer_and_cut_at_the_prefix() {
        let body = r#"{"Records":[
          {"eventName":"ObjectCreated:Put","eventTime":"2026-11-17T08:49:37.123Z",
           "s3":{"object":{"key":"drive%2Fa+b%E4%B8%AD.md","versionId":"v1","sequencer":"0055AED6DCD90281E5"}}},
          {"eventName":"ObjectRemoved:Delete","eventTime":"2026-11-17T08:49:38Z",
           "s3":{"object":{"key":"drive/gone.md","sequencer":"0055AED6DCD90281E6"}}},
          {"eventName":"ObjectCreated:Put","s3":{"object":{"key":"other/x.md"}}},
          {"eventName":"s3:TestEvent","s3":{"object":{"key":"drive/t.md"}}}]}"#;
        let h = parse_hints(body, "drive/", 7).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].raw_key, "a b中.md".as_bytes());
        assert_eq!(
            (h[0].op, h[0].version_id.as_deref()),
            (ChangeOp::Put, Some("v1"))
        );
        assert_eq!(
            h[0].observed_at_ms,
            rfc3339_ms("2026-11-17T08:49:37.123Z").unwrap()
        );
        assert_eq!(h[1].op, ChangeOp::Delete);
    }

    #[test]
    fn r2_and_oss_bodies() {
        let r2 = r#"{"account":"a","action":"DeleteObject","bucket":"b","object":{"key":"d/x.md","size":1},"eventTime":"2026-11-17T00:00:00.5Z"}"#;
        let h = parse_hints(r2, "d/", 0).unwrap();
        assert_eq!(
            (h[0].op, h[0].raw_key.clone(), h[0].observed_at_ms % 1000),
            (ChangeOp::Delete, b"x.md".to_vec(), 500)
        );
        let mns = r#"{"events":[{"eventName":"ObjectCreated:PutObject","eventTime":"2026-11-17T00:00:00.000Z","oss":{"object":{"key":"d/y.md"}}}]}"#;
        assert_eq!(parse_hints(mns, "d/", 0).unwrap()[0].raw_key, b"y.md");
        let ce = r#"{"type":"oss:ObjectRemoved:DeleteObject","time":"2026-11-17T00:00:01Z","data":{"oss":{"object":{"key":"d/z.md"}}}}"#;
        assert_eq!(parse_hints(ce, "d/", 0).unwrap()[0].op, ChangeOp::Delete);
    }

    #[test]
    fn neutral_body_and_refusals() {
        let n = r#"{"hints":[{"key":"a.md","op":"put","sequencer":"2","at_ms":5},{"key":"b.md","op":"weird"}]}"#;
        let h = parse_hints(n, "", 0).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(
            (h[0].sequencer.as_deref(), h[0].observed_at_ms),
            (Some("2"), 5)
        );
        assert_eq!(parse_hints("nope", "", 0), Err(ParseError::NotJson));
        assert_eq!(parse_hints("{}", "", 0), Err(ParseError::UnknownShape));
        // A key that climbs out cannot be produced: only the part below the prefix survives and
        // `stat` refuses unsafe keys again.
        assert!(url_decode_key("%zz").is_none());
    }

    #[test]
    fn cos_bodies_from_the_fixtures() {
        let put = include_str!("fixtures/cos-put.json");
        let h = parse_hints(put, "drive/", 0).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].raw_key, b"Projects/plan.md");
        assert_eq!(
            (h[0].op, h[0].version_id.clone(), h[0].observed_at_ms),
            (ChangeOp::Put, None, 1_545_205_770_000),
            "an empty vid is no version"
        );
        let del = include_str!("fixtures/cos-delete-versioned.json");
        let h = parse_hints(del, "drive/", 0).unwrap();
        assert_eq!(h[0].raw_key, "Notes/速记.md".as_bytes());
        assert_eq!(
            (h[0].op, h[0].version_id.as_deref(), h[0].observed_at_ms),
            (ChangeOp::Delete, Some("MTg0NDUxNzA2Mzc"), 1_545_205_800_000),
            "a delete marker is a delete; the time may come as text"
        );
        // Several records: other prefix, a folder placeholder key and a restore event.
        let merged = include_str!("fixtures/cos-merged.json");
        let h = parse_hints(merged, "drive/", 0).unwrap();
        let got: Vec<(&[u8], ChangeOp)> = h.iter().map(|x| (x.raw_key.as_slice(), x.op)).collect();
        assert_eq!(
            got,
            [
                (b"big.pdf".as_slice(), ChangeOp::Put),
                (b"restored.md".as_slice(), ChangeOp::Put)
            ]
        );
        // Without the drive prefix the same records name other keys.
        assert_eq!(parse_hints(merged, "", 0).unwrap().len(), 3);
        // Not a COS key: dropped, not an error (an event only ever accelerates).
        let odd = r#"{"Records":[{"event":{"eventName":"cos:ObjectCreated:Put"},"cos":{"cosObject":{"key":"nokey"}}}]}"#;
        assert_eq!(parse_hints(odd, "", 0).unwrap().len(), 0);
    }

    #[test]
    fn rfc3339() {
        assert_eq!(rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_ms("1994-11-06T08:49:37Z"), Some(784_111_777_000));
        assert_eq!(
            rfc3339_ms("2026-11-17T08:49:37.12Z").map(|v| v % 1000),
            Some(120)
        );
        assert_eq!(rfc3339_ms("2026-11-17 08:49:37"), None);
    }
}
