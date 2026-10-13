//! Independent oracle for one OpenSearch preflight directory.
//! It does not link the reader crate.

#![recursion_limit = "256"]
#![allow(clippy::uninlined_format_args, clippy::redundant_closure_for_method_calls)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("os-judge: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<u8, String> {
    let args = args()?;
    let mut failures = Vec::new();
    if args.get("mode").map(String::as_str) == Some("cancel") {
        check_phase("cancel", required(&args, "cancel")?, cancel_ok, &mut failures)?;
        check_phase("cancel-late", required(&args, "cancel-late")?, late_ok, &mut failures)?;
        check_phase("deadline", required(&args, "deadline")?, deadline_ok, &mut failures)?;
        let verdict = json!({"passed": failures.is_empty(), "failures": failures,
            "scope": "changed native cancellation gates; unchanged whole-source evidence separately frozen",
            "independent_approval": false});
        fs::write(required(&args, "out")?, serde_json::to_string_pretty(&verdict).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        return Ok(if failures.is_empty() { 0 } else { 2 });
    }
    let fixture = read_json(required(&args, "fixture")?)?;
    let mapping = read_json(required(&args, "mapping")?)?;
    let decoded = read_json(required(&args, "decoded")?)?;
    let read_dir = Path::new(required(&args, "read-dir")?);
    let result = read_json(&read_dir.join("result.json").to_string_lossy())?;
    let catalog = read_json(&read_dir.join("catalog.json").to_string_lossy())?;
    let keys = read_json(&read_dir.join("keys.json").to_string_lossy())?;
    check_identity(&fixture, &mut failures);
    check_catalog(&catalog, &mut failures);
    check_result(&result, &fixture, &mut failures);
    check_arrow(read_dir, &result, &mut failures)?;
    check_rows(&fixture, &mapping, &decoded, &keys, read_dir, &mut failures)?;
    check_schema(&decoded, &fixture, &mut failures);
    check_phase("visibility", required(&args, "visibility")?, visibility_ok, &mut failures)?;
    reconcile_ok(&read_json(required(&args, "reconcile")?)?, &fixture, &mut failures);
    check_phase("negative", required(&args, "negative")?, negative_ok, &mut failures)?;
    check_phase("shape", required(&args, "shape")?, shape_ok, &mut failures)?;
    check_phase("cancel", required(&args, "cancel")?, cancel_ok, &mut failures)?;
    check_phase("cancel-late", required(&args, "cancel-late")?, late_ok, &mut failures)?;
    check_phase("deadline", required(&args, "deadline")?, deadline_ok, &mut failures)?;
    check_phase("rowcap", required(&args, "rowcap")?, rowcap_ok, &mut failures)?;
    check_phase("bytecap", required(&args, "bytecap")?, bytecap_ok, &mut failures)?;
    check_phase("setup", required(&args, "setup")?, setup_ok, &mut failures)?;
    check_phase("stats", required(&args, "stats")?, stats_ok, &mut failures)?;
    check_phase("server-audit", required(&args, "server-audit")?, server_audit_ok, &mut failures)?;
    check_phase("block-cast", required(&args, "block-cast")?, block_cast_ok, &mut failures)?;
    check_audits(Path::new(required(&args, "audit-dir")?), &mut failures)?;
    let verdict = json!({
        "failures": failures,
        "passed": failures.is_empty(),
        "independent_approval": false,
        "author_complete_review_deferred": true,
    });
    let out = required(&args, "out")?;
    if let Some(parent) = Path::new(out).parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(out, serde_json::to_string_pretty(&verdict).map_err(|error| error.to_string())? + "\n")
        .map_err(|error| error.to_string())?;
    Ok(if failures.is_empty() { 0 } else { 2 })
}

fn check_identity(fixture: &Value, failures: &mut Vec<String>) {
    let docs = fixture.get("documents").and_then(Value::as_array);
    let Some(docs) = docs else {
        failures.push("fixture documents missing".into());
        return;
    };
    if docs.is_empty() || docs.len() > 5000 {
        failures.push(format!("fixture document count {}", docs.len()));
    }
    let mut ids = BTreeSet::new();
    for doc in docs {
        let id = doc.get("_id").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() || id.starts_with("d0") || id == "d01" || !ids.insert(id.to_string()) {
            failures.push(format!("fixture id {id}"));
        }
    }
}

fn check_catalog(catalog: &Value, failures: &mut Vec<String>) {
    expect(catalog, "protocol", "oo.source.opensearch.batch/v1", failures);
    expect_bool(catalog, "central_source_catalog_api", false, failures);
    expect_bool(catalog, "s1_claimed", false, failures);
    expect(catalog, "consistency_grade", "S0", failures);
    expect_bool(catalog, "joins", false, failures);
    expect_bool(catalog, "foreign_keys", false, failures);
    expect_bool(catalog, "multi_document_atomicity", false, failures);
    expect(catalog, "capture_method", "C5", failures);
    expect(catalog, "delete_detection", "keyset_reconcile", failures);
    expect(catalog, "family_profile_without_pit", "B", failures);
    let unsupported = catalog.get("unsupported").and_then(Value::as_array).cloned().unwrap_or_default();
    let geo = unsupported.iter().any(|item| {
        item.get("field").and_then(Value::as_str) == Some("where_point")
            && item.get("os_type").and_then(Value::as_str) == Some("geo_point")
    });
    if !geo {
        failures.push("catalog does not list where_point geo_point as unsupported".into());
    }
    let text = catalog.to_string();
    if text.contains("oo.source.pgsql") || text.contains("postgresql") {
        failures.push("catalog mentions postgresql protocol".into());
    }
}

fn check_result(result: &Value, fixture: &Value, failures: &mut Vec<String>) {
    let expected = fixture.get("documents").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    let expected_rows = u64::try_from(expected).unwrap_or(0);
    expect(result, "outcome", "ok", failures);
    expect_u64(result, "rows", expected_rows, failures);
    expect_u64(result, "total", expected_rows, failures);
    expect(result, "relation", "eq", failures);
    expect_bool(result, "ipc_present", true, failures);
    expect_bool(result, "pit_kept", true, failures);
    expect_u64(result, "business_clients", 1, failures);
    expect_u64(result, "pool_max_idle_per_host", 1, failures);
    expect(result, "http", "http1", failures);
    let pages = result.get("pages").and_then(Value::as_array).cloned().unwrap_or_default();
    let widths: Vec<u64> = pages.iter().filter_map(Value::as_u64).collect();
    if !pages_cover(&widths, expected_rows) {
        failures.push(format!("pages {widths:?}"));
    }
}

fn pages_cover(pages: &[u64], rows: u64) -> bool {
    if pages.is_empty() || pages.iter().sum::<u64>() != rows {
        return false;
    }
    let width = pages[0];
    width > 0 && pages.iter().rev().skip(1).all(|page| *page == width) && pages.last().copied().unwrap_or(0) > 0
}

fn check_arrow(read_dir: &Path, result: &Value, failures: &mut Vec<String>) -> Result<(), String> {
    let bytes = fs::read(read_dir.join("batch.arrow")).map_err(|error| format!("arrow:{error}"))?;
    if bytes.len() < 8
        || bytes[..4] != [0xff, 0xff, 0xff, 0xff]
        || bytes[bytes.len() - 8..] != [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
    {
        failures.push("arrow stream marker missing".into());
    }
    let digest = sha256_hex(&bytes);
    let recorded = result.get("ipc_sha256").and_then(Value::as_str).unwrap_or("");
    if digest != recorded {
        failures.push("ipc sha256 does not match result.json".into());
    }
    let sidecar = fs::read_to_string(read_dir.join("batch.arrow.sha256")).unwrap_or_default();
    if sidecar.trim() != digest {
        failures.push("ipc sha256 sidecar mismatch".into());
    }
    Ok(())
}

fn check_rows(
    fixture: &Value,
    mapping: &Value,
    decoded: &Value,
    keys: &Value,
    read_dir: &Path,
    failures: &mut Vec<String>,
) -> Result<(), String> {
    let expected = expected_rows(fixture, mapping)?;
    let rows = decoded.get("rows").and_then(Value::as_array).cloned().unwrap_or_default();
    if rows.len() != expected.len() {
        failures.push(format!("decoded rows {} expected {}", rows.len(), expected.len()));
    }
    let key_ids = keys.as_array().cloned().unwrap_or_default();
    let mut page_bytes = Vec::new();
    for entry in fs::read_dir(read_dir.join("pages")).map_err(|error| format!("pages:{error}"))? {
        let entry = entry.map_err(|error| error.to_string())?;
        page_bytes.push(fs::read(entry.path()).map_err(|error| error.to_string())?);
    }
    for (index, expect) in expected.iter().enumerate() {
        let Some(row) = rows.get(index) else {
            continue;
        };
        let id = row.get("source_doc_id").and_then(Value::as_str).unwrap_or("");
        if id != expect.id {
            failures.push(format!("row {index} id {id} expected {}", expect.id));
        }
        if key_ids.get(index).and_then(Value::as_str) != Some(expect.id.as_str()) {
            failures.push(format!("keys.json position {index}"));
        }
        let raw = row.get("raw_source_utf8").and_then(Value::as_str).unwrap_or("");
        if !page_bytes.iter().any(|page| contains_bytes(page, raw.as_bytes())) {
            failures.push(format!("{id} raw_source is not a substring of a saved page"));
        }
        let from_raw = project_raw(raw, mapping);
        match from_raw {
            Ok(projected) => diff_cells(&expect.id, "raw", &projected, row.get("columns"), true, failures),
            Err(error) => failures.push(format!("{id} raw project {error}")),
        }
        diff_cells(&expect.id, "fixture", &expect.columns, row.get("columns"), false, failures);
    }
    Ok(())
}

fn check_schema(decoded: &Value, fixture: &Value, failures: &mut Vec<String>) {
    let meta = decoded.get("schema_metadata").cloned().unwrap_or(Value::Null);
    let expected = fixture.get("documents").and_then(Value::as_array).map(Vec::len).unwrap_or(0).to_string();
    let pairs = [
        ("oo.protocol", "oo.source.opensearch.batch/v1"),
        ("oo.consistency_grade", "S0"),
        ("oo.s1_claimed", "false"),
        ("oo.joins", "false"),
        ("oo.foreign_keys", "false"),
        ("oo.multi_document_atomicity", "false"),
        ("oo.capture_method", "C5"),
        ("oo.delete_detection", "keyset_reconcile"),
        ("oo.typed_total_relation", "eq"),
        ("oo.sort", "seq_key,doc_key"),
    ];
    for (key, value) in pairs {
        if meta.get(key).and_then(Value::as_str) != Some(value) {
            failures.push(format!("schema metadata {key}"));
        }
    }
    if meta.get("oo.typed_total").and_then(Value::as_str) != Some(expected.as_str()) {
        failures.push("schema metadata oo.typed_total".into());
    }
    let fields = decoded.get("fields").and_then(Value::as_array).cloned().unwrap_or_default();
    let names: BTreeSet<&str> = fields.iter().filter_map(|field| field.get("name").and_then(Value::as_str)).collect();
    if names.contains("where_point") {
        failures.push("arrow schema contains unsupported where_point".into());
    }
    for required in ["source_doc_id", "source_index", "raw_source", "long_value", "present__long_value", "scaled_value", "payload_json", "tags", "thing"] {
        if !names.contains(required) {
            failures.push(format!("arrow schema missing {required}"));
        }
    }
}

struct Expected {
    id: String,
    columns: Map<String, Value>,
}

fn expected_rows(fixture: &Value, mapping: &Value) -> Result<Vec<Expected>, String> {
    let docs = fixture.get("documents").and_then(Value::as_array).ok_or("fixture documents")?;
    let mut rows = Vec::new();
    for doc in docs {
        let id = doc.get("_id").and_then(Value::as_str).ok_or("fixture id")?.to_string();
        let source = doc.get("source").ok_or("fixture source")?;
        rows.push(Expected {
            id,
            columns: project_value(source, mapping)?,
        });
    }
    rows.sort_by(|left, right| {
        let left_key = sort_key(&left.columns);
        let right_key = sort_key(&right.columns);
        left_key.cmp(&right_key)
    });
    Ok(rows)
}

fn sort_key(columns: &Map<String, Value>) -> (i64, String) {
    let seq = columns
        .get("seq_key")
        .and_then(|value| value.get("v"))
        .and_then(Value::as_str)
        .and_then(|text| text.parse::<i64>().ok())
        .unwrap_or(0);
    let doc = columns
        .get("doc_key")
        .and_then(|value| value.get("v"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    (seq, doc)
}

fn project_value(source: &Value, mapping: &Value) -> Result<Map<String, Value>, String> {
    let object = source.as_object().ok_or("source object")?;
    let mut columns = Map::new();
    for (name, spec) in properties(mapping)? {
        if unsupported(spec) {
            continue;
        }
        columns.insert(name.to_string(), cell_from_value(spec, object.get(name))?);
    }
    Ok(columns)
}

fn project_raw(raw: &str, mapping: &Value) -> Result<Map<String, Value>, String> {
    let source: Value = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    let mut projected = project_value(&source, mapping)?;
    let raw_map = raw_object(raw)?;
    for (name, _) in properties(mapping)? {
        if let Some(raw_field) = raw_map.get(name.as_str())
            && let Some(cell) = projected.get_mut(name)
            && cell.get("t").and_then(Value::as_str) == Some("json")
        {
            cell["v"] = json!(raw_field.trim());
        }
    }
    Ok(projected)
}

fn raw_object(text: &str) -> Result<BTreeMap<String, String>, String> {
    use serde::de::{Deserializer as _, MapAccess, Visitor};
    struct Fields;
    impl<'de> Visitor<'de> for Fields {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
            let mut out = BTreeMap::new();
            while let Some(key) = access.next_key::<String>()? {
                let value = access.next_value::<Box<serde_json::value::RawValue>>()?;
                out.insert(key, value.get().to_string());
            }
            Ok(out)
        }
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let fields = deserializer.deserialize_map(Fields).map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(fields)
}

fn cell_from_value(spec: &Value, value: Option<&Value>) -> Result<Value, String> {
    let Some(value) = value else {
        return Ok(json!({"t": "absent", "present": false}));
    };
    if value.is_null() {
        return Ok(json!({"t": "null", "present": true}));
    }
    let multi = spec.pointer("/meta/oo.multi").and_then(Value::as_str) == Some("true");
    if multi && spec.get("type").and_then(Value::as_str) == Some("long") {
        return Ok(json!({
            "t": "json",
            "v": serde_json::to_string(value).map_err(|error| error.to_string())?,
            "present": true
        }));
    }
    if multi {
        return list_cell(spec, value);
    }
    if spec.get("enabled").and_then(Value::as_bool) == Some(false) {
        return Ok(json!({"t": "json", "v": serde_json::to_string(value).map_err(|error| error.to_string())?, "present": true}));
    }
    match spec.get("type").and_then(Value::as_str).unwrap_or("object") {
        "keyword" | "text" => Ok(json!({"t": "utf8", "v": value.as_str().ok_or("utf8")?, "present": true})),
        "long" | "date" => {
            let number = lexeme(value)?;
            let parsed = parse_i64(&number)?;
            if spec.get("type").and_then(Value::as_str) == Some("date") {
                let micros = parsed.checked_mul(1_000).ok_or("millis_overflow")?;
                Ok(json!({"t": "ts_us", "v": micros.to_string(), "present": true}))
            } else {
                Ok(json!({"t": "i64", "v": parsed.to_string(), "present": true}))
            }
        }
        "double" => {
            let number = lexeme(value)?;
            let bits = f64_from_exact_lexeme(&number)?.to_bits().to_string();
            Ok(json!({"t": "f64", "bits": bits, "present": true}))
        }
        "scaled_float" => {
            let factor = spec.get("scaling_factor").and_then(Value::as_i64).ok_or("factor")?;
            let number = scaled_unscaled(&lexeme(value)?, factor)?;
            Ok(json!({"t": "i64", "v": number.to_string(), "present": true}))
        }
        "boolean" => Ok(json!({"t": "bool", "v": value.as_bool().ok_or("bool")?, "present": true})),
        "binary" => {
            let bytes = b64_decode(value.as_str().ok_or("binary")?)?;
            Ok(json!({"t": "bytes", "hex": hex(&bytes), "present": true}))
        }
        "object" => Ok(json!({
            "t": "json",
            "v": serde_json::to_string(value).map_err(|error| error.to_string())?,
            "present": true
        })),
        other => Err(format!("type {other}")),
    }
}

fn list_cell(spec: &Value, value: &Value) -> Result<Value, String> {
    let items = value.as_array().ok_or("list")?;
    let mut out = Vec::new();
    let scalar = strip_multi(spec);
    for item in items {
        if item.is_null() {
            out.push(json!({"t": "null", "present": true}));
        } else {
            out.push(cell_from_value(&scalar, Some(item))?);
        }
    }
    let kind = "list_binary";
    Ok(json!({"t": kind, "v": out, "present": true}))
}

fn strip_multi(spec: &Value) -> Value {
    let mut copy = spec.clone();
    if let Some(object) = copy.as_object_mut() {
        object.remove("meta");
    }
    copy
}

fn diff_cells(
    id: &str,
    label: &str,
    expected: &Map<String, Value>,
    actual: Option<&Value>,
    exact_json: bool,
    failures: &mut Vec<String>,
) {
    let Some(actual) = actual.and_then(Value::as_object) else {
        failures.push(format!("{id} {label} columns missing"));
        return;
    };
    for (name, expect) in expected {
        match actual.get(name) {
            Some(got) if cells_equal(expect, got, exact_json) => {}
            Some(_) => failures.push(format!("{id} {label} {name}")),
            None => failures.push(format!("{id} {label} missing {name}")),
        }
    }
}

fn cells_equal(expected: &Value, actual: &Value, exact_json: bool) -> bool {
    if expected.get("t") == Some(&json!("json")) && actual.get("t") == Some(&json!("json")) && !exact_json {
        let left = expected.get("v").and_then(Value::as_str).and_then(|text| serde_json::from_str::<Value>(text).ok());
        let right = actual.get("v").and_then(Value::as_str).and_then(|text| serde_json::from_str::<Value>(text).ok());
        return left.is_some() && left == right && expected.get("present") == actual.get("present");
    }
    if expected.get("t") == Some(&json!("struct")) && actual.get("t") == Some(&json!("struct")) {
        let left = expected.get("v").and_then(Value::as_object);
        let right = actual.get("v").and_then(Value::as_object);
        let (Some(left), Some(right)) = (left, right) else {
            return false;
        };
        return left.len() == right.len()
            && left.iter().all(|(key, value)| right.get(key).is_some_and(|got| cells_equal(value, got, exact_json)));
    }
    if expected.get("t").and_then(Value::as_str).unwrap_or("").starts_with("list_") {
        let left = expected.get("v").and_then(Value::as_array);
        let right = actual.get("v").and_then(Value::as_array);
        let (Some(left), Some(right)) = (left, right) else {
            return false;
        };
        return expected.get("t") == actual.get("t")
            && left.len() == right.len()
            && left.iter().zip(right).all(|(item, got)| cells_equal(item, got, exact_json));
    }
    expected == actual
}

fn visibility_ok(value: &Value, failures: &mut Vec<String>) {
    expect_bool(value, "before_refresh_has_late", false, failures);
    expect_bool(value, "after_refresh_has_late", true, failures);
    expect_bool(value, "pit_has_late", false, failures);
    expect_u64(value, "pit_close_status", 200, failures);
    let reuse = value.get("pit_reuse_status").and_then(Value::as_u64).unwrap_or(0);
    if reuse < 400 {
        failures.push(format!("pit reuse status {reuse}"));
    }
    expect_u64(value, "late_delete_status", 200, failures);
}

fn reconcile_ok(value: &Value, fixture: &Value, failures: &mut Vec<String>) {
    expect_bool(value, "reader_issued_document_delete", false, failures);
    expect(value, "detection", "keyset_reconcile", failures);
    expect_u64(value, "delete_status", 200, failures);
    let ids: BTreeSet<&str> = fixture
        .get("documents")
        .and_then(Value::as_array)
        .map(|docs| docs.iter().filter_map(|doc| doc.get("_id").and_then(Value::as_str)).collect())
        .unwrap_or_default();
    let deleted = value.get("deleted").and_then(Value::as_array).cloned().unwrap_or_default();
    let deleted_id = deleted.first().and_then(Value::as_str).unwrap_or("");
    if deleted.len() != 1 || !ids.contains(deleted_id) {
        failures.push(format!("reconcile deleted {deleted:?}"));
    }
}

fn negative_ok(value: &Value, failures: &mut Vec<String>) {
    let cases = value.get("cases").and_then(Value::as_array).cloned().unwrap_or_default();
    for case in &cases {
        if case.get("ok").and_then(Value::as_bool) == Some(false) {
            let name = case.get("name").and_then(Value::as_str).unwrap_or("?");
            failures.push(format!("negative {name}"));
        }
    }
    let mget = cases.iter().find(|case| case.get("name").and_then(Value::as_str) == Some("mget"));
    if mget.and_then(|case| case.get("status")).and_then(Value::as_u64) != Some(403) {
        failures.push("mget was not 403".into());
    }
    let unsafe_case = cases.iter().find(|case| case.get("name").and_then(Value::as_str) == Some("unsafe_double"));
    let error = unsafe_case.and_then(|case| case.get("encode_error")).and_then(Value::as_str).unwrap_or("");
    if !error.contains("unsafe_float") {
        failures.push(format!("unsafe double encode error {error}"));
    }
}

fn task_record_ok(value: &Value, failures: &mut Vec<String>) {
    let task = value.get("task").cloned().unwrap_or(Value::Null);
    let id = task.get("task_id").and_then(Value::as_str).unwrap_or("");
    let action = task.get("action").and_then(Value::as_str).unwrap_or("");
    let description = task.get("description").and_then(Value::as_str).unwrap_or("");
    if !id.contains(':') {
        failures.push("task id missing".into());
    }
    if !action.contains("indices:data/read/search") {
        failures.push(format!("task action {action}"));
    }
    if description.is_empty() {
        failures.push("task body missing".into());
    }
    let reason = value.get("terminal_reason").and_then(Value::as_str).unwrap_or("");
    let lower = reason.to_ascii_lowercase();
    if reason.is_empty() || lower.contains("operation timed out") || lower.contains("client closed") {
        failures.push("terminal reason is not the server reason".into());
    }
    if value.get("task_id_present_after").and_then(Value::as_bool) != Some(false) {
        failures.push("task id was not shown absent after the server finished".into());
    }
    expect_u64(value, "pit_open_status", 200, failures);
    expect_u64(value, "pit_held_status", 200, failures);
    expect_u64(value, "pit_close_status", 200, failures);
    let reuse = value.get("pit_reuse_status").and_then(Value::as_u64).unwrap_or(0);
    if reuse < 400 {
        failures.push(format!("pit reuse status {reuse}"));
    }
    for key in [
        "http_current_open_before_status",
        "http_current_open_during_status",
        "http_current_open_after_status",
    ] {
        expect_u64(value, key, 200, failures);
    }
    if value.get("http_current_open_before").and_then(Value::as_i64).is_none()
        || value.get("http_current_open_during").and_then(Value::as_i64).is_none()
        || value.get("http_current_open_after").and_then(Value::as_i64).is_none()
    {
        failures.push("http connection counts were not measured".into());
    }
}

fn cancel_ok(value: &Value, failures: &mut Vec<String>) {
    expect_bool(value, "labeled_as_timeout", false, failures);
    expect_bool(value, "dba_cancel_used", false, failures);
    expect_bool(value, "ipc_present", false, failures);
    expect_bool(value, "client_closed", true, failures);
    expect_bool(value, "cancel_after_time_interval_sent", false, failures);
    expect_bool(value, "natural_completion_before_trigger", false, failures);
    expect(value, "class", "user_cancel", failures);
    let reason_source = value.get("terminal_reason_source").and_then(Value::as_str).unwrap_or("");
    if reason_source == "bound_server_log" {
        let binding = value.get("server_log_binding").unwrap_or(&Value::Null);
        let line = binding.get("line").and_then(Value::as_str).unwrap_or("");
        let task = value.pointer("/task_before/task_id").and_then(Value::as_str).unwrap_or("");
        let node = task.split(':').next().unwrap_or("");
        let index = value.get("workload_index").and_then(Value::as_str).unwrap_or("");
        let marker = value.get("marker").and_then(Value::as_str).unwrap_or("");
        let trigger = json_u64(value.get("user_triggered_unix_ms"));
        let cancelled = json_u64(value.pointer("/task_after/cancellation_time_millis"));
        let others = value.get("other_search_task_ids_before").and_then(Value::as_array);
        if node.is_empty() || index.is_empty() || !line.contains(&format!("[{node}][{index}]["))
            || !line.contains("TaskCancelledException[cancelled task with reason: channel closed]")
            || line.to_ascii_lowercase().contains("expired")
            || binding.get("task_id").and_then(Value::as_str) != Some(task)
            || binding.get("index").and_then(Value::as_str) != Some(index)
            || binding.get("marker").and_then(Value::as_str) != Some(marker)
            || json_u64(binding.get("trigger_unix_ms")) != trigger
            || json_u64(binding.get("cancelled_unix_ms")) != cancelled
            || cancelled < trigger || cancelled - trigger > 5_000
            || !others.is_some_and(Vec::is_empty)
        { failures.push("server log does not bind the exclusive observed cancellation".into()); }
    } else {
        expect(value, "terminal_reason_source", "owned_http_response", failures);
        expect_u64(value, "server_status", 500, failures);
    }
    expect_u64(value, "task_cancel_http_status", 403, failures);
    let path = value.get("search_path").and_then(Value::as_str).unwrap_or("");
    let rejected = value.get("rejected_server_timer").and_then(Value::as_str).unwrap_or("");
    if path.contains("cancel_after_time_interval") || rejected.is_empty() || path.contains(rejected) {
        failures.push(format!("cancel search path {path}"));
    }
    let reason = value.get("terminal_reason").and_then(Value::as_str).unwrap_or("");
    let lower = reason.to_ascii_lowercase();
    let channel = (lower.contains("channel closed") || lower.contains("channel was closed")) && !lower.contains("expired");
    if !channel {
        failures.push(format!("cancel server reason {reason}"));
    }
    let elapsed = value.get("elapsed_ms").and_then(Value::as_u64).unwrap_or(0);
    if elapsed < 200 {
        failures.push(format!("cancel elapsed_ms {elapsed}"));
    }
    let outcome = value.get("outcome").and_then(Value::as_str).unwrap_or("");
    if outcome != "proved" {
        failures.push(format!("cancel outcome {outcome}"));
        return;
    }
    let marker = value.get("marker").and_then(Value::as_str).unwrap_or("");
    let deadline_marker = value.get("deadline_marker").and_then(Value::as_str).unwrap_or("");
    let before = value.get("task_before").cloned().unwrap_or(Value::Null);
    let after = value.get("task_after").cloned().unwrap_or(Value::Null);
    let task_id = before.get("task_id").and_then(Value::as_str).unwrap_or("");
    let after_id = after.get("task_id").and_then(Value::as_str).unwrap_or("");
    if task_id.is_empty() || !task_id.contains(':') || task_id != after_id {
        failures.push(format!("cancel task id {task_id} -> {after_id}"));
    }
    if before.get("cancelled").and_then(Value::as_bool) != Some(false) || after.get("cancelled").and_then(Value::as_bool) != Some(true) {
        failures.push("cancel before/after cancelled flag".into());
    }
    let description = value.get("task").and_then(|task| task.get("description")).and_then(Value::as_str).unwrap_or("");
    if marker.is_empty() || deadline_marker.is_empty() || !description.contains(marker) || description.contains(deadline_marker) {
        failures.push("cancel marker is not unique to this request".into());
    }
    let started = json_u64(before.get("start_time_in_millis"));
    let cancelled_at = json_u64(after.get("cancellation_time_millis"));
    let triggered = json_u64(value.get("user_triggered_unix_ms"));
    if started == 0 || cancelled_at < started || triggered == 0 || triggered.abs_diff(cancelled_at) > 30_000 {
        failures.push(format!("cancel times start {started} trigger {triggered} server {cancelled_at}"));
    }
    let triggered_at = value.get("user_triggered_at").and_then(Value::as_str).unwrap_or("");
    if !triggered_at.contains('T') {
        failures.push("cancel user trigger time missing".into());
    }
    let targeted = value.get("targeted_task_ids").and_then(Value::as_array);
    let one_target = targeted.is_some_and(|items| items.len() == 1 && items[0].as_str() == Some(task_id));
    if !one_target {
        failures.push("cancel targeted a different task".into());
    }
    let others = value.get("other_search_task_ids_before").and_then(Value::as_array).cloned().unwrap_or_default();
    if others.iter().any(|item| item.as_str() == Some(task_id)) {
        failures.push("cancel listed its own task as another task".into());
    }
    for key in ["search_workers_before_status", "search_workers_during_status", "search_workers_after_status"] {
        expect_u64(value, key, 200, failures);
    }
    let during = value.get("search_workers_during").and_then(Value::as_i64).unwrap_or(0);
    let after_workers = value.get("search_workers_after").and_then(Value::as_i64).unwrap_or(0);
    if during < 1 || after_workers >= during {
        failures.push(format!("cancel search workers during {during} after {after_workers}"));
    }
    task_record_ok(value, failures);
}

fn late_ok(value: &Value, failures: &mut Vec<String>) {
    expect(value, "class", "late", failures);
    expect_bool(value, "counted_as_user_cancel", false, failures);
    expect_bool(value, "natural_completion_before_trigger", true, failures);
    expect_bool(value, "task_id_present_at_trigger", false, failures);
    expect_bool(value, "cancel_after_time_interval_sent", false, failures);
    expect_bool(value, "dba_cancel_used", false, failures);
    expect_u64(value, "server_status_before_trigger", 200, failures);
    expect_u64(value, "task_list_status", 200, failures);
    let path = value.get("search_path").and_then(Value::as_str).unwrap_or("");
    if path.contains("cancel_after_time_interval") {
        failures.push(format!("late search path {path}"));
    }
    let triggered = json_u64(value.get("user_triggered_unix_ms"));
    let completed = json_u64(value.get("completed_unix_ms"));
    if completed == 0 || triggered < completed {
        failures.push(format!("late completed {completed} trigger {triggered}"));
    }
    let triggered_at = value.get("user_triggered_at").and_then(Value::as_str).unwrap_or("");
    if !triggered_at.contains('T') {
        failures.push("late user trigger time missing".into());
    }
}

fn json_u64(value: Option<&Value>) -> u64 {
    let Some(value) = value else {
        return 0;
    };
    value.as_u64().or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok())).unwrap_or(0)
}

fn shape_ok(value: &Value, failures: &mut Vec<String>) {
    expect_u64(value, "shadow_status", 400, failures);
    expect(value, "shadow_type", "mapper_parsing_exception", failures);
    expect_bool(value, "shadow_preserves_conflict", true, failures);
    expect(value, "source_doc_id", "shape-1", failures);
    expect(value, "source_index", "oo-dl01w1-44-shape", failures);
    expect_u64(value, "reader_search_status", 200, failures);
    expect_bool(value, "flat_object_preserved", true, failures);
    expect_bool(value, "identity_preserved", true, failures);
}

fn deadline_ok(value: &Value, failures: &mut Vec<String>) {
    expect_bool(value, "dba_cancel_used", false, failures);
    expect_bool(value, "client_closed", false, failures);
    let outcome = value.get("outcome").and_then(Value::as_str).unwrap_or("");
    if outcome != "proved" {
        failures.push(format!("deadline outcome {outcome}"));
    }
    let elapsed = value.get("elapsed_ms").and_then(Value::as_u64).unwrap_or(0);
    let timeout = value.get("timeout_ms").and_then(Value::as_u64).unwrap_or(0);
    let server = value.get("server_interval_ms").and_then(Value::as_u64).unwrap_or(0);
    if server < 200 || timeout <= server || elapsed + 250 < server || elapsed + 1000 >= timeout {
        failures.push(format!("deadline elapsed_ms {elapsed} server_interval_ms {server} timeout_ms {timeout}"));
    }
    if value.get("server_status").and_then(Value::as_u64) == Some(0) {
        failures.push("deadline server status is the client close".into());
    }
    let reason = value.get("terminal_reason").and_then(Value::as_str).unwrap_or("");
    if !reason.contains("expired") || reason.to_ascii_lowercase().contains("channel closed") {
        failures.push(format!("deadline server reason {reason}"));
    }
    task_record_ok(value, failures);
}

fn rowcap_ok(value: &Value, failures: &mut Vec<String>) {
    expect(value, "outcome", "row_cap", failures);
    expect_bool(value, "ipc_present", false, failures);
}

fn bytecap_ok(value: &Value, failures: &mut Vec<String>) {
    expect(value, "outcome", "byte_cap", failures);
    expect_bool(value, "ipc_present", false, failures);
}

fn setup_ok(value: &Value, failures: &mut Vec<String>) {
    let node = value;
    if node.get("distribution").and_then(Value::as_str) != Some("opensearch") {
        failures.push("node distribution".into());
    }
    if node.get("number").and_then(Value::as_str) != Some("3.9.0") {
        failures.push("node version".into());
    }
    expect_bool(node, "identity_ok", true, failures);
}

fn stats_ok(value: &Value, failures: &mut Vec<String>) {
    expect_u64(value, "status", 200, failures);
    if value.get("current_open").is_none() || value.get("total_opened").is_none() {
        failures.push("http stats missing counts".into());
    }
}

fn server_audit_ok(value: &Value, failures: &mut Vec<String>) {
    expect_u64(value, "status", 200, failures);
    expect_u64(value, "reader_compliance_doc_writes", 0, failures);
}

fn block_cast_ok(value: &Value, failures: &mut Vec<String>) {
    let rows = value.get("blocked").and_then(Value::as_array).cloned().unwrap_or_default();
    if rows.len() < 8 {
        failures.push("block-cast rows".into());
        return;
    }
    for row in rows.iter().take(6) {
        if row.get("error").and_then(Value::as_str).is_none() {
            failures.push("block-cast pair was allowed".into());
        }
    }
    let tenth = rows.iter().any(|row| {
        row.get("lexeme").and_then(Value::as_str) == Some("0.1") && row.get("error").and_then(Value::as_str).is_some()
    });
    if !tenth {
        failures.push("block-cast 0.1".into());
    }
}

fn check_audits(dir: &Path, failures: &mut Vec<String>) -> Result<(), String> {
    let mut files = Vec::new();
    collect_jsonl(dir, &mut files)?;
    if files.is_empty() {
        failures.push("no audit jsonl".into());
        return Ok(());
    }
    for path in files {
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        for line in text.lines().filter(|line| !line.is_empty()) {
            let event: Value = serde_json::from_str(line).map_err(|error| format!("audit:{error}"))?;
            let status = event.get("status").and_then(Value::as_u64).unwrap_or(0);
            let method = event.get("method").and_then(Value::as_str).unwrap_or("");
            let path = event.get("path").and_then(Value::as_str).unwrap_or("");
            if method == "POST" && path.contains("/_tasks/_cancel") && (200..300).contains(&status) {
                failures.push("task cancel returned 2xx".into());
            }
            if event.get("identity").and_then(Value::as_str) != Some("reader") {
                continue;
            }
            if !(200..300).contains(&status) {
                continue;
            }
            let bare = path.split('?').next().unwrap_or(path);
            let allowed = match method {
                "GET" => bare == "/" || bare.ends_with("/_mapping") || bare.ends_with("/_count"),
                "POST" => bare == "/_search" || bare.ends_with("/_search") || bare.ends_with("/_search/point_in_time"),
                "DELETE" => bare == "/_search/point_in_time",
                _ => false,
            };
            if !allowed {
                failures.push(format!("reader 2xx {method} {path}"));
            }
        }
    }
    Ok(())
}

fn collect_jsonl(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("audit.jsonl") {
            out.push(path);
        }
    }
    Ok(())
}

fn check_phase(
    name: &str,
    path: &str,
    check: fn(&Value, &mut Vec<String>),
    failures: &mut Vec<String>,
) -> Result<(), String> {
    let value = read_json(path)?;
    let before = failures.len();
    check(&value, failures);
    if failures.len() != before {
        failures.insert(before, format!("phase {name}"));
    }
    Ok(())
}

fn properties(mapping: &Value) -> Result<Vec<(&String, &Value)>, String> {
    let object = mapping.get("properties").and_then(Value::as_object).ok_or("properties")?;
    let mut names: Vec<_> = object.iter().collect();
    names.sort_by(|left, right| left.0.cmp(right.0));
    Ok(names)
}

fn unsupported(spec: &Value) -> bool {
    let kind = spec.get("type").and_then(Value::as_str).unwrap_or("");
    matches!(
        kind,
        "geo_point" | "geo_shape" | "rank_feature" | "rank_features" | "knn_vector" | "alias" | "join"
            | "completion" | "ip" | "percolator" | "nested" | "flat_object" | "unsigned_long"
            | "search_as_you_type"
    ) || (kind == "date" && spec.get("format").and_then(Value::as_str) != Some("epoch_millis"))
}

fn lexeme(value: &Value) -> Result<String, String> {
    match value {
        Value::Number(number) => Ok(number.to_string()),
        other => Err(format!("number lexeme {other}")),
    }
}

fn parse_i64(lexeme: &str) -> Result<i64, String> {
    if lexeme.contains(['.', 'e', 'E', '+']) || lexeme.is_empty() {
        return Err(format!("non integer {lexeme}"));
    }
    lexeme.parse::<i64>().map_err(|_| format!("i64 {lexeme}"))
}

fn f64_from_exact_lexeme(lexeme: &str) -> Result<f64, String> {
    let value: f64 = lexeme.parse().map_err(|_| format!("unsafe_float:{lexeme}"))?;
    if !value.is_finite() || !decimal_equals_f64(lexeme, value) {
        return Err(format!("unsafe_float:{lexeme}"));
    }
    Ok(value)
}

fn scaled_unscaled(lexeme: &str, factor: i64) -> Result<i64, String> {
    let (negative, digits, scale) = parse_decimal(lexeme).ok_or_else(|| format!("scaled {lexeme}"))?;
    let mut magnitude: i128 = 0;
    for byte in digits.bytes() {
        magnitude = magnitude * 10 + i128::from(byte - b'0');
    }
    let ten = pow10(scale);
    let numerator = magnitude * i128::from(factor);
    if ten == 0 || numerator % ten != 0 {
        return Err(format!("inexact {lexeme}"));
    }
    let whole = numerator / ten;
    let signed = if negative { -whole } else { whole };
    i64::try_from(signed).map_err(|_| format!("overflow {lexeme}"))
}

fn decimal_equals_f64(lexeme: &str, value: f64) -> bool {
    let Some((negative, digits, scale)) = parse_decimal(lexeme) else {
        return false;
    };
    let mut integer: u128 = 0;
    for byte in digits.bytes() {
        integer = integer * 10 + u128::from(byte - b'0');
    }
    if value == 0.0 {
        return integer == 0;
    }
    let bits = value.to_bits();
    let sign = (bits >> 63) != 0;
    if sign != negative {
        return false;
    }
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    let (mantissa, power) = if exponent == 0 {
        (u128::from(fraction), -1022 - 52)
    } else {
        (u128::from((1_u64 << 52) | fraction), exponent - 1023 - 52)
    };
    let ten = pow10(scale) as u128;
    if power >= 0 {
        mantissa.checked_shl(power as u32).and_then(|shifted| shifted.checked_mul(ten)) == Some(integer)
    } else {
        integer.checked_shl((-power) as u32).and_then(|left| mantissa.checked_mul(ten).map(|right| left == right)).unwrap_or(false)
    }
}

fn parse_decimal(lexeme: &str) -> Option<(bool, String, u32)> {
    let mut rest = lexeme;
    let negative = if let Some(stripped) = rest.strip_prefix('-') {
        rest = stripped;
        true
    } else {
        false
    };
    if rest.is_empty() || rest.contains(['e', 'E', '+']) {
        return None;
    }
    let (whole, frac) = rest.split_once('.').unwrap_or((rest, ""));
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) || !frac.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((negative, format!("{whole}{frac}"), frac.len() as u32))
}

fn pow10(scale: u32) -> i128 {
    let mut value = 1_i128;
    for _ in 0..scale {
        value *= 10;
    }
    value
}

fn b64_decode(text: &str) -> Result<Vec<u8>, String> {
    fn val(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("b64 length".into());
    }
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let (a, b, c, d) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        let av = val(a).ok_or("b64")?;
        let bv = val(b).ok_or("b64")?;
        out.push((av << 2) | (bv >> 4));
        if c != b'=' {
            let cv = val(c).ok_or("b64")?;
            out.push((bv << 4) | (cv >> 2));
            if d != b'=' {
                let dv = val(d).ok_or("b64")?;
                out.push((cv << 6) | dv);
            }
        }
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

fn expect(value: &Value, key: &str, wanted: &str, failures: &mut Vec<String>) {
    if value.get(key).and_then(Value::as_str) != Some(wanted) {
        failures.push(format!("{key} != {wanted}"));
    }
}

fn expect_bool(value: &Value, key: &str, wanted: bool, failures: &mut Vec<String>) {
    if value.get(key).and_then(Value::as_bool) != Some(wanted) {
        failures.push(format!("{key} bool"));
    }
}

fn expect_u64(value: &Value, key: &str, wanted: u64, failures: &mut Vec<String>) {
    if value.get(key).and_then(Value::as_u64) != Some(wanted) {
        failures.push(format!("{key} != {wanted}"));
    }
}

fn read_json(path: &str) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{path}:{error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("{path}:{error}"))
}

fn args() -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();
    let mut values = env::args().skip(1);
    while let Some(key) = values.next() {
        let name = key.strip_prefix("--").ok_or_else(|| format!("arg {key}"))?;
        let value = values.next().ok_or_else(|| format!("missing {key}"))?;
        map.insert(name.to_string(), value);
    }
    Ok(map)
}

fn required<'a>(map: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    map.get(key).map(String::as_str).ok_or_else(|| format!("missing --{key}"))
}

#[cfg(test)]
mod cancel_judge_tests {
    use super::{cancel_ok, deadline_ok, late_ok};
    use serde_json::{json, Value};

    fn user_cancel_doc() -> Value {
        json!({
            "outcome": "proved",
            "labeled_as_timeout": false,
            "dba_cancel_used": false,
            "ipc_present": false,
            "client_closed": true,
            "cancel_after_time_interval_sent": false,
            "natural_completion_before_trigger": false,
            "class": "user_cancel",
            "terminal_reason_source": "owned_http_response",
            "task_cancel_http_status": 403,
            "server_status": 500,
            "elapsed_ms": 1800,
            "terminal_reason": "cancelled task with reason: channel closed",
            "search_path": "/oo-dl01w1-44-workload/_search?request_cache=false",
            "marker": "oo44usercancel",
            "deadline_marker": "oo44workloaddeadline",
            "rejected_server_timer": "800ms",
            "user_triggered_at": "2026-10-11T00:00:01.000Z",
            "user_triggered_unix_ms": 1_700_000_000_000_u64,
            "task_id_present_after": false,
            "pit_open_status": 200,
            "pit_held_status": 200,
            "pit_close_status": 200,
            "pit_reuse_status": 404,
            "http_current_open_before_status": 200,
            "http_current_open_during_status": 200,
            "http_current_open_after_status": 200,
            "http_current_open_before": 2,
            "http_current_open_during": 3,
            "http_current_open_after": 2,
            "search_workers_before_status": 200,
            "search_workers_during_status": 200,
            "search_workers_after_status": 200,
            "search_workers_during": 1,
            "search_workers_after": 0,
            "targeted_task_ids": ["node:9"],
            "other_search_task_ids_before": [],
            "task": {
                "task_id": "node:9",
                "action": "indices:data/read/search",
                "description": "oo44usercancel only",
                "cancelled": false,
                "start_time_in_millis": 1_700_000_000_000_u64
            },
            "task_before": {
                "task_id": "node:9",
                "cancelled": false,
                "description": "oo44usercancel only",
                "start_time_in_millis": 1_700_000_000_000_u64
            },
            "task_after": {
                "task_id": "node:9",
                "cancelled": true,
                "cancellation_time_millis": 1_700_000_000_500_u64,
                "description": "oo44usercancel only"
            }
        })
    }

    #[test]
    fn channel_close_passes_and_timeout_does_not() {
        let mut failures = Vec::new();
        cancel_ok(&user_cancel_doc(), &mut failures);
        assert!(failures.is_empty(), "{failures:?}");
        let mut expired = user_cancel_doc();
        expired["terminal_reason"] = json!("cancelled task with reason: Cancellation timeout of 800ms is expired");
        failures.clear();
        cancel_ok(&expired, &mut failures);
        assert!(failures.iter().any(|item| item.contains("cancel server reason")), "{failures:?}");
    }

    #[test]
    fn deadline_requires_expiry_and_an_open_client() {
        let mut doc = user_cancel_doc();
        doc["client_closed"] = json!(false);
        doc["terminal_reason"] = json!("cancelled task with reason: Cancellation timeout of 2s is expired");
        doc["elapsed_ms"] = json!(2500);
        doc["timeout_ms"] = json!(20000);
        doc["server_interval_ms"] = json!(2000);
        doc["outcome"] = json!("proved");
        let mut failures = Vec::new();
        deadline_ok(&doc, &mut failures);
        assert!(failures.is_empty(), "{failures:?}");
        doc["terminal_reason"] = json!("cancelled task with reason: channel closed");
        failures.clear();
        deadline_ok(&doc, &mut failures);
        assert!(failures.iter().any(|item| item.contains("deadline server reason")), "{failures:?}");
    }

    #[test]
    fn late_completion_is_not_the_user_cancel() {
        let doc = json!({
            "class": "late",
            "counted_as_user_cancel": false,
            "natural_completion_before_trigger": true,
            "task_id_present_at_trigger": false,
            "cancel_after_time_interval_sent": false,
            "dba_cancel_used": false,
            "server_status_before_trigger": 200,
            "task_list_status": 200,
            "search_path": "/oo-dl01w1-44-workload/_search?request_cache=false",
            "completed_unix_ms": 1_700_000_000_000_u64,
            "user_triggered_unix_ms": 1_700_000_000_020_u64,
            "user_triggered_at": "2026-10-11T00:00:01.020Z"
        });
        let mut failures = Vec::new();
        late_ok(&doc, &mut failures);
        assert!(failures.is_empty(), "{failures:?}");
    }
}
