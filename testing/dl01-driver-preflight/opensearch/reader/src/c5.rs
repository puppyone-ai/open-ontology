//! C5 full-diff capture for one approved OpenSearch index.
//!
//! OpenSearch has no change stream and no transactions. This module compares a
//! finished PIT keyset to the local shadow by key and canonical row text.
//! Deletes stay `DELETES_LAGGED` until a configured reconcile cycle, then become
//! tombstones. A tombstone is not cleared when the same key appears again.
//! Consistency is B. `transactional`, CDC, and a cursor continuation are refused.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const CONFIG_PROTOCOL: &str = "oo.opensearch.c5.config/v1";
pub const SHADOW_PROTOCOL: &str = "oo.opensearch.c5.shadow/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleSpec {
    pub name: String,
    pub every_ms: u64,
    pub reconcile_every: u64,
}

#[derive(Clone, Debug)]
pub struct SourceConfig {
    pub source_id: String,
    pub secret_ref: String,
    pub connector: String,
    pub target: String,
    pub capture_method: String,
    pub consistency: String,
    pub sync: String,
    pub delete_detection: String,
    pub approved_index: String,
    pub relation: String,
    pub key_field: String,
    pub budget_unit: String,
    pub budget_rows: u64,
    pub locale: String,
    pub timezone: String,
    pub role: String,
    pub approved_fields: Vec<String>,
    pub field_types: BTreeMap<String, String>,
    pub window: String,
    pub max_rows_per_sec: u64,
    pub schedules: Vec<ScheduleSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LiveRow {
    pub key: String,
    pub doc_id: String,
    pub canonical: String,
    pub hash: String,
    pub source_json: String,
    pub unclassified: Vec<String>,
    pub tombstone: bool,
    pub generation: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shadow {
    pub protocol: String,
    pub next_generation: u64,
    pub watermark: u64,
    pub rows: BTreeMap<String, LiveRow>,
    pub quality: String,
    pub publication: String,
    pub fires_ms: BTreeMap<String, u64>,
    pub cycles: BTreeMap<String, u64>,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            protocol: SHADOW_PROTOCOL.to_string(),
            next_generation: 1,
            watermark: 0,
            rows: BTreeMap::new(),
            quality: "unpublished".into(),
            publication: "unpublished".into(),
            fires_ms: BTreeMap::new(),
            cycles: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleKind {
    Complete,
    Partial,
    Expired,
}

#[derive(Clone, Debug)]
pub struct SnapDoc {
    pub doc_id: String,
    pub source: Value,
}

#[derive(Clone, Debug)]
pub struct CycleInput {
    pub schedule: String,
    pub now_ms: u64,
    pub now_minute: u32,
    pub kind: CycleKind,
    pub docs: Vec<SnapDoc>,
    pub mapping_dynamic: String,
    pub mapping_types: BTreeMap<String, String>,
    pub resume_cursor: Option<String>,
    pub typed_total_relation: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CycleReport {
    pub applied: bool,
    pub discarded: bool,
    pub reason: String,
    pub schedule: String,
    pub configured_every_ms: u64,
    pub actual_wait_ms: u64,
    pub reconcile: bool,
    pub continuation: bool,
    pub cursor_repaired: bool,
    pub resume_cursor_sent_to_source: bool,
    pub watermark_before: u64,
    pub watermark_after: u64,
    pub generation: u64,
    pub quality: String,
    pub publication: String,
    pub inserted: Vec<String>,
    pub updated: Vec<String>,
    pub tombstoned: Vec<String>,
    pub suspected_deletes: Vec<String>,
    pub resurrect_refused: Vec<String>,
    pub unclassified_updates: Vec<String>,
    pub typed_update_count: usize,
    pub live_count: usize,
    pub capture_method: String,
    pub consistency: String,
    pub position_expired: bool,
    pub schema_break: bool,
}

pub fn parse_config(value: &Value) -> Result<SourceConfig, String> {
    let protocol = req_str(value, "protocol")?;
    if protocol != CONFIG_PROTOCOL {
        return Err(format!("config_protocol:{protocol}"));
    }
    let approved_fields = string_list(value, "approved_fields")?;
    if approved_fields.is_empty() {
        return Err("approved_fields_empty".into());
    }
    let mut seen = BTreeSet::new();
    for field in &approved_fields {
        if !seen.insert(field.clone()) {
            return Err(format!("duplicate_approved_field:{field}"));
        }
        if !field_name_ok(field) {
            return Err(format!("field_name:{field}"));
        }
    }
    let field_types = string_map(value, "field_types")?;
    if field_types.len() != approved_fields.len() || approved_fields.iter().any(|field| !field_types.contains_key(field)) {
        return Err("field_types_must_match_approved_fields".into());
    }
    let schedules_value = value.get("schedules").and_then(Value::as_array).ok_or("schedules_missing")?;
    if schedules_value.is_empty() {
        return Err("schedules_empty".into());
    }
    let mut schedules = Vec::new();
    let mut names = BTreeSet::new();
    for item in schedules_value {
        let name = req_str(item, "name")?;
        if !names.insert(name.clone()) {
            return Err(format!("duplicate_schedule:{name}"));
        }
        let every_ms = req_u64(item, "every_ms")?;
        let reconcile_every = req_u64(item, "reconcile_every")?;
        if every_ms < 1000 {
            return Err(format!("every_ms_below_1000:{name}"));
        }
        if reconcile_every == 0 {
            return Err(format!("reconcile_every_zero:{name}"));
        }
        schedules.push(ScheduleSpec { name, every_ms, reconcile_every });
    }
    let limits = value.get("limits").ok_or("limits_missing")?;
    let window = req_str(limits, "window")?;
    window_contains(&window, 0)?;
    let max_rows_per_sec = req_u64(limits, "max_rows_per_sec")?;
    if max_rows_per_sec == 0 {
        return Err("max_rows_per_sec_zero".into());
    }
    if value.get("incremental").is_some() {
        return Err("c5_rejects_cursor_incremental".into());
    }
    let key_field = req_str(value, "key_field")?;
    if !approved_fields.iter().any(|field| field == &key_field) {
        return Err("key_field_not_approved".into());
    }
    let requested = value.get("requested_fields").and_then(Value::as_array);
    if let Some(items) = requested {
        for item in items {
            let field = item.as_str().ok_or("requested_field_not_string")?;
            if !approved_fields.iter().any(|approved| approved == field) {
                return Err(format!("unapproved_field:{field}"));
            }
        }
    }
    Ok(SourceConfig {
        source_id: req_str(value, "source_id")?,
        secret_ref: req_str(value, "secret_ref")?,
        connector: req_str(value, "connector")?,
        target: req_str(value, "target")?,
        capture_method: req_str(value, "capture_method")?,
        consistency: req_str(value, "consistency")?,
        sync: req_str(value, "sync")?,
        delete_detection: req_str(value, "delete_detection")?,
        approved_index: req_str(value, "approved_index")?,
        relation: req_str(value, "relation")?,
        key_field,
        budget_unit: req_str(value, "budget_unit")?,
        budget_rows: req_u64(value, "budget_rows")?,
        locale: req_str(value, "locale")?,
        timezone: req_str(value, "timezone")?,
        role: req_str(value, "role")?,
        approved_fields,
        field_types,
        window,
        max_rows_per_sec,
        schedules,
    })
}

pub fn admit_config(config: &SourceConfig, secret_path_exists: bool, requested_index: &str) -> Result<(), String> {
    if !secret_path_exists {
        return Err("secret_path_missing".into());
    }
    if !config.secret_ref.starts_with("secret://") || config.secret_ref.len() < "secret://".len() + 3 {
        return Err("secret_ref_invalid".into());
    }
    if config.connector != "opensearch" {
        return Err(format!("connector_unverified:{}", config.connector));
    }
    if config.capture_method != "C5" {
        return Err(format!("capture_method_refused:{}", config.capture_method));
    }
    if config.sync != "full" {
        return Err(format!("sync_refused:{}", config.sync));
    }
    if config.consistency != "B" {
        return Err(format!("consistency_refused:{}", config.consistency));
    }
    if config.delete_detection != "reconcile" {
        return Err(format!("delete_detection_refused:{}", config.delete_detection));
    }
    if config.budget_unit != "Rows" || config.budget_rows == 0 {
        return Err("budget_refused".into());
    }
    if config.locale.is_empty() || config.timezone.is_empty() || config.role.is_empty() {
        return Err("identity_incomplete".into());
    }
    if requested_index != config.approved_index {
        return Err("index_outside_approved".into());
    }
    if !index_name_ok(&config.approved_index) || !sql_ident_ok(&config.relation) {
        return Err("name_refused".into());
    }
    Ok(())
}

pub fn window_contains(window: &str, minute: u32) -> Result<bool, String> {
    let (start_text, end_text) = window.split_once('-').ok_or("window_syntax")?;
    let start = parse_minute(start_text)?;
    let end = parse_minute(end_text)?;
    if minute > 24 * 60 - 1 {
        return Err("minute_out_of_range".into());
    }
    Ok(if start <= end {
        minute >= start && minute <= end
    } else {
        minute >= start || minute <= end
    })
}

pub fn precheck_cycle(shadow: &Shadow, config: &SourceConfig, schedule_name: &str, now_ms: u64, now_minute: u32) -> Result<u64, String> {
    let schedule = find_schedule(config, schedule_name)?;
    if !window_contains(&config.window, now_minute)? {
        return Err("outside_window".into());
    }
    match shadow.fires_ms.get(&schedule.name).copied() {
        Some(previous) if now_ms < previous.saturating_add(schedule.every_ms) => {
            Err(format!("early_fire:remaining_ms:{}", previous + schedule.every_ms - now_ms))
        }
        Some(previous) => Ok(now_ms - previous),
        None => Ok(0),
    }
}

pub fn apply_cycle(shadow: &mut Shadow, config: &SourceConfig, input: &CycleInput) -> Result<CycleReport, String> {
    let schedule = find_schedule(config, &input.schedule)?;
    let actual_wait_ms = precheck_cycle(shadow, config, &schedule.name, input.now_ms, input.now_minute)?;
    let watermark_before = shadow.watermark;
    let base = CycleReport {
        applied: false,
        discarded: true,
        reason: String::new(),
        schedule: schedule.name.clone(),
        configured_every_ms: schedule.every_ms,
        actual_wait_ms,
        reconcile: false,
        continuation: false,
        cursor_repaired: false,
        resume_cursor_sent_to_source: false,
        watermark_before,
        watermark_after: watermark_before,
        generation: shadow.next_generation,
        quality: shadow.quality.clone(),
        publication: shadow.publication.clone(),
        inserted: Vec::new(),
        updated: Vec::new(),
        tombstoned: Vec::new(),
        suspected_deletes: Vec::new(),
        resurrect_refused: Vec::new(),
        unclassified_updates: Vec::new(),
        typed_update_count: 0,
        live_count: live_count(shadow),
        capture_method: "C5".into(),
        consistency: "B".into(),
        position_expired: false,
        schema_break: false,
    };
    shadow.fires_ms.insert(schedule.name.clone(), input.now_ms);
    let cycle_no = shadow.cycles.get(&schedule.name).copied().unwrap_or(0) + 1;
    shadow.cycles.insert(schedule.name.clone(), cycle_no);
    let reconcile = cycle_no % schedule.reconcile_every == 0;
    if mapping_break(config, &input.mapping_dynamic, &input.mapping_types) {
        shadow.quality = "schema_break".into();
        shadow.publication = "paused".into();
        let mut report = base;
        report.reason = "schema_break".into();
        report.reconcile = reconcile;
        report.quality = shadow.quality.clone();
        report.publication = shadow.publication.clone();
        report.schema_break = true;
        return Ok(report);
    }
    if input.kind == CycleKind::Expired {
        let mut report = base;
        report.reason = "POSITION_EXPIRED".into();
        report.position_expired = true;
        report.reconcile = reconcile;
        return Ok(report);
    }
    if input.kind == CycleKind::Partial {
        let mut report = base;
        report.reason = "partial_snapshot_discarded".into();
        report.reconcile = reconcile;
        return Ok(report);
    }
    if input.typed_total_relation != "eq" {
        let mut report = base;
        report.reason = format!("typed_total_relation:{}", input.typed_total_relation);
        return Ok(report);
    }
    let projected = match project_docs(config, &input.docs) {
        Ok(rows) => rows,
        Err(error) => {
            let mut report = base;
            report.reason = error;
            report.reconcile = reconcile;
            return Ok(report);
        }
    };
    if projected.len() as u64 > config.budget_rows {
        let mut report = base;
        report.reason = "budget_exceeded".into();
        report.reconcile = reconcile;
        return Ok(report);
    }
    let generation = shadow.next_generation;
    shadow.next_generation = generation.saturating_add(1);
    let mut report = base;
    report.discarded = false;
    report.applied = true;
    report.generation = generation;
    report.reconcile = reconcile;
    report.reason = if input.resume_cursor.is_some() {
        "full_reread_cursor_not_used".into()
    } else {
        "applied".into()
    };
    let mut seen = BTreeSet::new();
    for row in &projected {
        seen.insert(row.key.clone());
        match shadow.rows.get(&row.key) {
            Some(existing) if existing.tombstone => {
                report.resurrect_refused.push(row.key.clone());
            }
            Some(existing) if existing.canonical == row.canonical && existing.source_json == row.source_json && !existing.tombstone => {}
            Some(existing) if existing.canonical == row.canonical && !existing.tombstone => {
                let mut kept = existing.clone();
                kept.unclassified = row.unclassified.clone();
                kept.source_json = row.source_json.clone();
                kept.doc_id = row.doc_id.clone();
                kept.generation = generation;
                report.unclassified_updates.push(row.key.clone());
                shadow.rows.insert(row.key.clone(), kept);
            }
            Some(existing) => {
                let mut kept = row.clone();
                kept.generation = generation;
                kept.tombstone = false;
                if existing.canonical != row.canonical {
                    report.updated.push(row.key.clone());
                    report.typed_update_count += 1;
                }
                shadow.rows.insert(row.key.clone(), kept);
            }
            None => {
                let mut inserted = row.clone();
                inserted.generation = generation;
                inserted.tombstone = false;
                report.inserted.push(inserted.key.clone());
                shadow.rows.insert(row.key.clone(), inserted);
            }
        }
    }
    let mut missing = Vec::new();
    for (key, row) in &shadow.rows {
        if !row.tombstone && !seen.contains(key) {
            missing.push(key.clone());
        }
    }
    missing.sort();
    if reconcile {
        for key in &missing {
            if let Some(row) = shadow.rows.get_mut(key) {
                row.tombstone = true;
                row.generation = generation;
            }
            report.tombstoned.push(key.clone());
        }
        shadow.quality = "caught_up".into();
    } else {
        report.suspected_deletes = missing;
        shadow.quality = "DELETES_LAGGED".into();
    }
    shadow.watermark = generation;
    shadow.publication = "shadow_current".into();
    report.watermark_after = shadow.watermark;
    report.quality = shadow.quality.clone();
    report.publication = shadow.publication.clone();
    report.live_count = live_count(shadow);
    Ok(report)
}

pub fn project_docs(config: &SourceConfig, docs: &[SnapDoc]) -> Result<Vec<LiveRow>, String> {
    let mut rows = Vec::new();
    let mut keys = BTreeSet::new();
    for doc in docs {
        let object = doc.source.as_object().ok_or("source_not_object")?;
        let key_value = object.get(&config.key_field).ok_or("missing_key")?;
        let key = key_value.as_str().filter(|text| key_ok(text)).ok_or("key_not_stable_string")?;
        if !keys.insert(key.to_string()) {
            return Err(format!("duplicate_key:{key}"));
        }
        let canonical = canonical_text(&config.approved_fields, object);
        let mut unclassified: Vec<String> = object
            .keys()
            .filter(|name| !config.approved_fields.iter().any(|field| field == *name))
            .cloned()
            .collect();
        unclassified.sort();
        rows.push(LiveRow {
            key: key.to_string(),
            doc_id: doc.doc_id.clone(),
            hash: sha256_hex(canonical.as_bytes()),
            canonical,
            source_json: serde_json::to_string(&doc.source).map_err(|error| format!("source_json:{error}"))?,
            unclassified,
            tombstone: false,
            generation: 0,
        });
    }
    rows.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(rows)
}

pub fn canonical_text(fields: &[String], object: &Map<String, Value>) -> String {
    let mut projected = Map::new();
    for field in fields {
        let value = object.get(field).cloned().unwrap_or(Value::Null);
        projected.insert(field.clone(), value);
    }
    serde_json::to_string(&Value::Object(projected)).unwrap_or_else(|_| "{}".to_string())
}

pub fn full_cell_diff(source_canonical: &BTreeMap<String, String>, pg_canonical: &BTreeMap<String, String>) -> Vec<String> {
    let mut diffs = Vec::new();
    let mut keys = BTreeSet::new();
    keys.extend(source_canonical.keys().cloned());
    keys.extend(pg_canonical.keys().cloned());
    for key in keys {
        match (source_canonical.get(&key), pg_canonical.get(&key)) {
            (Some(left), Some(right)) if left == right => {}
            (Some(left), Some(right)) => diffs.push(format!("cell:{key}:{left}:{right}")),
            (Some(_), None) => diffs.push(format!("missing_pg:{key}")),
            (None, Some(_)) => diffs.push(format!("extra_pg:{key}")),
            (None, None) => {}
        }
    }
    diffs
}

pub fn mutation_sql(relation: &str, rows: &[LiveRow]) -> Result<String, String> {
    if !sql_ident_ok(relation) {
        return Err("relation_name".into());
    }
    let mut out = String::new();
    out.push_str("BEGIN;\n");
    out.push_str(&format!(
        "CREATE TABLE IF NOT EXISTS {relation} (\n  doc_key text PRIMARY KEY,\n  doc_id text NOT NULL,\n  canonical text NOT NULL,\n  row_hash text NOT NULL,\n  doc text NOT NULL,\n  unclassified text NOT NULL,\n  tombstone boolean NOT NULL,\n  generation bigint NOT NULL\n);\n"
    ));
    out.push_str(&format!(
        "CREATE OR REPLACE VIEW {relation}_typed AS SELECT doc_key, canonical, row_hash, generation FROM {relation} WHERE NOT tombstone;\n"
    ));
    for row in rows {
        if !key_ok(&row.key) || !key_ok(&row.doc_id) {
            return Err("sql_key".into());
        }
        let unclassified = serde_json::to_string(&row.unclassified).map_err(|error| format!("unclassified_json:{error}"))?;
        out.push_str(&format!(
            "INSERT INTO {relation} (doc_key, doc_id, canonical, row_hash, doc, unclassified, tombstone, generation) VALUES ({key}, {doc_id}, {canonical}, {hash}, {doc}, {unclassified}, {tomb}, {generation}) ON CONFLICT (doc_key) DO UPDATE SET doc_id = EXCLUDED.doc_id, canonical = EXCLUDED.canonical, row_hash = EXCLUDED.row_hash, doc = EXCLUDED.doc, unclassified = EXCLUDED.unclassified, tombstone = EXCLUDED.tombstone, generation = EXCLUDED.generation WHERE {relation}.tombstone = false OR EXCLUDED.tombstone = true;\n",
            key = sql_literal(&row.key),
            doc_id = sql_literal(&row.doc_id),
            canonical = sql_literal(&row.canonical),
            hash = sql_literal(&row.hash),
            doc = sql_literal(&row.source_json),
            unclassified = sql_literal(&unclassified),
            tomb = if row.tombstone { "true" } else { "false" },
            generation = row.generation,
        ));
    }
    out.push_str("COMMIT;\n");
    Ok(out)
}

pub fn is_source_mutation(method: &str, path: &str) -> bool {
    if path.contains("/_search") {
        return false;
    }
    let method = method.to_ascii_uppercase();
    method == "PUT" || method == "DELETE" || method == "POST" || method == "PATCH"
}

pub fn changed_rows(before: &Shadow, after: &Shadow) -> Vec<LiveRow> {
    let mut rows = Vec::new();
    let mut keys = BTreeSet::new();
    keys.extend(before.rows.keys().cloned());
    keys.extend(after.rows.keys().cloned());
    for key in keys {
        match (before.rows.get(&key), after.rows.get(&key)) {
            (Some(left), Some(right)) if left == right => {}
            (_, Some(right)) => rows.push(right.clone()),
            (Some(left), None) => rows.push(left.clone()),
            (None, None) => {}
        }
    }
    rows
}

pub fn mapping_types_of(mapping: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(properties) = mapping.get("properties").and_then(Value::as_object) else {
        return out;
    };
    for (name, spec) in properties {
        if let Some(os_type) = spec.get("type").and_then(Value::as_str) {
            out.insert(name.clone(), os_type.to_string());
        } else if spec.get("properties").is_some() {
            // OpenSearch omits type when an object mapping only lists properties.
            // A present type, including nested, is kept above.
            out.insert(name.clone(), "object".to_string());
        }
    }
    out
}

pub fn schema_broken(config: &SourceConfig, dynamic: &str, types: &BTreeMap<String, String>) -> bool {
    mapping_break(config, dynamic, types)
}

fn mapping_break(config: &SourceConfig, dynamic: &str, types: &BTreeMap<String, String>) -> bool {
    if dynamic != "strict" {
        return true;
    }
    config.approved_fields.iter().any(|field| types.get(field).map(String::as_str) != Some(config.field_types[field].as_str()))
}

fn find_schedule<'a>(config: &'a SourceConfig, name: &str) -> Result<&'a ScheduleSpec, String> {
    config.schedules.iter().find(|item| item.name == name).ok_or_else(|| format!("unknown_schedule:{name}"))
}

fn live_count(shadow: &Shadow) -> usize {
    shadow.rows.values().filter(|row| !row.tombstone).count()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn key_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == ':')
}

fn field_name_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn index_name_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn sql_ident_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        && value.chars().next().is_some_and(|ch| ch.is_ascii_lowercase() || ch == '_')
}

fn req_str(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("missing_{key}"))
}

fn req_u64(value: &Value, key: &str) -> Result<u64, String> {
    value.get(key).and_then(Value::as_u64).ok_or_else(|| format!("missing_{key}"))
}

fn string_list(value: &Value, key: &str) -> Result<Vec<String>, String> {
    let items = value.get(key).and_then(Value::as_array).ok_or_else(|| format!("missing_{key}"))?;
    let mut out = Vec::new();
    for item in items {
        let text = item.as_str().ok_or_else(|| format!("{key}_not_string"))?;
        out.push(text.to_string());
    }
    Ok(out)
}

fn string_map(value: &Value, key: &str) -> Result<BTreeMap<String, String>, String> {
    let object = value.get(key).and_then(Value::as_object).ok_or_else(|| format!("missing_{key}"))?;
    let mut out = BTreeMap::new();
    for (name, item) in object {
        let text = item.as_str().ok_or_else(|| format!("{key}_not_string"))?;
        out.insert(name.clone(), text.to_string());
    }
    Ok(out)
}

fn parse_minute(text: &str) -> Result<u32, String> {
    let (hour_text, minute_text) = text.split_once(':').ok_or("window_syntax")?;
    if hour_text.len() != 2 || minute_text.len() != 2 {
        return Err("window_syntax".into());
    }
    let hour: u32 = hour_text.parse().map_err(|_| "window_syntax".to_string())?;
    let minute: u32 = minute_text.parse().map_err(|_| "window_syntax".to_string())?;
    if hour > 23 || minute > 59 {
        return Err("window_syntax".into());
    }
    Ok(hour * 60 + minute)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> SourceConfig {
        parse_config(&json!({
            "protocol": CONFIG_PROTOCOL,
            "source_id": "oo_dl01w1_44",
            "secret_ref": "secret://oo-dl01w1-44/opensearch",
            "connector": "opensearch",
            "target": "oo_dl01w1_44_ledger_pg",
            "capture_method": "C5",
            "consistency": "B",
            "sync": "full",
            "delete_detection": "reconcile",
            "approved_index": "oo-dl01w1-44-ledger",
            "relation": "ledger_rows",
            "key_field": "doc_key",
            "budget_unit": "Rows",
            "budget_rows": 40,
            "locale": "und",
            "timezone": "UTC",
            "role": "oo_r_ledger",
            "approved_fields": ["doc_key", "long_value", "note"],
            "requested_fields": ["doc_key", "note"],
            "field_types": {"doc_key": "keyword", "long_value": "long", "note": "text"},
            "limits": {"window": "00:00-23:59", "max_rows_per_sec": 50000},
            "schedules": [
                {"name": "fast", "every_ms": 1500, "reconcile_every": 2},
                {"name": "slow", "every_ms": 3000, "reconcile_every": 1}
            ]
        }))
        .unwrap()
    }

    fn doc(id: &str, key: &str, long_value: i64, note: &str) -> SnapDoc {
        SnapDoc {
            doc_id: id.into(),
            source: json!({"doc_key": key, "long_value": long_value, "note": note}),
        }
    }

    fn input(schedule: &str, now_ms: u64, docs: Vec<SnapDoc>) -> CycleInput {
        CycleInput {
            schedule: schedule.into(),
            now_ms,
            now_minute: 12 * 60,
            kind: CycleKind::Complete,
            docs,
            mapping_dynamic: "strict".into(),
            mapping_types: BTreeMap::from([
                ("doc_key".into(), "keyword".into()),
                ("long_value".into(), "long".into()),
                ("note".into(), "text".into()),
            ]),
            resume_cursor: None,
            typed_total_relation: "eq".into(),
        }
    }

    #[test]
    fn refuses_before_a_read_would_start() {
        let mut cfg = config();
        assert_eq!(admit_config(&cfg, false, &cfg.approved_index).unwrap_err(), "secret_path_missing");
        assert_eq!(admit_config(&cfg, true, "other-index").unwrap_err(), "index_outside_approved");
        cfg.consistency = "transactional".into();
        assert!(admit_config(&cfg, true, "oo-dl01w1-44-ledger").unwrap_err().contains("consistency_refused"));
        cfg.consistency = "B".into();
        cfg.capture_method = "C1".into();
        cfg.sync = "cdc".into();
        assert!(admit_config(&cfg, true, "oo-dl01w1-44-ledger").unwrap_err().contains("capture_method_refused"));
        let mut raw = json!({
            "protocol": CONFIG_PROTOCOL,
            "source_id": "oo_dl01w1_44",
            "secret_ref": "secret://oo-dl01w1-44/opensearch",
            "connector": "opensearch",
            "target": "oo_dl01w1_44_ledger_pg",
            "capture_method": "C3",
            "consistency": "B",
            "sync": "full",
            "delete_detection": "reconcile",
            "approved_index": "oo-dl01w1-44-ledger",
            "relation": "ledger_rows",
            "key_field": "doc_key",
            "budget_unit": "Rows",
            "budget_rows": 40,
            "locale": "und",
            "timezone": "UTC",
            "role": "oo_r_ledger",
            "approved_fields": ["doc_key"],
            "field_types": {"doc_key": "keyword"},
            "limits": {"window": "00:00-23:59", "max_rows_per_sec": 1},
            "incremental": {"cursor": "event_ms", "every": "1h", "overlap": "5m"},
            "schedules": [{"name": "fast", "every_ms": 1500, "reconcile_every": 1}]
        });
        assert_eq!(parse_config(&raw).unwrap_err(), "c5_rejects_cursor_incremental");
        raw.as_object_mut().unwrap().remove("incremental");
        let c3 = parse_config(&raw).unwrap();
        assert!(admit_config(&c3, true, &c3.approved_index).unwrap_err().contains("capture_method_refused:C3"));
        raw["capture_method"] = json!("C5");
        raw["sync"] = json!("auto");
        let auto = parse_config(&raw).unwrap();
        assert!(admit_config(&auto, true, &auto.approved_index).unwrap_err().contains("sync_refused:auto"));
        raw["requested_fields"] = json!(["not_approved"]);
        raw["sync"] = json!("full");
        assert!(parse_config(&raw).unwrap_err().contains("unapproved_field"));
    }

    #[test]
    fn two_periods_do_not_share_a_timer_and_early_fire_does_not_mutate() {
        let cfg = config();
        let mut shadow = Shadow::default();
        let first = input("fast", 5_000, vec![doc("led-01", "ledger-open", 1, "a")]);
        apply_cycle(&mut shadow, &cfg, &first).unwrap();
        let watermark = shadow.watermark;
        let mut early = input("fast", 5_100, vec![doc("led-01", "ledger-open", 1, "b")]);
        early.now_minute = 12 * 60;
        let error = apply_cycle(&mut shadow, &cfg, &early).unwrap_err();
        assert!(error.starts_with("early_fire:"));
        assert_eq!(shadow.watermark, watermark);
        assert!(shadow.rows["ledger-open"].canonical.contains("\"note\":\"a\""));
        let slow = input("slow", 5_100, vec![doc("led-01", "ledger-open", 1, "a")]);
        let report = apply_cycle(&mut shadow, &cfg, &slow).unwrap();
        assert_eq!(report.configured_every_ms, 3000);
        assert_eq!(report.actual_wait_ms, 0);
        assert!(report.reconcile);
        let mut outside = input("slow", 9_000, vec![doc("led-01", "ledger-open", 1, "a")]);
        outside.now_minute = 3 * 60;
        let mut closed = cfg.clone();
        closed.window = "04:00-04:30".into();
        assert_eq!(apply_cycle(&mut shadow, &closed, &outside).unwrap_err(), "outside_window");
    }

    #[test]
    fn insert_update_lagged_delete_and_tombstone_does_not_resurrect() {
        let cfg = config();
        let mut shadow = Shadow::default();
        let load = apply_cycle(&mut shadow, &cfg, &input("fast", 0, vec![doc("led-01", "ledger-open", 1, "a")])).unwrap();
        assert_eq!(load.inserted, vec!["ledger-open".to_string()]);
        assert_eq!(load.quality, "DELETES_LAGGED");
        assert!(!load.reconcile);
        let caught = apply_cycle(
            &mut shadow,
            &cfg,
            &input("fast", 1_500, vec![doc("led-01", "ledger-open", 1, "a")]),
        )
        .unwrap();
        assert!(caught.reconcile);
        assert_eq!(caught.quality, "caught_up");
        assert_eq!(caught.typed_update_count, 0);
        let inserted = apply_cycle(
            &mut shadow,
            &cfg,
            &input("fast", 3_000, vec![doc("led-01", "ledger-open", 1, "a"), doc("led-c5", "ledger-c5", 4, "insert")]),
        )
        .unwrap();
        assert_eq!(inserted.inserted, vec!["ledger-c5".to_string()]);
        assert_eq!(inserted.quality, "DELETES_LAGGED");
        let updated = apply_cycle(
            &mut shadow,
            &cfg,
            &input("fast", 4_500, vec![doc("led-01", "ledger-open", 1, "a"), doc("led-c5", "ledger-c5", 5, "update")]),
        )
        .unwrap();
        assert_eq!(updated.updated, vec!["ledger-c5".to_string()]);
        assert!(updated.reconcile);
        assert_eq!(updated.quality, "caught_up");
        let lagged = apply_cycle(&mut shadow, &cfg, &input("fast", 6_000, vec![doc("led-01", "ledger-open", 1, "a")])).unwrap();
        assert_eq!(lagged.suspected_deletes, vec!["ledger-c5".to_string()]);
        assert!(lagged.tombstoned.is_empty());
        assert!(!shadow.rows["ledger-c5"].tombstone);
        assert_eq!(lagged.quality, "DELETES_LAGGED");
        let gone = apply_cycle(&mut shadow, &cfg, &input("fast", 7_500, vec![doc("led-01", "ledger-open", 1, "a")])).unwrap();
        assert_eq!(gone.tombstoned, vec!["ledger-c5".to_string()]);
        assert!(shadow.rows["ledger-c5"].tombstone);
        assert_eq!(gone.live_count, 1);
        let resurrect = apply_cycle(
            &mut shadow,
            &cfg,
            &input("fast", 9_000, vec![doc("led-01", "ledger-open", 1, "a"), doc("led-c5", "ledger-c5", 9, "back")]),
        )
        .unwrap();
        assert_eq!(resurrect.resurrect_refused, vec!["ledger-c5".to_string()]);
        assert!(shadow.rows["ledger-c5"].tombstone);
        assert!(!shadow.rows["ledger-c5"].canonical.contains("back"));
        let again = apply_cycle(&mut shadow, &cfg, &input("fast", 10_500, vec![doc("led-01", "ledger-open", 1, "a")])).unwrap();
        assert!(again.inserted.is_empty());
        assert_eq!(again.typed_update_count, 0);
        assert!(again.tombstoned.is_empty());
    }

    #[test]
    fn partial_expiry_schema_budget_and_cursor_do_not_repair() {
        let cfg = config();
        let mut shadow = Shadow::default();
        apply_cycle(&mut shadow, &cfg, &input("fast", 0, vec![doc("led-01", "ledger-open", 1, "a")])).unwrap();
        let watermark = shadow.watermark;
        let mut partial = input("fast", 1_500, vec![doc("led-01", "ledger-open", 2, "changed")]);
        partial.kind = CycleKind::Partial;
        let report = apply_cycle(&mut shadow, &cfg, &partial).unwrap();
        assert!(report.discarded);
        assert_eq!(report.reason, "partial_snapshot_discarded");
        assert_eq!(shadow.watermark, watermark);
        assert!(shadow.rows["ledger-open"].canonical.contains("\"note\":\"a\""));
        let mut expired = input("fast", 3_000, Vec::new());
        expired.kind = CycleKind::Expired;
        let report = apply_cycle(&mut shadow, &cfg, &expired).unwrap();
        assert!(report.position_expired);
        assert_eq!(shadow.watermark, watermark);
        let mut broken = input("fast", 4_500, vec![doc("led-01", "ledger-open", 3, "x")]);
        broken.mapping_types.insert("long_value".into(), "text".into());
        let report = apply_cycle(&mut shadow, &cfg, &broken).unwrap();
        assert!(report.schema_break);
        assert_eq!(report.publication, "paused");
        assert_eq!(shadow.watermark, watermark);
        let mut relation = input("fast", 6_000, vec![doc("led-01", "ledger-open", 1, "a")]);
        relation.typed_total_relation = "gte".into();
        let report = apply_cycle(&mut shadow, &cfg, &relation).unwrap();
        assert!(report.reason.contains("gte"));
        assert_eq!(shadow.watermark, watermark);
        let mut cursor = input("fast", 7_500, vec![doc("led-01", "ledger-open", 1, "a")]);
        cursor.resume_cursor = Some(format!("c5-gen-{watermark}"));
        let report = apply_cycle(&mut shadow, &cfg, &cursor).unwrap();
        assert!(!report.continuation);
        assert!(!report.cursor_repaired);
        assert!(!report.resume_cursor_sent_to_source);
        assert_eq!(report.reason, "full_reread_cursor_not_used");
        assert!(report.watermark_after > watermark);
        let mut tiny = cfg.clone();
        tiny.budget_rows = 1;
        let mut budget = input("fast", 9_000, vec![doc("led-01", "ledger-open", 1, "a"), doc("led-02", "ledger-post", 2, "b")]);
        budget.now_ms = 9_000;
        let before = shadow.watermark;
        let report = apply_cycle(&mut shadow, &tiny, &budget).unwrap();
        assert_eq!(report.reason, "budget_exceeded");
        assert_eq!(shadow.watermark, before);
    }

    #[test]
    fn object_without_type_is_not_a_break_and_a_real_type_change_is() {
        let mut cfg = config();
        cfg.approved_fields.push("thing".into());
        cfg.field_types.insert("thing".into(), "object".into());
        let mapping = json!({
            "dynamic": "strict",
            "properties": {
                "doc_key": {"type": "keyword"},
                "long_value": {"type": "long"},
                "note": {"type": "text"},
                "thing": {"dynamic": "strict", "properties": {"label": {"type": "keyword"}, "n": {"type": "long"}}},
                "nested_only": {"type": "nested", "properties": {"a": {"type": "keyword"}}}
            }
        });
        let types = mapping_types_of(&mapping);
        assert_eq!(types.get("thing").map(String::as_str), Some("object"));
        assert_eq!(types.get("nested_only").map(String::as_str), Some("nested"));
        assert!(!schema_broken(&cfg, "strict", &types));
        let mut changed = types.clone();
        changed.insert("long_value".into(), "text".into());
        assert!(schema_broken(&cfg, "strict", &changed));
        assert!(schema_broken(&cfg, "true", &types));
    }

    #[test]
    fn new_field_stays_out_of_the_typed_canonical() {
        let cfg = config();
        let mut shadow = Shadow::default();
        let mut with_extra = doc("led-01", "ledger-open", 1, "a");
        with_extra.source.as_object_mut().unwrap().insert("c5_extra".into(), json!("hidden"));
        let report = apply_cycle(&mut shadow, &cfg, &input("fast", 0, vec![with_extra])).unwrap();
        assert_eq!(report.inserted.len(), 1);
        let row = &shadow.rows["ledger-open"];
        assert_eq!(row.unclassified, vec!["c5_extra".to_string()]);
        assert!(!row.canonical.contains("c5_extra"));
        assert!(!row.canonical.contains("hidden"));
        let mut typed_same = doc("led-01", "ledger-open", 1, "a");
        typed_same.source.as_object_mut().unwrap().insert("c5_extra".into(), json!("other"));
        let report = apply_cycle(&mut shadow, &cfg, &input("fast", 1_500, vec![typed_same])).unwrap();
        assert_eq!(report.typed_update_count, 0);
        assert_eq!(report.unclassified_updates, vec!["ledger-open".to_string()]);
    }

    #[test]
    fn canonical_cell_diff_and_sql_cover_every_key() {
        let cfg = config();
        let object = json!({"note": "a", "doc_key": "ledger-open", "long_value": -9223372036854775808_i64, "where_point": {"lon": 4, "lat": 3}});
        let text = canonical_text(&cfg.approved_fields, object.as_object().unwrap());
        assert_eq!(text, "{\"doc_key\":\"ledger-open\",\"long_value\":-9223372036854775808,\"note\":\"a\"}");
        let mut source = BTreeMap::new();
        source.insert("ledger-open".into(), text.clone());
        let mut pg = source.clone();
        assert!(full_cell_diff(&source, &pg).is_empty());
        pg.insert("extra".into(), "{}".into());
        assert_eq!(full_cell_diff(&source, &pg).len(), 1);
        let row = LiveRow {
            key: "ledger-open".into(),
            doc_id: "led-01".into(),
            canonical: text,
            hash: "abc".into(),
            source_json: "{}".into(),
            unclassified: vec!["where_point".into()],
            tombstone: true,
            generation: 4,
        };
        let sql = mutation_sql("ledger_rows", &[row]).unwrap();
        assert!(sql.contains("WHERE NOT tombstone"));
        assert!(sql.contains("tombstone = true"));
        assert!(sql.contains("OR EXCLUDED.tombstone = true"));
        assert!(!is_source_mutation("POST", "/oo/_search"));
        assert!(!is_source_mutation("DELETE", "/_search/point_in_time"));
        assert!(is_source_mutation("PUT", "/oo/_doc/led-01"));
        assert!(is_source_mutation("POST", "/oo/_update/led-01"));
        assert!(is_source_mutation("DELETE", "/oo/_doc/led-01"));
        assert!(is_source_mutation("PUT", "/oo/_mapping"));
    }

    #[test]
    fn wrapped_window_and_equal_hashes_are_stable() {
        assert!(window_contains("20:00-08:00", 21 * 60).unwrap());
        assert!(window_contains("20:00-08:00", 7 * 60).unwrap());
        assert!(!window_contains("20:00-08:00", 12 * 60).unwrap());
        assert!(window_contains("bad", 0).is_err());
        let cfg = config();
        let docs = vec![doc("led-01", "ledger-open", 1, "")];
        let once = project_docs(&cfg, &docs).unwrap();
        let twice = project_docs(&cfg, &docs).unwrap();
        assert_eq!(once[0].hash, twice[0].hash);
        assert!(once[0].canonical.contains("\"note\":\"\""));
    }
}
