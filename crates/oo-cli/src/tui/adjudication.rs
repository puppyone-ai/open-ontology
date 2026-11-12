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
    /// Relationship recall coverage; not part of the summary hash. Absent in
    /// legacy documents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery: Option<Discovery>,
    pub graph: Graph,
    pub summary_sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Discovery {
    pub status: String,
    #[serde(default)]
    pub budget: u64,
    #[serde(default)]
    pub checked: u64,
    #[serde(default)]
    pub remaining: u64,
    #[serde(default)]
    pub unchecked: Vec<Unchecked>,
}

/// A candidate relation the discovery budget did not reach.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Unchecked {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub from_columns: Vec<String>,
    #[serde(default)]
    pub to_columns: Vec<String>,
}

/// The one override key that is not an item: the owner's explicit acceptance
/// of an incomplete relationship discovery.
pub const DISCOVERY_OVERRIDE: &str = "discovery/relationships";
pub const ACCEPT_INCOMPLETE: &str = "accept_incomplete";

impl Adjudication {
    /// Relations the discovery budget left unchecked, when discovery is
    /// incomplete; `None` for complete or legacy documents.
    pub fn unchecked_relations(&self) -> Option<usize> {
        self.discovery
            .as_ref()
            .filter(|d| d.status == "incomplete")
            .map(|d| d.unchecked.len().max(d.remaining as usize))
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dictionary_bindings_sha256: Option<String>,
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

    /// Whether the owner accepted an incomplete relationship discovery.
    pub fn accepts_incomplete(&self) -> bool {
        self.items
            .get(DISCOVERY_OVERRIDE)
            .is_some_and(|o| o.decision == ACCEPT_INCOMPLETE && !o.reason.trim().is_empty())
    }

    /// Overrides that change an item decision (the discovery acceptance is
    /// not one).
    pub fn decision_count(&self) -> usize {
        self.items
            .keys()
            .filter(|k| k.as_str() != DISCOVERY_OVERRIDE)
            .count()
    }
}

/// Relation evidence says a JOIN on it can multiply child rows.
pub fn fans_out(item: &Item) -> bool {
    item.class == "relation"
        && (item.evidence.get("parent_unique") == Some(&Value::Bool(false))
            || item
                .evidence
                .get("max_fanout")
                .and_then(Value::as_u64)
                .is_some_and(|n| n > 1))
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
        if id == DISCOVERY_OVERRIDE {
            // Not an item: acknowledges unchecked relations, changes nothing.
            if adj.unchecked_relations().is_none() {
                return Err("OVERRIDE_UNKNOWN_ITEM");
            }
            if change.decision != ACCEPT_INCOMPLETE {
                return Err("OVERRIDE_DECISION");
            }
            if change.reason.trim().is_empty() {
                return Err("OVERRIDE_REASON_REQUIRED");
            }
            continue;
        }
        let item = out
            .items
            .iter_mut()
            .find(|i| &i.id == id)
            .ok_or("OVERRIDE_UNKNOWN_ITEM")?;
        let (a, b) = decisions(&item.class).ok_or("OVERRIDE_CLASS")?;
        if change.decision != a && change.decision != b {
            return Err("OVERRIDE_DECISION");
        }
        // Every change of a product decision carries the owner's reason; the
        // builder refuses an override without one, so refuse it here first.
        if change.reason.trim().is_empty() {
            return Err(
                if item.class == "sensitive" && change.decision == "visible" {
                    "OVERRIDE_REASON_REQUIRED"
                } else {
                    "OVERRIDE_REASON_EMPTY"
                },
            );
        }
        // Owner review cannot turn a fan-out JOIN into a relationship.
        if change.decision == "adopted" && fans_out(item) {
            return Err("OVERRIDE_FANOUT");
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
    if adj.unchecked_relations().is_some() && !overrides.is_some_and(Overrides::accepts_incomplete)
    {
        return Err("CONFIRM_DISCOVERY_INCOMPLETE");
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

/// One-line evidence summary, e.g. `distinct 0.998 · declared fk · many-to-one`.
pub fn evidence_line(item: &Item) -> String {
    if item.class == "relation" {
        return relation_line(item);
    }
    let mut parts = Vec::new();
    for (key, value) in &item.evidence {
        let name = match key.as_str() {
            "join_cooccurrence" => "joins",
            "name_similarity" => "name",
            "format_hit_rate" => "format hits",
            "distinct_ratio" => "distinct",
            "non_null_ratio" => "non-null",
            "null_rows" => "null rows",
            "empty_rows" => "empty rows",
            // Lists, provenance and bookkeeping stay out of the one-liner.
            "considered" | "key" | "method" | "query_log" | "full_scan" | "display" => continue,
            other => other,
        }
        .replace('_', " ");
        let text = match (key.as_str(), value) {
            (_, Value::Null | Value::Array(_) | Value::Object(_)) => continue,
            ("key_source", Value::String(s)) => s.replace('_', " "),
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

fn ev_f64(item: &Item, key: &str) -> Option<f64> {
    item.evidence.get(key).and_then(Value::as_f64)
}

fn ev_u64(item: &Item, key: &str) -> u64 {
    item.evidence.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Scope columns that make a repeating parent key unique.
pub fn scope_columns(item: &Item) -> Vec<&str> {
    item.evidence
        .get("scope_columns")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Role of a relation when one child has several edges to the same parent.
pub fn role(item: &Item) -> Option<&str> {
    item.evidence
        .get("role")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty())
}

/// Relation evidence, most decisive first: coverage, structural flaws
/// (fan-out, scope, collisions), kind, role, unmatched rows, cardinality,
/// size, signals.
fn relation_line(item: &Item) -> String {
    let mut parts = Vec::new();
    if let Some(c) = item.evidence.get("containment") {
        parts.push(format!("rows {}", number(c)));
    }
    if let Some(d) = item.evidence.get("distinct_containment") {
        parts.push(format!("distinct {}", number(d)));
    }
    let fanout = ev_u64(item, "max_fanout");
    if fanout > 1 {
        parts.push(format!("fan-out ×{fanout}"));
    }
    let scope = scope_columns(item);
    if !scope.is_empty() {
        parts.push(format!("scope {}", scope.join(",")));
    }
    let collisions = ev_u64(item, "normalized_collisions");
    if collisions > 0 {
        parts.push(format!("{collisions} collisions"));
    }
    if item.evidence.get("parent_unique") == Some(&Value::Bool(false)) {
        parts.push("parent not unique".into());
    }
    if let Some(kind) = item.evidence.get("kind").and_then(Value::as_str) {
        parts.push(
            match kind {
                "snapshot_name_mapping" => "name mapping",
                other => other,
            }
            .replace('_', " "),
        );
    }
    // Only open recall is worth naming; declared and model edges say so in kind.
    if item.evidence.get("source").and_then(Value::as_str) == Some("discovery") {
        parts.push("discovered".into());
    }
    if let Some(r) = role(item) {
        parts.push(format!("as {r}"));
    }
    let unmatched = ev_u64(item, "unmatched_rows");
    if unmatched > 0 {
        parts.push(format!("{} unmatched", magnitude(unmatched)));
    }
    if let Some(Value::String(s)) = item.evidence.get("cardinality") {
        parts.push(s.replace('_', "-"));
    }
    if let Some(v) = item.evidence.get("rows_checked") {
        parts.push(format!("{} rows", number(v)));
    }
    if let Some(v) = item.evidence.get("join_cooccurrence") {
        parts.push(format!("joins {}", number(v)));
    }
    if let Some(v) = item.evidence.get("name_similarity") {
        parts.push(format!("name {}", number(v)));
    }
    if item.evidence.get("parent_unique") == Some(&Value::Bool(true)) {
        parts.push("parent unique".into());
    }
    parts.join(" · ")
}

/// The coverage threshold that applies to a relation: the lower one when a
/// naming, JOIN or declared-key signal backs it.
pub fn needed_coverage(item: &Item, thresholds: &Map<String, Value>) -> f64 {
    let th = |k: &str, d: f64| thresholds.get(k).and_then(Value::as_f64).unwrap_or(d);
    let signal = ev_f64(item, "name_similarity").is_some_and(|s| s >= 0.5)
        || ev_f64(item, "join_cooccurrence").is_some_and(|j| j > 0.0)
        || item.evidence.get("kind").and_then(Value::as_str) == Some("declared_fk");
    if signal {
        th("relation_containment_with_signal", 0.9)
    } else {
        th("relation_containment_strict", 0.98)
    }
}

/// What a rejected relation lacked, e.g. `containment 0.71 < 0.9` or
/// `scope valid_from required, fan-out ×3`. The threshold applies to the
/// lower of row and distinct coverage.
pub fn missing_evidence(item: &Item, thresholds: &Map<String, Value>) -> String {
    let mut missing = Vec::new();
    let scope = scope_columns(item);
    let fanout = ev_u64(item, "max_fanout");
    if !scope.is_empty() {
        missing.push(format!("scope {} required", scope.join(",")));
    }
    if fanout > 1 {
        missing.push(format!("fan-out ×{fanout}"));
    } else if scope.is_empty() && item.evidence.get("parent_unique") == Some(&Value::Bool(false)) {
        missing.push("parent not unique".into());
    }
    let collisions = ev_u64(item, "normalized_collisions");
    if collisions > 0 {
        missing.push(format!("{collisions} keys collide after trim/lower"));
    }
    if let Some(c) = ev_f64(item, "containment") {
        let need = needed_coverage(item, thresholds);
        let d = ev_f64(item, "distinct_containment").unwrap_or(c);
        let n = |x: f64| number(&json!(x));
        if d < c && d < need {
            missing.push(format!("distinct {} < {} (rows {})", n(d), n(need), n(c)));
        } else if c < need {
            missing.push(format!("containment {} < {}", n(c), n(need)));
        }
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
pub(crate) mod tests {
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
    fn dictionary_definition_digest_survives_parse_and_binds_confirmation() {
        let mut adj = fixture();
        let original = adj.summary_sha256.clone();
        adj.graph.dictionary_bindings_sha256 = Some("a".repeat(64));
        adj.summary_sha256 = summary_sha256(&adj);
        assert_ne!(adj.summary_sha256, original);
        assert_eq!(parse(json!(adj)).unwrap(), adj);
        let mut changed = json!(adj);
        changed["graph"]["dictionary_bindings_sha256"] = json!("b".repeat(64));
        assert_eq!(parse(changed).unwrap_err(), "ADJUDICATION_SUMMARY_MISMATCH");
    }

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

    /// The fixture with two relations left unchecked by the discovery budget.
    pub(crate) fn incomplete() -> Adjudication {
        let mut adj = fixture();
        let unchecked = |id: &str, from: &str, col: &str, to: &str| Unchecked {
            id: id.into(),
            kind: "inferred_id".into(),
            from: from.into(),
            to: to.into(),
            from_columns: vec![col.into()],
            to_columns: vec!["id".into()],
        };
        adj.discovery = Some(Discovery {
            status: "incomplete".into(),
            budget: 19,
            checked: 19,
            remaining: 2,
            unchecked: vec![
                unchecked(
                    "relation/parcels.order_id",
                    "lg.parcels",
                    "order_id",
                    "lg.orders",
                ),
                unchecked(
                    "relation/vehicles.driver_id",
                    "lg.vehicles",
                    "driver_id",
                    "lg.drivers",
                ),
            ],
        });
        adj
    }

    fn accept(reason: &str) -> Override {
        Override {
            decision: ACCEPT_INCOMPLETE.into(),
            reason: reason.into(),
        }
    }

    #[test]
    fn discovery_is_optional_and_outside_the_summary() {
        let adj = fixture();
        assert_eq!(adj.discovery.as_ref().unwrap().status, "complete");
        assert_eq!(adj.unchecked_relations(), None);
        let mut legacy = json!(adj);
        legacy.as_object_mut().unwrap().remove("discovery");
        let legacy = parse(legacy).unwrap();
        assert_eq!(legacy.discovery, None);
        assert!(
            json!(legacy).get("discovery").is_none(),
            "legacy bytes unchanged"
        );
        let open = incomplete();
        assert_eq!(open.unchecked_relations(), Some(2));
        assert_eq!(summary_sha256(&open), adj.summary_sha256);
        assert_eq!(parse(json!(open)).unwrap(), open);
    }

    #[test]
    fn incomplete_discovery_needs_an_explicit_reasoned_acceptance() {
        let at = "2026-11-09T00:00:00Z";
        let adj = incomplete();
        assert_eq!(
            confirm(&adj, None, &adj.summary_sha256, "ok", at).unwrap_err(),
            "CONFIRM_DISCOVERY_INCOMPLETE"
        );
        let mut o = Overrides::new(&adj);
        o.items.insert(DISCOVERY_OVERRIDE.into(), accept("  "));
        assert_eq!(apply(&adj, &o).unwrap_err(), "OVERRIDE_REASON_REQUIRED");
        o.items.get_mut(DISCOVERY_OVERRIDE).unwrap().decision = "adopted".into();
        assert_eq!(apply(&adj, &o).unwrap_err(), "OVERRIDE_DECISION");
        o.items.insert(
            DISCOVERY_OVERRIDE.into(),
            accept("budget spent; resume next week"),
        );
        // Acceptance changes no item, edge or summary.
        let effective = apply(&adj, &o).unwrap();
        assert_eq!(effective, adj);
        assert_eq!(o.decision_count(), 0);
        assert!(o.accepts_incomplete());
        let doc = confirm(&adj, Some(&o), &adj.summary_sha256, "ok", at).unwrap();
        assert_eq!(doc["summary_sha256"], json!(adj.summary_sha256));
        assert_eq!(doc["overrides_sha256"], json!(overrides_sha256(&o)));

        // A complete (or legacy) discovery has nothing to accept.
        let complete = fixture();
        let mut o = Overrides::new(&complete);
        o.items.insert(DISCOVERY_OVERRIDE.into(), accept("why not"));
        assert_eq!(apply(&complete, &o).unwrap_err(), "OVERRIDE_UNKNOWN_ITEM");
        let mut o = Overrides::new(&adj);
        o.items.insert("discovery/other".into(), accept("x"));
        assert_eq!(apply(&adj, &o).unwrap_err(), "OVERRIDE_UNKNOWN_ITEM");
    }

    #[test]
    fn fan_out_relations_cannot_be_adopted_by_override() {
        let adj = fixture();
        let mut o = Overrides::new(&adj);
        o.items.insert(
            "relation/orders.route_code".into(),
            Override {
                decision: "adopted".into(),
                reason: "looks fine".into(),
            },
        );
        assert_eq!(apply(&adj, &o).unwrap_err(), "OVERRIDE_FANOUT");
        // parent_unique false alone is enough.
        let mut alone = adj.clone();
        let item = alone
            .items
            .iter_mut()
            .find(|i| i.id == "relation/orders.route_code")
            .unwrap();
        item.evidence.insert("max_fanout".into(), json!(1));
        assert!(fans_out(item));
        assert_eq!(apply(&alone, &o).unwrap_err(), "OVERRIDE_FANOUT");
        // A collision is ambiguous but not a fan-out: the owner may adopt it.
        let mut ok = Overrides::new(&adj);
        ok.items.insert(
            "relation/warehouses.station_code".into(),
            Override {
                decision: "adopted".into(),
                reason: "codes cleaned upstream".into(),
            },
        );
        assert!(apply(&adj, &ok).is_ok());
    }

    #[test]
    fn relation_evidence_shows_row_and_distinct_coverage() {
        let adj = fixture();
        let item = |id: &str| adj.items.iter().find(|i| i.id == id).unwrap();
        let th = &adj.thresholds;
        let line = evidence_line(item("relation/shipments.origin_wh"));
        assert!(
            line.starts_with("rows 0.71 · distinct 0.75 · inferred id · 284k unmatched"),
            "{line}"
        );
        assert_eq!(
            missing_evidence(item("relation/shipments.origin_wh"), th),
            "containment 0.71 < 0.98"
        );
        assert_eq!(
            missing_evidence(item("relation/drivers.employee_id"), th),
            "distinct 0.52 < 0.9 (rows 0.64)"
        );
        let scoped = item("relation/orders.route_code");
        assert!(
            evidence_line(scoped)
                .contains("fan-out ×3 · scope valid_from · parent not unique · scoped"),
            "{}",
            evidence_line(scoped)
        );
        assert_eq!(
            missing_evidence(scoped, th),
            "scope valid_from required, fan-out ×3"
        );
        assert_eq!(
            missing_evidence(item("relation/warehouses.station_code"), th),
            "2 keys collide after trim/lower"
        );
        let role = evidence_line(item("relation/routes.dest_station_id"));
        assert!(role.contains("as dest_station"), "{role}");
        assert!(
            evidence_line(item("relation/orders.customer_id")).contains("declared fk"),
            "kind shown"
        );
        // Legacy evidence (no distinct coverage) keeps the old wording.
        let mut legacy = item("relation/drivers.employee_id").clone();
        legacy.evidence.remove("distinct_containment");
        assert_eq!(missing_evidence(&legacy, th), "containment 0.64 < 0.9");
        let mut open = item("relation/warehouses.station_code").clone();
        open.evidence.insert("source".into(), json!("discovery"));
        assert!(
            evidence_line(&open).contains("inferred id · discovered"),
            "{}",
            evidence_line(&open)
        );
        open.evidence.insert("source".into(), json!("model"));
        assert!(!evidence_line(&open).contains("model"));
        let key = evidence_line(item("entity_key/calendar"));
        assert!(key.contains("declared pk") && !key.contains('['), "{key}");
    }
}
