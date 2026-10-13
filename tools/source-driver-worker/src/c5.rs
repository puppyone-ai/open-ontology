//! Oracle C5 full-diff shadow. Not log CDC and not the native loader.
//!
//! Periods omitted from config resolve to the docs daily value. A committed
//! generation is an atomic rename. An expired position is not a resume cursor.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

pub const DOCS_DAILY_SECONDS: u64 = 86_400;
pub const METHOD: &str = "C5";
pub const INITIAL_LOAD: &str = "shadow_diff";
pub const DELETE_DETECTION: &str = "reconcile";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Periods {
    pub detect_period_seconds: u64,
    pub pull_period_seconds: u64,
    pub reconcile_period_seconds: u64,
    pub delete_visibility_sla_seconds: u64,
    pub origin: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitStep {
    pub name: &'static str,
    pub configured_seconds: u64,
    pub elapsed_seconds: u64,
}

pub fn resolve_periods(raw: &Map<String, Value>) -> Result<Periods, &'static str> {
    let keys = [
        "detect_period_seconds",
        "pull_period_seconds",
        "reconcile_period_seconds",
        "delete_visibility_sla_seconds",
    ];
    if raw.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err("unknown_period");
    }
    let mut defaults = 0u8;
    let mut values = [0u64; 4];
    for (index, key) in keys.iter().enumerate() {
        match raw.get(*key) {
            None | Some(Value::Null) => {
                values[index] = DOCS_DAILY_SECONDS;
                defaults += 1;
            }
            Some(Value::Number(number)) => {
                let Some(value) = number.as_u64() else {
                    return Err("period");
                };
                if value < 1 {
                    return Err("period");
                }
                values[index] = value;
            }
            _ => return Err("period"),
        }
    }
    if values[2] > values[3] {
        return Err("delete_sla_unmet");
    }
    let origin = if defaults == 4 {
        "docs_default"
    } else if defaults == 0 {
        "configured"
    } else {
        "mixed"
    };
    Ok(Periods {
        detect_period_seconds: values[0],
        pull_period_seconds: values[1],
        reconcile_period_seconds: values[2],
        delete_visibility_sla_seconds: values[3],
        origin,
    })
}

pub fn wait_periods<F>(periods: &Periods, mut sleeper: F) -> Result<Vec<WaitStep>, &'static str>
where
    F: FnMut(u64) -> u64,
{
    let plan = [
        ("detect", periods.detect_period_seconds),
        ("pull", periods.pull_period_seconds),
        ("reconcile", periods.reconcile_period_seconds),
    ];
    let mut steps = Vec::with_capacity(3);
    for (name, configured) in plan {
        let elapsed = sleeper(configured);
        if elapsed < configured {
            return Err("short_wait");
        }
        steps.push(WaitStep {
            name,
            configured_seconds: configured,
            elapsed_seconds: elapsed,
        });
    }
    Ok(steps)
}

pub fn wait_periods_realtime(periods: &Periods) -> Result<Vec<WaitStep>, &'static str> {
    wait_periods(periods, |seconds| {
        let started = Instant::now();
        thread::sleep(Duration::from_secs(seconds));
        started.elapsed().as_secs()
    })
}

pub fn canonical_json(cells: &Map<String, Value>) -> Result<String, &'static str> {
    for (key, value) in cells {
        if !key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return Err("cell_key");
        }
        match value {
            Value::Null | Value::String(_) => {}
            Value::Number(number) if number.as_i64().is_some() => {}
            _ => return Err("cell_value"),
        }
    }
    let mut ordered = Map::new();
    for (key, value) in cells {
        ordered.insert(key.clone(), value.clone());
    }
    serde_json::to_string(&Value::Object(ordered)).map_err(|_| "cell_json")
}

pub fn row_hash(cells: &Map<String, Value>) -> Result<String, &'static str> {
    let text = canonical_json(cells)?;
    let digest = Sha256::digest(text.as_bytes());
    Ok(hex(&digest))
}

pub fn column_flags(data_type: &str, precision: Option<i32>, scale: Option<i32>) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if matches!(
        data_type,
        "CHAR" | "VARCHAR2" | "NCHAR" | "NVARCHAR2" | "CLOB" | "NCLOB"
    ) {
        flags.push("empty_string_is_null");
    }
    if data_type == "NUMBER" && precision.is_none() {
        flags.push("unconstrained_number");
    }
    if data_type.starts_with("TIMESTAMP") && scale.unwrap_or(0) > 6 {
        flags.push("precision_loss");
    }
    flags
}

pub fn normalize_text_cell(data_type: &str, value: Option<String>) -> Option<String> {
    if matches!(
        data_type,
        "CHAR" | "VARCHAR2" | "NCHAR" | "NVARCHAR2" | "CLOB" | "NCLOB"
    ) {
        return match value {
            Some(text) if text.is_empty() => None,
            other => other,
        };
    }
    value
}

pub fn consistency_grade(snapshot_token: bool, source_changed_without_token: bool) -> &'static str {
    if source_changed_without_token || !snapshot_token {
        "B"
    } else {
        "S0"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEvent {
    pub op: &'static str,
    pub key: String,
}

pub fn diff_hashes(previous: &BTreeMap<String, String>, next: &BTreeMap<String, String>) -> Vec<DiffEvent> {
    let mut keys = BTreeMap::<String, ()>::new();
    for key in previous.keys().chain(next.keys()) {
        keys.insert(key.clone(), ());
    }
    let mut events = Vec::new();
    for key in keys.into_keys() {
        match (previous.get(&key), next.get(&key)) {
            (None, Some(_)) => events.push(DiffEvent {
                op: "insert",
                key,
            }),
            (Some(_), None) => events.push(DiffEvent {
                op: "delete",
                key,
            }),
            (Some(old), Some(new)) if old != new => events.push(DiffEvent {
                op: "update",
                key,
            }),
            _ => {}
        }
    }
    events
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    pub generation: u64,
    pub position: String,
    pub hashes: BTreeMap<String, String>,
    pub columns: Vec<String>,
    pub tombstones: Vec<String>,
}

fn generation_path(dir: &Path, generation: u64) -> PathBuf {
    dir.join(format!("gen-{generation}.json"))
}

fn staging_path(dir: &Path, generation: u64) -> PathBuf {
    dir.join(format!("gen-{generation}.staging.json"))
}

fn write_json(path: &Path, value: &Value) -> Result<(), &'static str> {
    let text = serde_json::to_vec_pretty(value).map_err(|_| "store")?;
    let mut file = File::create(path).map_err(|_| "store")?;
    file.write_all(&text).map_err(|_| "store")?;
    file.sync_all().map_err(|_| "store")?;
    Ok(())
}

pub fn load_generation(dir: &Path, generation: u64) -> Result<Generation, &'static str> {
    let bytes = fs::read(generation_path(dir, generation)).map_err(|_| "missing_generation")?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "store")?;
    parse_generation(&value)
}

fn parse_generation(value: &Value) -> Result<Generation, &'static str> {
    let object = value.as_object().ok_or("store")?;
    let generation = object.get("generation").and_then(Value::as_u64).ok_or("store")?;
    let position = object
        .get("position")
        .and_then(Value::as_str)
        .ok_or("store")?
        .to_owned();
    let mut hashes = BTreeMap::new();
    for (key, hash) in object.get("hashes").and_then(Value::as_object).ok_or("store")? {
        hashes.insert(key.clone(), hash.as_str().ok_or("store")?.to_owned());
    }
    let columns = object
        .get("columns")
        .and_then(Value::as_array)
        .ok_or("store")?
        .iter()
        .map(|item| item.as_str().ok_or("store").map(str::to_owned))
        .collect::<Result<Vec<_>, _>>()?;
    let tombstones = object
        .get("tombstones")
        .and_then(Value::as_array)
        .ok_or("store")?
        .iter()
        .map(|item| item.as_str().ok_or("store").map(str::to_owned))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Generation {
        generation,
        position,
        hashes,
        columns,
        tombstones,
    })
}

#[derive(Debug)]
pub struct Apply {
    pub committed: bool,
    pub state: &'static str,
    pub generation: Option<u64>,
    pub events: Vec<DiffEvent>,
    pub resumed: bool,
}

#[allow(
    clippy::too_many_arguments,
    reason = "one snapshot call carries the previous generation and the new read"
)]
pub fn apply_snapshot(
    dir: &Path,
    previous: Option<&Generation>,
    generation: u64,
    position: &str,
    columns: &[String],
    approved: &[String],
    hashes: &BTreeMap<String, String>,
    volume_limit: u64,
    interrupt: bool,
    stale_upsert_generation: Option<u64>,
) -> Result<Apply, &'static str> {
    if hashes.len() as u64 > volume_limit {
        return Err("volume_exceeds_limit");
    }
    if columns != approved {
        return Ok(Apply {
            committed: false,
            state: "paused_schema_change",
            generation: previous.map(|item| item.generation),
            events: Vec::new(),
            resumed: false,
        });
    }
    let empty = BTreeMap::new();
    let prior = previous.map(|item| &item.hashes).unwrap_or(&empty);
    let events = diff_hashes(prior, hashes);
    let mut tombstones = previous.map(|item| item.tombstones.clone()).unwrap_or_default();
    if let Some(stale) = stale_upsert_generation {
        let resurrect = events.iter().any(|event| {
            event.op == "insert"
                && tombstones.iter().any(|item| {
                    item.rsplit_once(':').is_some_and(|(key, fence)| {
                        key == event.key && fence.parse::<u64>().ok().is_some_and(|fence| stale <= fence)
                    })
                })
        });
        if resurrect {
            return Err("stale_upsert");
        }
    }
    for event in &events {
        if event.op == "delete" {
            tombstones.push(format!("{}:{generation}", event.key));
        }
    }
    let body = serde_json::json!({
        "generation": generation,
        "position": position,
        "columns": columns,
        "hashes": hashes,
        "tombstones": tombstones,
        "resumed": false,
        "method": METHOD,
        "initial_load": INITIAL_LOAD,
        "delete_detection": DELETE_DETECTION,
    });
    fs::create_dir_all(dir).map_err(|_| "store")?;
    let staging = staging_path(dir, generation);
    write_json(&staging, &body)?;
    if interrupt {
        fs::remove_file(&staging).map_err(|_| "store")?;
        return Ok(Apply {
            committed: false,
            state: "interrupted",
            generation: previous.map(|item| item.generation),
            events,
            resumed: false,
        });
    }
    let final_path = generation_path(dir, generation);
    fs::rename(&staging, &final_path).map_err(|_| "store")?;
    Ok(Apply {
        committed: true,
        state: "committed",
        generation: Some(generation),
        events,
        resumed: false,
    })
}

pub fn position_state(age_seconds: u64, retention_seconds: u64) -> &'static str {
    if retention_seconds == 0 || age_seconds > retention_seconds {
        "POSITION_EXPIRED"
    } else if age_seconds * 10 > retention_seconds * 8 {
        "retention_pause"
    } else if age_seconds * 2 > retention_seconds {
        "STALE_RISK"
    } else {
        "ready"
    }
}

pub fn resume_expired() -> Result<(), &'static str> {
    Err("resume_forbidden")
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn number_u64(value: u64) -> Value {
    Value::Number(Number::from(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn cells(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn omitted_periods_are_the_docs_daily_value() {
        let periods = resolve_periods(&Map::new()).expect("default");
        assert_eq!(periods.origin, "docs_default");
        assert_eq!(periods.detect_period_seconds, 86_400);
        assert_eq!(periods.pull_period_seconds, 86_400);
        assert_eq!(periods.reconcile_period_seconds, 86_400);
        assert_eq!(periods.delete_visibility_sla_seconds, 86_400);
    }

    #[test]
    fn reconcile_longer_than_the_sla_is_refused() {
        let mut raw = Map::new();
        raw.insert("reconcile_period_seconds".to_owned(), number_u64(20));
        raw.insert("delete_visibility_sla_seconds".to_owned(), number_u64(10));
        assert_eq!(resolve_periods(&raw).unwrap_err(), "delete_sla_unmet");
    }

    #[test]
    fn configured_wait_calls_each_period() {
        let mut raw = Map::new();
        raw.insert("detect_period_seconds".to_owned(), number_u64(2));
        raw.insert("pull_period_seconds".to_owned(), number_u64(3));
        raw.insert("reconcile_period_seconds".to_owned(), number_u64(4));
        raw.insert("delete_visibility_sla_seconds".to_owned(), number_u64(86_400));
        let periods = resolve_periods(&raw).expect("configured");
        assert_eq!(periods.origin, "configured");
        let mut seen = Vec::new();
        let steps = wait_periods(&periods, |seconds| {
            seen.push(seconds);
            seconds
        })
        .expect("wait");
        assert_eq!(seen, vec![2, 3, 4]);
        assert_eq!(steps[2].name, "reconcile");
    }

    #[test]
    fn realtime_wait_covers_one_second_periods() {
        let mut raw = Map::new();
        raw.insert("detect_period_seconds".to_owned(), number_u64(1));
        raw.insert("pull_period_seconds".to_owned(), number_u64(1));
        raw.insert("reconcile_period_seconds".to_owned(), number_u64(1));
        raw.insert("delete_visibility_sla_seconds".to_owned(), number_u64(86_400));
        let periods = resolve_periods(&raw).expect("short");
        let started = Instant::now();
        let steps = wait_periods_realtime(&periods).expect("slept");
        assert!(started.elapsed() >= Duration::from_secs(3));
        assert!(steps.iter().all(|step| step.elapsed_seconds >= 1));
    }

    #[test]
    fn diff_covers_insert_update_delete_and_replay() {
        let mut previous = BTreeMap::new();
        previous.insert("41".to_owned(), "a".to_owned());
        previous.insert("42".to_owned(), "b".to_owned());
        let mut next = BTreeMap::new();
        next.insert("41".to_owned(), "c".to_owned());
        next.insert("91".to_owned(), "d".to_owned());
        let events = diff_hashes(&previous, &next);
        assert_eq!(events[0].op, "update");
        assert_eq!(events[1].op, "delete");
        assert_eq!(events[2].op, "insert");
        assert!(diff_hashes(&next, &next).is_empty());
    }

    #[test]
    fn interrupt_leaves_no_committed_generation() {
        let dir = std::env::temp_dir().join(format!("oo04-c5-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let columns = vec!["ID".to_owned()];
        let mut hashes = BTreeMap::new();
        hashes.insert("41".to_owned(), "abc".to_owned());
        let applied = apply_snapshot(
            &dir, None, 1, "scn-1", &columns, &columns, &hashes, 8, true, None,
        )
        .expect("apply");
        assert!(!applied.committed);
        assert_eq!(applied.state, "interrupted");
        assert!(!generation_path(&dir, 1).exists());
        assert!(!staging_path(&dir, 1).exists());
        let committed = apply_snapshot(
            &dir, None, 1, "scn-1", &columns, &columns, &hashes, 8, false, None,
        )
        .expect("commit");
        assert!(committed.committed);
        assert!(!committed.resumed);
        let again = apply_snapshot(
            &dir,
            Some(&load_generation(&dir, 1).expect("load")),
            2,
            "scn-1",
            &columns,
            &columns,
            &hashes,
            8,
            false,
            None,
        )
        .expect("replay");
        assert!(again.events.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn schema_pause_and_expired_position_do_not_resume() {
        let dir = std::env::temp_dir().join(format!("oo04-c5-schema-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let approved = vec!["ID".to_owned()];
        let observed = vec!["ID".to_owned(), "C5_NOTE".to_owned()];
        let hashes = BTreeMap::new();
        let paused = apply_snapshot(
            &dir, None, 1, "scn-1", &observed, &approved, &hashes, 8, false, None,
        )
        .expect("pause");
        assert_eq!(paused.state, "paused_schema_change");
        assert!(!generation_path(&dir, 1).exists());
        assert_eq!(position_state(2, 1), "POSITION_EXPIRED");
        assert_eq!(resume_expired().unwrap_err(), "resume_forbidden");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn flags_grade_and_empty_oracle_text() {
        assert_eq!(
            column_flags("VARCHAR2", None, None),
            vec!["empty_string_is_null"]
        );
        assert_eq!(
            column_flags("NUMBER", None, None),
            vec!["unconstrained_number"]
        );
        assert_eq!(column_flags("TIMESTAMP", None, Some(9)), vec!["precision_loss"]);
        assert!(column_flags("DATE", None, None).is_empty());
        assert!(normalize_text_cell("VARCHAR2", Some(String::new())).is_none());
        assert_eq!(
            normalize_text_cell("VARCHAR2", Some("ZQ".to_owned())).as_deref(),
            Some("ZQ")
        );
        assert_eq!(consistency_grade(true, false), "S0");
        assert_eq!(consistency_grade(false, false), "B");
        assert_eq!(consistency_grade(true, true), "B");
        assert_ne!(METHOD, "C1");
        assert_ne!(INITIAL_LOAD, "log_cdc");
    }

    #[test]
    fn canonical_hash_is_stable_for_ascii_cells() {
        let map = cells(&[
            ("ID", Value::from(41)),
            ("V2", Value::from("a b")),
            ("N16", Value::from("1.250000")),
            ("C6", Value::Null),
        ]);
        let text = canonical_json(&map).expect("json");
        assert_eq!(text, r#"{"C6":null,"ID":41,"N16":"1.250000","V2":"a b"}"#);
        let hash = row_hash(&map).expect("hash");
        assert_eq!(hash.len(), 64);
        assert_eq!(row_hash(&map).expect("again"), hash);
    }

    #[test]
    fn a_deleted_key_rejects_an_older_upsert() {
        let dir = std::env::temp_dir().join(format!("oo04-c5-stale-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let columns = vec!["ID".to_owned()];
        let mut with_row = BTreeMap::new();
        with_row.insert("91".to_owned(), "one".to_owned());
        apply_snapshot(&dir, None, 1, "scn-1", &columns, &columns, &with_row, 8, false, None).expect("insert");
        let gone = BTreeMap::new();
        let deleted = apply_snapshot(
            &dir,
            Some(&load_generation(&dir, 1).expect("gen1")),
            2,
            "scn-2",
            &columns,
            &columns,
            &gone,
            8,
            false,
            None,
        )
        .expect("delete");
        assert_eq!(deleted.events[0].op, "delete");
        let error = apply_snapshot(
            &dir,
            Some(&load_generation(&dir, 2).expect("gen2")),
            3,
            "scn-1",
            &columns,
            &columns,
            &with_row,
            8,
            false,
            Some(1),
        );
        assert_eq!(error.unwrap_err(), "stale_upsert");
        assert!(!generation_path(&dir, 3).exists());
        let fresh = apply_snapshot(
            &dir,
            Some(&load_generation(&dir, 2).expect("gen2b")),
            3,
            "scn-3",
            &columns,
            &columns,
            &with_row,
            8,
            false,
            Some(3),
        )
        .expect("reinsert");
        assert_eq!(fresh.events[0].op, "insert");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_limit_refuses_before_commit() {
        let dir = std::env::temp_dir().join(format!("oo04-c5-vol-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let columns = vec!["ID".to_owned()];
        let mut hashes = BTreeMap::new();
        hashes.insert("1".to_owned(), "a".to_owned());
        hashes.insert("2".to_owned(), "b".to_owned());
        let error = apply_snapshot(&dir, None, 1, "scn", &columns, &columns, &hashes, 1, false, None);
        assert_eq!(error.unwrap_err(), "volume_exceeds_limit");
        assert!(!generation_path(&dir, 1).exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
