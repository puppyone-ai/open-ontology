//! `oo.adjudication/v1` (read), overrides and the one publish confirmation
//! (written). The confirmation binds the summary hash recomputed after the
//! overrides are applied, so an override can never ride on a stale confirm.
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use oo_core::api::payload_sha256;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const PROTOCOL: &str = "oo.adjudication/v1";
pub const OVERRIDE_PROTOCOL: &str = "oo.adjudication-override/v1";
pub const CONFIRM_PROTOCOL: &str = "oo.publish-confirm/v1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Adjudication {
    pub protocol: String,
    pub input_sha256: String,
    #[serde(default)]
    pub thresholds: Map<String, Value>,
    pub items: Vec<Item>,
    pub graph: Graph,
    pub summary_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub class: String,
    #[serde(default)]
    pub subject: Map<String, Value>,
    pub decision: String,
    #[serde(default)]
    pub evidence: Map<String, Value>,
    #[serde(default)]
    pub scores: Map<String, Value>,
    #[serde(default)]
    pub rule_id: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub entities: Vec<Entity>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub label: String,
    pub table: String,
    #[serde(default)]
    pub key: Vec<String>,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub rows: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub relation: String,
    pub from: String,
    pub to: String,
    pub cardinality: String,
    pub adopted: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Override {
    pub decision: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Overrides {
    pub protocol: String,
    pub summary_sha256: String,
    pub items: BTreeMap<String, Override>,
}

impl Overrides {
    pub fn new(adj: &Adjudication) -> Self {
        Overrides {
            protocol: OVERRIDE_PROTOCOL.into(),
            summary_sha256: adj.summary_sha256.clone(),
            items: BTreeMap::new(),
        }
    }
}

/// The two decisions a class may take; the first is the "keep" side.
pub fn decisions(class: &str) -> Option<(&'static str, &'static str)> {
    Some(match class {
        "relation" | "entity_key" => ("adopted", "rejected"),
        "sensitive" => ("hidden", "visible"),
        "metric" => ("certified", "demoted"),
        "code_table" => ("complete", "incomplete"),
        _ => return None,
    })
}

/// The other decision of the pair.
pub fn flip(class: &str, decision: &str) -> Option<&'static str> {
    let (a, b) = decisions(class)?;
    if decision == a {
        Some(b)
    } else if decision == b {
        Some(a)
    } else {
        None
    }
}

/// Canonical summary: sorted-key compact JSON of `{"decisions":{id:decision},
/// "graph":graph}`, SHA-256 hex. Lane G must hash the same preimage.
pub fn summary_sha256(adj: &Adjudication) -> String {
    let decisions: BTreeMap<&str, &str> = adj
        .items
        .iter()
        .map(|i| (i.id.as_str(), i.decision.as_str()))
        .collect();
    let canonical = json!({"decisions": decisions, "graph": adj.graph});
    payload_sha256(canonical.to_string().as_bytes())
}

fn strip(hash: &str) -> &str {
    hash.strip_prefix("sha256:").unwrap_or(hash)
}

/// Validate shape and the declared summary hash.
pub fn parse(value: Value) -> Result<Adjudication, &'static str> {
    let adj: Adjudication = serde_json::from_value(value).map_err(|_| "ADJUDICATION_SHAPE")?;
    if adj.protocol != PROTOCOL {
        return Err("ADJUDICATION_PROTOCOL");
    }
    let mut ids = std::collections::BTreeSet::new();
    if adj.items.iter().any(|i| !ids.insert(i.id.as_str())) {
        return Err("ADJUDICATION_DUPLICATE_ITEM");
    }
    let entities: std::collections::BTreeSet<&str> =
        adj.graph.entities.iter().map(|e| e.id.as_str()).collect();
    if entities.len() != adj.graph.entities.len()
        || adj
            .graph
            .edges
            .iter()
            .any(|e| !entities.contains(e.from.as_str()) || !entities.contains(e.to.as_str()))
    {
        return Err("ADJUDICATION_GRAPH");
    }
    if strip(&adj.summary_sha256) != summary_sha256(&adj) {
        return Err("ADJUDICATION_SUMMARY_MISMATCH");
    }
    Ok(adj)
}

/// Apply overrides and return the effective adjudication (summary recomputed).
pub fn apply(adj: &Adjudication, overrides: &Overrides) -> Result<Adjudication, &'static str> {
    if overrides.protocol != OVERRIDE_PROTOCOL
        || strip(&overrides.summary_sha256) != strip(&adj.summary_sha256)
    {
        return Err("OVERRIDE_BINDING");
    }
    let mut out = adj.clone();
    for (id, change) in &overrides.items {
        let item = out
            .items
            .iter_mut()
            .find(|i| &i.id == id)
            .ok_or("OVERRIDE_UNKNOWN_ITEM")?;
        let (a, b) = decisions(&item.class).ok_or("OVERRIDE_CLASS")?;
        if change.decision != a && change.decision != b {
            return Err("OVERRIDE_DECISION");
        }
        // Exposing a column the product hid is the irreversible direction.
        if item.class == "sensitive"
            && change.decision == "visible"
            && change.reason.trim().is_empty()
        {
            return Err("OVERRIDE_REASON_REQUIRED");
        }
        item.decision.clone_from(&change.decision);
        if item.class == "relation" {
            for edge in out.graph.edges.iter_mut().filter(|e| &e.relation == id) {
                edge.adopted = change.decision == "adopted";
            }
        }
    }
    out.summary_sha256 = summary_sha256(&out);
    Ok(out)
}

pub fn overrides_sha256(overrides: &Overrides) -> String {
    payload_sha256(json!(overrides).to_string().as_bytes())
}

/// Build `oo.publish-confirm/v1`. `given` must equal the effective summary.
pub fn confirm(
    adj: &Adjudication,
    overrides: Option<&Overrides>,
    given: &str,
    reason: &str,
    confirmed_at: &str,
) -> Result<Value, &'static str> {
    let effective = match overrides {
        Some(o) => apply(adj, o)?,
        None => adj.clone(),
    };
    let summary = summary_sha256(&effective);
    if strip(given) != summary {
        return Err("CONFIRM_SUMMARY_MISMATCH");
    }
    let reason = reason.trim();
    if reason.is_empty() || reason.chars().count() > 500 || reason.chars().any(char::is_control) {
        return Err("CONFIRM_REASON_REQUIRED");
    }
    Ok(json!({
        "protocol": CONFIRM_PROTOCOL,
        "summary_sha256": summary,
        "overrides_sha256": overrides.filter(|o| !o.items.is_empty()).map(overrides_sha256),
        "reason": reason,
        "confirmed_at": confirmed_at,
    }))
}

/// Counts for the confirm dialog and `--json`.
pub fn counts(adj: &Adjudication) -> Value {
    let n = |class: &str, decision: &str| {
        adj.items
            .iter()
            .filter(|i| i.class == class && i.decision == decision)
            .count()
    };
    json!({
        "entities": adj.graph.entities.len(),
        "relations": adj.graph.edges.iter().filter(|e| e.adopted).count(),
        "rejected_relations": adj.graph.edges.iter().filter(|e| !e.adopted).count(),
        "hidden_columns": n("sensitive", "hidden"),
        "demoted_metrics": n("metric", "demoted"),
        "incomplete_code_tables": n("code_table", "incomplete"),
    })
}

/// `0.997`, `120k`, `true` → short text.
pub fn number(value: &Value) -> String {
    if let Some(n) = value.as_u64() {
        return magnitude(n);
    }
    if let Some(f) = value.as_f64() {
        let s = format!("{f:.3}");
        return s.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

/// Row magnitude: `12`, `1.2k`, `45k`, `1.2M`.
pub fn magnitude(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..10_000 => format!("{:.1}k", n as f64 / 1e3).replace(".0k", "k"),
        10_000..1_000_000 => format!("{}k", n / 1_000),
        1_000_000..10_000_000 => format!("{:.1}M", n as f64 / 1e6).replace(".0M", "M"),
        10_000_000..1_000_000_000 => format!("{}M", n / 1_000_000),
        _ => format!("{:.1}B", n as f64 / 1e9),
    }
}

/// One-line evidence summary, e.g. `containment 0.997 · parent unique · 120k rows`.
pub fn evidence_line(item: &Item) -> String {
    let mut parts = Vec::new();
    for (key, value) in &item.evidence {
        let name = match key.as_str() {
            "join_cooccurrence" => "joins",
            "name_similarity" => "name",
            "format_hit_rate" => "format hits",
            "distinct_ratio" => "distinct",
            "non_null_ratio" => "non-null",
            other => other,
        }
        .replace('_', " ");
        let text = match (key.as_str(), value) {
            ("parent_unique", Value::Bool(true)) => "parent unique".to_owned(),
            ("parent_unique", Value::Bool(false)) => "parent not unique".to_owned(),
            ("rows_checked", v) => format!("{} rows", number(v)),
            ("cardinality", Value::String(s)) => s.replace('_', "-"),
            (_, Value::Bool(true)) => name,
            (_, Value::Bool(false)) => format!("{name} ✗"),
            (_, v) => format!("{name} {}", number(v)),
        };
        parts.push(text);
    }
    parts.join(" · ")
}

/// What a rejected relation lacked, e.g. `containment 0.71 < 0.9`.
pub fn missing_evidence(item: &Item, thresholds: &Map<String, Value>) -> String {
    let get = |k: &str| item.evidence.get(k).and_then(Value::as_f64);
    let strict = thresholds
        .get("relation_containment_strict")
        .and_then(Value::as_f64)
        .unwrap_or(0.98);
    let signal = thresholds
        .get("relation_containment_with_signal")
        .and_then(Value::as_f64)
        .unwrap_or(0.9);
    let mut missing = Vec::new();
    if let Some(c) = get("containment") {
        let has_signal = get("name_similarity").is_some_and(|s| s >= 0.5)
            || get("join_cooccurrence").is_some_and(|j| j > 0.0);
        let need = if has_signal { signal } else { strict };
        if c < need {
            missing.push(format!(
                "containment {} < {}",
                number(&json!(c)),
                number(&json!(need))
            ));
        }
    }
    if item.evidence.get("parent_unique") == Some(&Value::Bool(false)) {
        missing.push("parent not unique".into());
    }
    if missing.is_empty() {
        item.reason.clone()
    } else {
        missing.join(", ")
    }
}

/// UTC RFC 3339 seconds, from the system clock.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339(secs)
}

pub fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

const MAX_BYTES: u64 = 16 * 1024 * 1024;

pub fn read_json(path: &Path) -> Result<Value, &'static str> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "ADJUDICATION_FILE_IO")?;
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES)
    {
        return Err("ADJUDICATION_FILE_IO");
    }
    let mut text = String::new();
    file.take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "ADJUDICATION_FILE_IO")?;
    serde_json::from_str(&text).map_err(|_| "ADJUDICATION_JSON")
}

pub fn load(path: &Path) -> Result<Adjudication, &'static str> {
    parse(read_json(path)?)
}

/// New private file (0600); never overwrites or follows a link.
pub fn write_new(path: &Path, value: &Value) -> Result<(), &'static str> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|_| "CONFIRM_FILE_IO")?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "CONFIRM_FILE_EXISTS_OR_IO")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "CONFIRM_FILE_IO")
}

#[cfg(test)]
pub(crate) fn fixture() -> Adjudication {
    parse(serde_json::from_str(include_str!("../../tests/fixtures/adjudication.json")).unwrap())
        .unwrap()
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_adjudication_hash_vector() {
        let vector: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../execution/v0.3/contracts/hash-vector.json"
        ))
        .unwrap();
        let document = super::parse(vector["document"].clone()).unwrap();
        assert_eq!(
            super::summary_sha256(&document),
            vector["expected_sha256"].as_str().unwrap()
        );
    }

    #[test]
    fn curated_demo_uses_the_same_confirmation_contract() {
        let document = super::parse(
            serde_json::from_str(include_str!(
                "../../../../deploy/lite/demo/adjudication.json"
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(document.graph.entities.len(), 10);
        assert_eq!(document.graph.edges.len(), 10);
        assert_eq!(super::summary_sha256(&document), document.summary_sha256);
    }

    use super::*;

    #[test]
    fn fixture_summary_hash_is_canonical_and_tamper_evident() {
        let adj = fixture();
        assert_eq!(adj.summary_sha256, summary_sha256(&adj));
        let mut tampered = json!(adj);
        tampered["items"][0]["decision"] = json!("rejected");
        assert_eq!(
            parse(tampered).unwrap_err(),
            "ADJUDICATION_SUMMARY_MISMATCH"
        );
    }

    #[test]
    fn confirm_binds_the_summary_after_overrides() {
        let adj = fixture();
        let at = "2026-11-09T00:00:00Z";
        let plain = confirm(&adj, None, &adj.summary_sha256, "looks right", at).unwrap();
        assert_eq!(plain["summary_sha256"], json!(adj.summary_sha256));
        assert_eq!(plain["overrides_sha256"], Value::Null);
        assert_eq!(
            confirm(&adj, None, "0".repeat(64).as_str(), "x", at).unwrap_err(),
            "CONFIRM_SUMMARY_MISMATCH"
        );
        assert_eq!(
            confirm(&adj, None, &adj.summary_sha256, "  ", at).unwrap_err(),
            "CONFIRM_REASON_REQUIRED"
        );

        let rejected = adj
            .items
            .iter()
            .find(|i| i.class == "relation" && i.decision == "rejected")
            .unwrap();
        let mut o = Overrides::new(&adj);
        o.items.insert(
            rejected.id.clone(),
            Override {
                decision: "adopted".into(),
                reason: "known FK".into(),
            },
        );
        let effective = apply(&adj, &o).unwrap();
        assert_ne!(effective.summary_sha256, adj.summary_sha256);
        assert!(
            effective
                .graph
                .edges
                .iter()
                .filter(|e| e.relation == rejected.id)
                .all(|e| e.adopted)
        );
        // The stale (pre-override) hash no longer confirms.
        assert_eq!(
            confirm(&adj, Some(&o), &adj.summary_sha256, "ok", at).unwrap_err(),
            "CONFIRM_SUMMARY_MISMATCH"
        );
        let doc = confirm(&adj, Some(&o), &effective.summary_sha256, "ok", at).unwrap();
        assert_eq!(doc["overrides_sha256"], json!(overrides_sha256(&o)));
        assert_eq!(doc["protocol"], CONFIRM_PROTOCOL);
    }

    #[test]
    fn overrides_are_bound_validated_and_unhiding_needs_a_reason() {
        let adj = fixture();
        let hidden = adj
            .items
            .iter()
            .find(|i| i.class == "sensitive" && i.decision == "hidden")
            .unwrap();
        let mut o = Overrides::new(&adj);
        o.items.insert(
            hidden.id.clone(),
            Override {
                decision: "visible".into(),
                reason: String::new(),
            },
        );
        assert_eq!(apply(&adj, &o).unwrap_err(), "OVERRIDE_REASON_REQUIRED");
        o.items.get_mut(&hidden.id).unwrap().decision = "certified".into();
        assert_eq!(apply(&adj, &o).unwrap_err(), "OVERRIDE_DECISION");
        let mut stale = Overrides::new(&adj);
        stale.summary_sha256 = "f".repeat(64);
        assert_eq!(apply(&adj, &stale).unwrap_err(), "OVERRIDE_BINDING");
        let mut unknown = Overrides::new(&adj);
        unknown
            .items
            .insert("relation/nope".into(), Override::default());
        assert_eq!(apply(&adj, &unknown).unwrap_err(), "OVERRIDE_UNKNOWN_ITEM");
        assert_eq!(flip("metric", "demoted"), Some("certified"));
    }

    #[test]
    fn formats_numbers_and_dates() {
        assert_eq!(magnitude(120_000), "120k");
        assert_eq!(magnitude(1_200), "1.2k");
        assert_eq!(magnitude(3_000_000), "3M");
        assert_eq!(number(&json!(0.71)), "0.71");
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_793_836_800), "2026-11-05T00:00:00Z");
    }
}
