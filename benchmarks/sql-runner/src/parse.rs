//! Strict suite parse and the file checks that finish before the run directory exists.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use oo_core::limits::DOCUMENT_MAX_BYTES;
use oo_core::parse_json_value;
use serde_json::{Map, Value};

use crate::sha256_hex;

pub(crate) const PROTOCOL: &str = "oo.benchmark.sql-runner/v1";
const ADAPTER_PROTOCOL: &str = "oo.benchmark.query-result-adapter/v1";
const QUERY_PROTOCOL: &str = "oo.gateway.query-pg/v1";
const MAX_CASES: usize = 32;
const MAX_K: u64 = 8;
const MAX_WALL_MS: u64 = 120_000;
const MAX_TUPLE_BYTES: usize = 128;
const MAX_ROLE: usize = 63;
const MAX_CASE_ID: usize = 64;
const MAX_SQL: usize = 65_536;
const MAX_ROWS: u64 = 2_000;
const MAX_RESULT_BYTES: u64 = 262_144;
const MAX_QUERY_WALL_MS: u64 = 30_000;
const MAX_POLICIES: usize = 32;
const PASSWORD_FILE_MAX: usize = 1_026;
const PASSWORD_MAX: usize = 1_024;
const CA_MAX: usize = 1_048_576;
const EXECUTABLE_MAX: usize = 268_435_456;
const PATH_MAX: usize = 4_096;

const TUPLE_KEYS: [&str; 10] = [
    "project",
    "snapshot",
    "as_of",
    "dataset",
    "suite",
    "model",
    "harness",
    "role",
    "budget",
    "scorer_version",
];

const SUITE_KEYS: [&str; 6] = [
    "cases",
    "executables",
    "metadata",
    "mode",
    "tuple",
    "version",
];

const CASE_KEYS: [&str; 8] = [
    "annotation",
    "binding_file",
    "gold_file",
    "id",
    "k",
    "request_file",
    "role",
    "trial_wall_ms",
];

#[derive(Debug, PartialEq, Eq)]
pub struct ConfigError {
    token: &'static str,
    pointer: Option<String>,
}

impl ConfigError {
    pub(crate) fn bare(token: &'static str) -> Self {
        Self {
            token,
            pointer: None,
        }
    }

    pub(crate) fn at(token: &'static str, pointer: impl Into<String>) -> Self {
        Self {
            token,
            pointer: Some(pointer.into()),
        }
    }

    pub fn stderr_line(&self) -> String {
        match &self.pointer {
            Some(pointer) => format!("sql-runner: {} {pointer}\n", self.token),
            None => format!("sql-runner: {}\n", self.token),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Annotation {
    Score,
    NeedsReannotation,
}

pub(crate) struct Secret {
    bytes: Vec<u8>,
}

impl Secret {
    pub(crate) fn exposes(&self, haystack: &[u8]) -> bool {
        !self.bytes.is_empty()
            && haystack
                .windows(self.bytes.len())
                .any(|window| window == self.bytes)
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

pub(crate) struct Tuple {
    pub project: String,
    pub snapshot: String,
    pub as_of: String,
    pub dataset: String,
    pub suite: String,
    pub model: String,
    pub harness: String,
    pub role: String,
    pub budget: String,
    pub scorer_version: String,
}

pub(crate) struct Executables {
    pub query_pg: PathBuf,
    pub adapter: PathBuf,
    pub comparator: PathBuf,
    pub query_hash: String,
    pub adapter_hash: String,
    pub comparator_hash: String,
}

pub(crate) struct Case {
    pub id: String,
    pub k: u32,
    pub annotation: Annotation,
    pub trial_wall_ms: u64,
    pub request: Value,
    pub binding_line: Vec<u8>,
    pub row_comparator: String,
    pub expected: Value,
    pub secret: Secret,
    pub gold_path: String,
}

pub(crate) struct Loaded {
    pub mode: String,
    pub tuple: Tuple,
    pub executables: Executables,
    pub cases: Vec<Case>,
}

enum Spell {
    Value(u64),
    Negative,
    Overflow,
    NotInteger,
}

pub(crate) fn check_abs(text: &str, pointer: &str) -> Result<(), ConfigError> {
    if text.len() > PATH_MAX {
        return Err(ConfigError::at("budget", pointer));
    }
    if !text.starts_with('/') || text == "/" {
        return Err(ConfigError::at("path", pointer));
    }
    if !text.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b':' | b'-')
    }) {
        return Err(ConfigError::at("path", pointer));
    }
    let mut parts = text.split('/');
    if parts.next() != Some("") {
        return Err(ConfigError::at("path", pointer));
    }
    let mut saw = false;
    for part in parts {
        if part.is_empty() || part == "." || part == ".." || part == "unknown" {
            return Err(ConfigError::at("path", pointer));
        }
        saw = true;
    }
    if saw {
        Ok(())
    } else {
        Err(ConfigError::at("path", pointer))
    }
}

pub(crate) fn reserve_run_dir(path: &Path) -> Result<(), ConfigError> {
    let Some(parent) = path.parent() else {
        return Err(ConfigError::at("run_dir", "/run_dir"));
    };
    if parent.as_os_str().is_empty() {
        return Err(ConfigError::at("run_dir", "/run_dir"));
    }
    match fs::symlink_metadata(parent) {
        Ok(meta) if meta.file_type().is_dir() => {}
        _ => return Err(ConfigError::at("run_dir", "/run_dir")),
    }
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(ConfigError::at("run_dir", "/run_dir")),
    }
}

pub(crate) fn load(suite_path: &Path, run_dir: &str) -> Result<Loaded, ConfigError> {
    let bytes = read_regular(suite_path, DOCUMENT_MAX_BYTES, "/suite")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| ConfigError::bare("json"))?;
    if text.len() > DOCUMENT_MAX_BYTES {
        return Err(ConfigError::at("budget", "/"));
    }
    let value = parse_json_value(text, PROTOCOL).map_err(|_| ConfigError::bare("json"))?;
    let root = object(&value, "/")?;
    check_keys(root, &SUITE_KEYS, "/")?;
    let version = req_str(root, "version", "/")?;
    if version != PROTOCOL {
        return Err(ConfigError::at("schema", "/version"));
    }
    let mode = parse_mode(req_str(root, "mode", "/")?)?;
    let metadata = req_str(root, "metadata", "/")?;
    if metadata != "declared" {
        return Err(ConfigError::at("schema", "/metadata"));
    }
    let tuple = parse_tuple(req(root, "tuple", "/")?)?;
    let executables = load_executables(req(root, "executables", "/")?)?;
    let cases = load_cases(req(root, "cases", "/")?, &tuple.role, run_dir)?;
    Ok(Loaded {
        mode,
        tuple,
        executables,
        cases,
    })
}

pub(crate) fn trial_dir(run_dir: &str, id: &str, index: u32) -> String {
    format!("{run_dir}/case-{id}/trial-{index}")
}

pub(crate) fn rewrite_request(request: &Value, audit: &str) -> Result<Vec<u8>, ConfigError> {
    let Value::Object(map) = request else {
        return Err(ConfigError::bare("json"));
    };
    let mut copy = map.clone();
    copy.insert("audit_file".to_owned(), Value::String(audit.to_owned()));
    serde_json::to_vec(&Value::Object(copy)).map_err(|_| ConfigError::bare("json"))
}

fn parse_mode(text: &str) -> Result<String, ConfigError> {
    match text {
        "synthetic" | "frozen_eval" => Ok(text.to_owned()),
        _ => Err(ConfigError::at("mode", "/mode")),
    }
}

fn parse_tuple(value: &Value) -> Result<Tuple, ConfigError> {
    let map = object(value, "/tuple")?;
    check_keys(map, &TUPLE_KEYS, "/tuple")?;
    let project = tuple_label(req_str(map, "project", "/tuple")?, "project")?;
    let snapshot = tuple_label(req_str(map, "snapshot", "/tuple")?, "snapshot")?;
    let as_of = tuple_label(req_str(map, "as_of", "/tuple")?, "as_of")?;
    let dataset = tuple_label(req_str(map, "dataset", "/tuple")?, "dataset")?;
    let suite = tuple_label(req_str(map, "suite", "/tuple")?, "suite")?;
    let model = fixed_tuple(req_str(map, "model", "/tuple")?, "model", "none")?;
    let harness = fixed_tuple(req_str(map, "harness", "/tuple")?, "harness", "sql-runner")?;
    let scorer_version = fixed_tuple(
        req_str(map, "scorer_version", "/tuple")?,
        "scorer_version",
        PROTOCOL,
    )?;
    let role = tuple_role(req_str(map, "role", "/tuple")?)?;
    let budget = tuple_label(req_str(map, "budget", "/tuple")?, "budget")?;
    Ok(Tuple {
        project,
        snapshot,
        as_of,
        dataset,
        suite,
        model,
        harness,
        role,
        budget,
        scorer_version,
    })
}

fn fixed_tuple(text: &str, key: &str, expect: &str) -> Result<String, ConfigError> {
    let pointer = format!("/tuple/{key}");
    if text == "unknown" || text == "production" || text != expect {
        return Err(ConfigError::at("tuple", pointer));
    }
    Ok(text.to_owned())
}

fn tuple_role(text: &str) -> Result<String, ConfigError> {
    if text == "unknown" || text == "production" {
        return Err(ConfigError::at("tuple", "/tuple/role"));
    }
    ident(text, MAX_ROLE, "/tuple/role")?;
    Ok(text.to_owned())
}

fn tuple_label(text: &str, key: &str) -> Result<String, ConfigError> {
    let pointer = format!("/tuple/{key}");
    if text == "unknown" || text == "production" {
        return Err(ConfigError::at("tuple", pointer));
    }
    bounded_label(text, MAX_TUPLE_BYTES, &pointer)?;
    Ok(text.to_owned())
}

fn load_executables(value: &Value) -> Result<Executables, ConfigError> {
    let map = object(value, "/executables")?;
    check_keys(map, &["adapter", "comparator", "query_pg"], "/executables")?;
    let (query_pg, query_hash) = load_one_exe(req(map, "query_pg", "/executables")?, "query_pg")?;
    let (adapter, adapter_hash) = load_one_exe(req(map, "adapter", "/executables")?, "adapter")?;
    let (comparator, comparator_hash) =
        load_one_exe(req(map, "comparator", "/executables")?, "comparator")?;
    Ok(Executables {
        query_pg,
        adapter,
        comparator,
        query_hash,
        adapter_hash,
        comparator_hash,
    })
}

fn load_one_exe(value: &Value, name: &str) -> Result<(PathBuf, String), ConfigError> {
    let pointer = format!("/executables/{name}");
    let map = object(value, &pointer)?;
    check_keys(map, &["path", "sha256"], &pointer)?;
    let path_text = req_str(map, "path", &pointer)?;
    check_abs(path_text, &child(&pointer, "path"))?;
    let expect = req_str(map, "sha256", &pointer)?;
    if !hex64(expect) {
        return Err(ConfigError::at("hash", child(&pointer, "sha256")));
    }
    let bytes = read_regular(
        Path::new(path_text),
        EXECUTABLE_MAX,
        &child(&pointer, "path"),
    )?;
    let got = sha256_hex(&bytes);
    if got != expect {
        return Err(ConfigError::at("hash", child(&pointer, "sha256")));
    }
    Ok((PathBuf::from(path_text), expect.to_owned()))
}

fn load_cases(value: &Value, tuple_role: &str, run_dir: &str) -> Result<Vec<Case>, ConfigError> {
    let items = array(value, "/cases")?;
    if items.is_empty() || items.len() > MAX_CASES {
        return Err(ConfigError::at("budget", "/cases"));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut cases = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let pointer = format!("/cases/{index}");
        let case = load_case(item, &pointer, tuple_role, run_dir)?;
        if !seen.insert(case.id.clone()) {
            return Err(ConfigError::at("schema", child(&pointer, "id")));
        }
        cases.push(case);
    }
    Ok(cases)
}

fn load_case(
    value: &Value,
    pointer: &str,
    tuple_role: &str,
    run_dir: &str,
) -> Result<Case, ConfigError> {
    let map = object(value, pointer)?;
    check_keys(map, &CASE_KEYS, pointer)?;
    let id = req_str(map, "id", pointer)?.to_owned();
    ident(&id, MAX_CASE_ID, &child(pointer, "id"))?;
    let role = req_str(map, "role", pointer)?;
    if role != tuple_role {
        return Err(ConfigError::at("role", child(pointer, "role")));
    }
    ident(role, MAX_ROLE, &child(pointer, "role"))?;
    let k = json_u64(req(map, "k", pointer)?, &child(pointer, "k"), 1, MAX_K)?;
    let k_u32 = u32::try_from(k).expect("k fits");
    let annotation = match req_str(map, "annotation", pointer)? {
        "score" => Annotation::Score,
        "needs_reannotation" => Annotation::NeedsReannotation,
        _ => return Err(ConfigError::at("schema", child(pointer, "annotation"))),
    };
    let trial_wall_ms = json_u64(
        req(map, "trial_wall_ms", pointer)?,
        &child(pointer, "trial_wall_ms"),
        1,
        MAX_WALL_MS,
    )?;
    let request_file = req_str(map, "request_file", pointer)?;
    let binding_file = req_str(map, "binding_file", pointer)?;
    let gold_file = req_str(map, "gold_file", pointer)?;
    check_abs(request_file, &child(pointer, "request_file"))?;
    check_abs(binding_file, &child(pointer, "binding_file"))?;
    check_abs(gold_file, &child(pointer, "gold_file"))?;
    if request_file == gold_file || binding_file == gold_file || request_file == binding_file {
        return Err(ConfigError::at("schema", child(pointer, "gold_file")));
    }
    let request_pointer = child(pointer, "request_file");
    let binding_pointer = child(pointer, "binding_file");
    let gold_pointer = child(pointer, "gold_file");
    let (request, request_raw, secret) = load_request(request_file, role, &request_pointer)?;
    let binding_line = load_binding(binding_file, &request, &binding_pointer)?;
    let (row_comparator, expected) = load_gold(gold_file, &gold_pointer)?;
    reject_gold(&request_raw, gold_file, &request_pointer)?;
    reject_gold(&binding_line, gold_file, &binding_pointer)?;
    let reserialized = serde_json::to_vec(&request).map_err(|_| ConfigError::bare("json"))?;
    reject_secret(&reserialized, &secret, &request_pointer)?;
    reject_gold(&reserialized, gold_file, &request_pointer)?;
    for index in 0..k_u32 {
        let audit = format!("{}/audit.jsonl", trial_dir(run_dir, &id, index));
        check_abs(&audit, &request_pointer)?;
        let rewritten = rewrite_request(&request, &audit)?;
        reject_secret(&rewritten, &secret, &request_pointer)?;
        reject_gold(&rewritten, gold_file, &request_pointer)?;
    }
    Ok(Case {
        id,
        k: k_u32,
        annotation,
        trial_wall_ms,
        request,
        binding_line,
        row_comparator,
        expected,
        secret,
        gold_path: gold_file.to_owned(),
    })
}

fn load_request(
    path: &str,
    role: &str,
    pointer: &str,
) -> Result<(Value, Vec<u8>, Secret), ConfigError> {
    let raw = read_regular(Path::new(path), DOCUMENT_MAX_BYTES, pointer)?;
    let text = std::str::from_utf8(&raw).map_err(|_| ConfigError::at("json", pointer))?;
    let value =
        parse_json_value(text, QUERY_PROTOCOL).map_err(|_| ConfigError::at("json", pointer))?;
    let map = object(&value, pointer)?;
    let version = req_str(map, "version", pointer)?;
    if version != QUERY_PROTOCOL {
        return Err(ConfigError::at("schema", pointer));
    }
    let password_file = req_str(map, "password_file", pointer)?;
    let ca_file = req_str(map, "ca_file", pointer)?;
    let audit_file = req_str(map, "audit_file", pointer)?;
    check_abs(password_file, pointer)?;
    check_abs(ca_file, pointer)?;
    check_abs(audit_file, pointer)?;
    let statement = object(req(map, "statement", pointer)?, pointer)?;
    if req_str(statement, "source", pointer)? != "request" {
        return Err(ConfigError::at("schema", pointer));
    }
    let sql = req_str(statement, "sql", pointer)?;
    if sql.is_empty() || sql.len() > MAX_SQL || sql.as_bytes().contains(&0) {
        return Err(ConfigError::at("schema", pointer));
    }
    let _max_rows = json_u64(req(statement, "max_rows", pointer)?, pointer, 0, MAX_ROWS)?;
    let _max_bytes = json_u64(
        req(statement, "max_result_bytes", pointer)?,
        pointer,
        0,
        MAX_RESULT_BYTES,
    )?;
    let _wall = json_u64(
        req(statement, "wall_clock_ms", pointer)?,
        pointer,
        1,
        MAX_QUERY_WALL_MS,
    )?;
    let identity = object(req(map, "identity", pointer)?, pointer)?;
    let identity_role = req_str(identity, "role", pointer)?;
    if identity_role != format!("oo_r_{role}") {
        return Err(ConfigError::at("role", pointer));
    }
    let policies = array(req(map, "policies", pointer)?, pointer)?;
    if policies.is_empty() || policies.len() > MAX_POLICIES {
        return Err(ConfigError::at("budget", pointer));
    }
    for policy in policies {
        let policy_map = object(policy, pointer)?;
        if req_str(policy_map, "role", pointer)? != role {
            return Err(ConfigError::at("role", pointer));
        }
    }
    let secret = load_secret(password_file, pointer)?;
    reject_secret(&raw, &secret, pointer)?;
    check_ca(ca_file, pointer)?;
    Ok((value, raw, secret))
}

fn load_binding(path: &str, request: &Value, pointer: &str) -> Result<Vec<u8>, ConfigError> {
    let raw = read_regular(Path::new(path), DOCUMENT_MAX_BYTES, pointer)?;
    let text = std::str::from_utf8(&raw).map_err(|_| ConfigError::at("json", pointer))?;
    let value =
        parse_json_value(text, ADAPTER_PROTOCOL).map_err(|_| ConfigError::at("json", pointer))?;
    let map = object(&value, pointer)?;
    check_keys(
        map,
        &[
            "columns",
            "empty_result",
            "max_result_bytes",
            "max_rows",
            "record",
            "turn",
            "version",
        ],
        pointer,
    )?;
    if let Some(empty) = map.get("empty_result") {
        match empty {
            Value::String(text) if text == "rows" || text == "not_found_if_complete_empty" => {}
            _ => return Err(ConfigError::at("schema", child(pointer, "empty_result"))),
        }
    }
    if req_str(map, "record", pointer)? != "header" {
        return Err(ConfigError::at("schema", pointer));
    }
    if req_str(map, "version", pointer)? != ADAPTER_PROTOCOL {
        return Err(ConfigError::at("schema", pointer));
    }
    let _turn = json_u64(req(map, "turn", pointer)?, pointer, 0, 0)?;
    let request_map = object(request, pointer)?;
    let statement = object(req(request_map, "statement", pointer)?, pointer)?;
    let max_rows = json_u64(req(statement, "max_rows", pointer)?, pointer, 0, MAX_ROWS)?;
    let max_bytes = json_u64(
        req(statement, "max_result_bytes", pointer)?,
        pointer,
        0,
        MAX_RESULT_BYTES,
    )?;
    let header_rows = json_u64(req(map, "max_rows", pointer)?, pointer, 0, MAX_ROWS)?;
    let header_bytes = json_u64(
        req(map, "max_result_bytes", pointer)?,
        pointer,
        0,
        MAX_RESULT_BYTES,
    )?;
    if header_rows != max_rows || header_bytes != max_bytes {
        return Err(ConfigError::at("schema", pointer));
    }
    if !req(map, "columns", pointer)?.is_array() {
        return Err(ConfigError::at("schema", pointer));
    }
    let mut line = serde_json::to_vec(&value).map_err(|_| ConfigError::bare("json"))?;
    line.push(b'\n');
    Ok(line)
}

fn load_gold(path: &str, pointer: &str) -> Result<(String, Value), ConfigError> {
    let raw = read_regular(Path::new(path), DOCUMENT_MAX_BYTES, pointer)?;
    let text = std::str::from_utf8(&raw).map_err(|_| ConfigError::at("json", pointer))?;
    let value = parse_json_value(text, PROTOCOL).map_err(|_| ConfigError::at("json", pointer))?;
    let map = object(&value, pointer)?;
    check_keys(map, &["expected", "row_comparator"], pointer)?;
    let row_comparator = req_str(map, "row_comparator", pointer)?;
    if !comparator_name(row_comparator) {
        return Err(ConfigError::at("schema", pointer));
    }
    let expected = req(map, "expected", pointer)?;
    if !expected.is_object() {
        return Err(ConfigError::at("schema", pointer));
    }
    Ok((row_comparator.to_owned(), expected.clone()))
}

fn comparator_name(text: &str) -> bool {
    (1..=32).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        && text != "unknown"
        && text != "production"
}

fn load_secret(path: &str, pointer: &str) -> Result<Secret, ConfigError> {
    let mut body = read_regular(Path::new(path), PASSWORD_FILE_MAX, pointer)?;
    if body.ends_with(b"\r\n") {
        body.truncate(body.len() - 2);
    } else if body.ends_with(b"\n") {
        body.truncate(body.len() - 1);
    }
    if body.is_empty()
        || body.len() > PASSWORD_MAX
        || body.contains(&0)
        || std::str::from_utf8(&body).is_err()
    {
        return Err(ConfigError::at("password", pointer));
    }
    Ok(Secret { bytes: body })
}

fn check_ca(path: &str, pointer: &str) -> Result<(), ConfigError> {
    let bytes = read_regular(Path::new(path), CA_MAX, pointer)?;
    if bytes.is_empty()
        || bytes
            .windows(b"PRIVATE KEY".len())
            .any(|window| window == b"PRIVATE KEY")
    {
        return Err(ConfigError::at("schema", pointer));
    }
    Ok(())
}

fn reject_secret(haystack: &[u8], secret: &Secret, pointer: &str) -> Result<(), ConfigError> {
    if secret.exposes(haystack) {
        Err(ConfigError::at("password", pointer))
    } else {
        Ok(())
    }
}

fn reject_gold(haystack: &[u8], gold_path: &str, pointer: &str) -> Result<(), ConfigError> {
    let needle = gold_path.as_bytes();
    if !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
    {
        Err(ConfigError::at("gold", pointer))
    } else {
        Ok(())
    }
}

fn read_regular(path: &Path, max: usize, pointer: &str) -> Result<Vec<u8>, ConfigError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) => return Err(ConfigError::at("path", pointer)),
    };
    if !meta.file_type().is_file() {
        return Err(ConfigError::at("path", pointer));
    }
    if meta.len() > u64::try_from(max).expect("size") {
        return Err(ConfigError::at("budget", pointer));
    }
    let file = File::open(path).map_err(|_| ConfigError::at("io", pointer))?;
    let mut limited = file.take(u64::try_from(max).expect("size") + 1);
    let mut buf = Vec::new();
    if limited.read_to_end(&mut buf).is_err() {
        return Err(ConfigError::at("io", pointer));
    }
    if buf.len() > max {
        return Err(ConfigError::at("budget", pointer));
    }
    Ok(buf)
}

fn json_u64(value: &Value, pointer: &str, min: u64, max: u64) -> Result<u64, ConfigError> {
    let Value::Number(number) = value else {
        return Err(ConfigError::at("schema", pointer));
    };
    match spell_integer(&number.to_string()) {
        Spell::Value(got) if (min..=max).contains(&got) => Ok(got),
        Spell::Value(_) | Spell::Overflow => Err(ConfigError::at("budget", pointer)),
        Spell::Negative | Spell::NotInteger => Err(ConfigError::at("schema", pointer)),
    }
}

fn hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn spell_integer(text: &str) -> Spell {
    if text.is_empty() {
        return Spell::NotInteger;
    }
    let bytes = text.as_bytes();
    if bytes[0] == b'-' {
        let rest = &text[1..];
        if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
            return Spell::NotInteger;
        }
        if rest.len() > 1 && rest.starts_with('0') {
            return Spell::NotInteger;
        }
        return Spell::Negative;
    }
    if bytes.contains(&b'.')
        || bytes.contains(&b'e')
        || bytes.contains(&b'E')
        || bytes.contains(&b'+')
        || !bytes.iter().all(|byte| byte.is_ascii_digit())
    {
        return Spell::NotInteger;
    }
    if bytes.len() > 1 && bytes[0] == b'0' {
        return Spell::NotInteger;
    }
    match parse_u64(text) {
        Some(value) => Spell::Value(value),
        None => Spell::Overflow,
    }
}

fn parse_u64(text: &str) -> Option<u64> {
    let mut value = 0u64;
    for byte in text.bytes() {
        let digit = u64::from(byte - b'0');
        if value > (u64::MAX - digit) / 10 {
            return None;
        }
        value = value * 10 + digit;
    }
    Some(value)
}

fn ident(text: &str, max: usize, pointer: &str) -> Result<(), ConfigError> {
    let len = text.len();
    if len < 1 || len > max {
        return Err(ConfigError::at("budget", pointer));
    }
    let bytes = text.as_bytes();
    let body_ok = bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'));
    if bytes[0].is_ascii_alphabetic() && body_ok {
        Ok(())
    } else {
        Err(ConfigError::at("schema", pointer))
    }
}

fn bounded_label(text: &str, max: usize, pointer: &str) -> Result<(), ConfigError> {
    let len = text.len();
    if len < 1 || len > max {
        return Err(ConfigError::at("budget", pointer));
    }
    if text.bytes().any(|byte| byte < 0x20 || byte == 0x7F) {
        return Err(ConfigError::at("schema", pointer));
    }
    Ok(())
}

fn object<'a>(value: &'a Value, pointer: &str) -> Result<&'a Map<String, Value>, ConfigError> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(ConfigError::at("schema", pointer)),
    }
}

fn array<'a>(value: &'a Value, pointer: &str) -> Result<&'a Vec<Value>, ConfigError> {
    match value {
        Value::Array(items) => Ok(items),
        _ => Err(ConfigError::at("schema", pointer)),
    }
}

fn req<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    pointer: &str,
) -> Result<&'a Value, ConfigError> {
    map.get(key)
        .ok_or_else(|| ConfigError::at("schema", child(pointer, key)))
}

fn req_str<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    pointer: &str,
) -> Result<&'a str, ConfigError> {
    match req(map, key, pointer)? {
        Value::String(text) => Ok(text),
        _ => Err(ConfigError::at("schema", child(pointer, key))),
    }
}

fn check_keys(
    map: &Map<String, Value>,
    allowed: &[&str],
    pointer: &str,
) -> Result<(), ConfigError> {
    let mut unknown: Vec<&str> = map
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .map(String::as_str)
        .collect();
    unknown.sort_unstable();
    if let Some(key) = unknown.first() {
        return Err(ConfigError::at("schema", child(pointer, key)));
    }
    Ok(())
}

fn child(pointer: &str, key: &str) -> String {
    if pointer == "/" {
        format!("/{key}")
    } else {
        format!("{pointer}/{key}")
    }
}
