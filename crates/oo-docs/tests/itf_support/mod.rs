//! Readers for the ITF JSON that `quint run --out-itf` writes (80 section 4):
//! integers are numbers or `{"#bigint": "n"}`, sets `{"#set": [...]}`, maps
//! `{"#map": [[k, v], ...]}`, records plain objects. Shared by the FM5, FM6,
//! FM8, FM10 and FM11 conformance tests (`#[path]` module, not a test target).
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::Value;

pub fn int(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n.as_i64().expect("int"),
        Value::Object(o) => o["#bigint"]
            .as_str()
            .expect("bigint")
            .parse()
            .expect("bigint"),
        other => panic!("not an int: {other}"),
    }
}

pub fn set(v: &Value) -> &Vec<Value> {
    v["#set"]
        .as_array()
        .unwrap_or_else(|| panic!("not a set: {v}"))
}

pub fn set_ints(v: &Value) -> BTreeSet<i64> {
    set(v).iter().map(int).collect()
}

pub fn map(v: &Value) -> &Vec<Value> {
    v["#map"]
        .as_array()
        .unwrap_or_else(|| panic!("not a map: {v}"))
}

pub fn map_ints(v: &Value) -> BTreeMap<i64, i64> {
    map(v).iter().map(|p| (int(&p[0]), int(&p[1]))).collect()
}

pub fn map_bools(v: &Value) -> BTreeMap<i64, bool> {
    map(v)
        .iter()
        .map(|p| (int(&p[0]), p[1].as_bool().expect("bool")))
        .collect()
}

pub fn map_sets(v: &Value) -> BTreeMap<i64, BTreeSet<i64>> {
    map(v)
        .iter()
        .map(|p| (int(&p[0]), set_ints(&p[1])))
        .collect()
}

pub fn map_values(v: &Value) -> BTreeMap<i64, &Value> {
    map(v).iter().map(|p| (int(&p[0]), &p[1])).collect()
}

pub fn text(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}

/// Traces of one model: the committed samples of `formal/traces/<model>` plus,
/// when `env_dir` is set, a directory of bulk traces (`formal/scripts/formal-traces`).
pub fn load_traces(model: &str, env_dir: &str) -> Vec<(String, Value)> {
    let dir = std::env::var(env_dir)
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../formal/traces")
                .join(model)
        });
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".itf.json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no traces in {}", dir.display());
    files
        .into_iter()
        .map(|p| {
            let t: Value = serde_json::from_slice(&std::fs::read(&p).expect("read"))
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            (p.file_name().unwrap().to_string_lossy().into_owned(), t)
        })
        .collect()
}

/// The action a state was reached by (`--mbt` metadata), `None` for the initial state.
pub fn action_of(state: &Value) -> Option<&str> {
    state["mbt::actionTaken"].as_str().filter(|a| *a != "init")
}
