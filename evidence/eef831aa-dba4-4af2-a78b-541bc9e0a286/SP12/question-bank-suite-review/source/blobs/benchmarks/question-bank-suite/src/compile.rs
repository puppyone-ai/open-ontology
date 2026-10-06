//! Turn one admitted public bank into sql-runner files.
//!
//! `oo_question_bank::evaluate` admits the bank. This module checks that
//! report, keeps one role and one `as_of`, and writes the suite. It does not
//! spawn executables.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use oo_core::limits::DOCUMENT_MAX_BYTES;
use oo_core::parse_json_value;
use serde_json::{Map, Value};

use crate::sha256_hex;

const PROTOCOL: &str = "oo.benchmark.question-bank-suite/v1";
const REPORT: &str = "oo.benchmark.question-bank-suite-report/v1";
const MANIFEST: &str = "oo.benchmark.question-bank-suite-manifest/v1";
const AGENT: &str = "oo.benchmark.question-bank-suite-agent-view/v1";
const BANK_REPORT: &str = "oo.benchmark.question-bank-report/v1";
const RUNNER: &str = "oo.benchmark.sql-runner/v1";
const QUERY: &str = "oo.gateway.query-pg/v1";
const ADAPTER: &str = "oo.benchmark.query-result-adapter/v1";
const MAX_CASES: usize = 32;
const MAX_K: u64 = 8;
const MAX_WALL_MS: u64 = 120_000;
const MAX_ROWS: u64 = 2_000;
const MAX_RESULT_BYTES: u64 = 262_144;
const MAX_QUERY_WALL_MS: u64 = 30_000;
const MAX_POLICIES: usize = 32;
const MAX_TUPLE_BYTES: usize = 128;
const MAX_ROLE: usize = 63;
const MAX_CASE_ID: usize = 64;
const MAX_SQL: usize = 65_536;
const PASSWORD_FILE_MAX: usize = 1_026;
const PASSWORD_MAX: usize = 1_024;
const CA_MAX: usize = 1_048_576;
const EXECUTABLE_MAX: u64 = 268_435_456;
const PATH_MAX: usize = 4_096;

const ROOT_KEYS: &[&str] = &[
    "bank",
    "executables",
    "k",
    "manifest_sha256",
    "mode",
    "selection",
    "templates",
    "trial_wall_ms",
    "tuple",
    "version",
];

const TUPLE_KEYS: &[&str] = &[
    "as_of",
    "budget",
    "dataset",
    "harness",
    "model",
    "project",
    "role",
    "scorer_version",
    "snapshot",
    "suite",
];

const REQUEST_KEYS: &[&str] = &[
    "audit_file",
    "ca_file",
    "catalog",
    "endpoint",
    "identity",
    "password_file",
    "policies",
    "statement",
    "version",
];

const STATEMENT_KEYS: &[&str] = &[
    "max_result_bytes",
    "max_rows",
    "source",
    "sql",
    "wall_clock_ms",
];

const IDENTITY_KEYS: &[&str] = &["policy_id", "project", "role"];
const ENDPOINT_KEYS: &[&str] = &["database", "host", "port", "user"];
const CATALOG_KEYS: &[&str] = &[
    "large_table_row_threshold",
    "max_columns",
    "max_functions",
    "max_operators",
    "max_tables",
    "unknown_rows",
];
const POLICY_KEYS: &[&str] = &[
    "config_digest",
    "generation",
    "login",
    "project",
    "role",
    "table",
];
const TABLE_KEYS: &[&str] = &["columns", "name", "namespace"];
const HEADER_KEYS: &[&str] = &["max_result_bytes", "max_rows", "record", "turn", "version"];

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

    pub fn token(&self) -> &'static str {
        self.token
    }

    pub fn pointer(&self) -> Option<&str> {
        self.pointer.as_deref()
    }

    pub fn stderr_line(&self) -> String {
        match &self.pointer {
            Some(pointer) => format!("question-bank-suite: {} {pointer}\n", self.token),
            None => format!("question-bank-suite: {}\n", self.token),
        }
    }
}

struct Secret {
    bytes: Vec<u8>,
}

impl Secret {
    fn exposes(&self, haystack: &[u8]) -> bool {
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

struct Template {
    request: Value,
    max_rows: u64,
    max_result_bytes: u64,
    secret: Secret,
}

struct Planned {
    bank_index: usize,
    id: String,
    annotation: &'static str,
    gold_sql: String,
    row_comparator: String,
    typed_gold: Value,
    columns: Vec<Value>,
    not_found: bool,
}

struct Input {
    bank_text: String,
    bank: Value,
    manifest_sha256: String,
    role: String,
    as_of: String,
    k: u64,
    trial_wall_ms: u64,
    mode: String,
    tuple: Map<String, Value>,
    executables: Map<String, Value>,
    templates: BTreeMap<String, Value>,
}

pub(crate) fn compile_document(
    document: &str,
    out_dir: &Path,
) -> Result<crate::Compiled, ConfigError> {
    if document.len() > DOCUMENT_MAX_BYTES {
        return Err(ConfigError::at("budget", "/"));
    }
    let root = parse_json_value(document, PROTOCOL).map_err(|_| ConfigError::bare("json"))?;
    let input = parse_input(&root)?;
    let evaluation =
        oo_question_bank::evaluate(&input.bank_text).map_err(|error| match error.pointer() {
            Some(pointer) => ConfigError::at("bank", format!("/bank{pointer}")),
            None => ConfigError::bare("bank"),
        })?;
    let admitted =
        parse_json_value(&evaluation.report, BANK_REPORT).map_err(|_| ConfigError::bare("json"))?;
    verify_admission(&admitted, &input)?;
    let planned = select_group(&admitted, &input)?;
    let templates = check_templates(&input)?;
    let template = templates
        .get(&input.role)
        .ok_or_else(|| ConfigError::at("template", "/templates"))?;
    let out_text = path_text(out_dir, "/out_dir")?;
    check_abs(out_text, "/out_dir")?;
    reserve_out_dir(out_dir)?;
    verify_executables(&input.executables)?;
    let rendered = render(&input, template, &admitted, &planned, out_text)?;
    reject_secret_and_gold(&rendered, &template.secret)?;
    write_tree(out_dir, &rendered.files)?;
    Ok(crate::Compiled {
        report: rendered.report,
    })
}

struct Rendered {
    report: String,
    files: Vec<(PathBuf, Vec<u8>)>,
}

fn parse_input(root: &Value) -> Result<Input, ConfigError> {
    let map = object(root, "/")?;
    exact_keys(map, ROOT_KEYS, "/")?;
    if req_str(map, "version", "/")? != PROTOCOL {
        return Err(ConfigError::at("schema", "/version"));
    }
    let bank = req(map, "bank", "/")?;
    if !bank.is_object() {
        return Err(ConfigError::at("schema", "/bank"));
    }
    let bank_text = serde_json::to_string(bank).map_err(|_| ConfigError::bare("json"))?;
    let manifest_sha256 = req_str(map, "manifest_sha256", "/")?.to_owned();
    if !hex64(&manifest_sha256) {
        return Err(ConfigError::at("hash", "/manifest_sha256"));
    }
    let selection = object(req(map, "selection", "/")?, "/selection")?;
    exact_keys(selection, &["as_of", "role"], "/selection")?;
    let role = req_str(selection, "role", "/selection")?.to_owned();
    ident(&role, MAX_ROLE, "/selection/role")?;
    let as_of = req_str(selection, "as_of", "/selection")?.to_owned();
    bounded_label(&as_of, MAX_TUPLE_BYTES, "/selection/as_of")?;
    let k = json_u64(req(map, "k", "/")?, "/k", 1, MAX_K)?;
    let trial_wall_ms = json_u64(
        req(map, "trial_wall_ms", "/")?,
        "/trial_wall_ms",
        1,
        MAX_WALL_MS,
    )?;
    let mode = match req_str(map, "mode", "/")? {
        "synthetic" | "frozen_eval" => req_str(map, "mode", "/")?.to_owned(),
        _ => return Err(ConfigError::at("mode", "/mode")),
    };
    let tuple = parse_tuple(req(map, "tuple", "/")?, &role)?;
    let executables = parse_executables(req(map, "executables", "/")?)?;
    let templates = parse_template_map(req(map, "templates", "/")?)?;
    Ok(Input {
        bank_text,
        bank: bank.clone(),
        manifest_sha256,
        role,
        as_of,
        k,
        trial_wall_ms,
        mode,
        tuple,
        executables,
        templates,
    })
}

fn parse_tuple(value: &Value, selection_role: &str) -> Result<Map<String, Value>, ConfigError> {
    let map = object(value, "/tuple")?;
    exact_keys(map, TUPLE_KEYS, "/tuple")?;
    for key in ["project", "snapshot", "as_of", "dataset", "suite", "budget"] {
        let text = req_str(map, key, "/tuple")?;
        if text == "unknown" || text == "production" {
            return Err(ConfigError::at("tuple", format!("/tuple/{key}")));
        }
        bounded_label(text, MAX_TUPLE_BYTES, &format!("/tuple/{key}"))?;
    }
    fixed_tuple(req_str(map, "model", "/tuple")?, "model", "none")?;
    fixed_tuple(req_str(map, "harness", "/tuple")?, "harness", "sql-runner")?;
    fixed_tuple(
        req_str(map, "scorer_version", "/tuple")?,
        "scorer_version",
        RUNNER,
    )?;
    let role = req_str(map, "role", "/tuple")?;
    if role == "unknown" || role == "production" || role != selection_role {
        return Err(ConfigError::at("tuple", "/tuple/role"));
    }
    ident(role, MAX_ROLE, "/tuple/role")?;
    Ok(map.clone())
}

fn fixed_tuple(text: &str, key: &str, expect: &str) -> Result<(), ConfigError> {
    if text == "unknown" || text == "production" || text != expect {
        return Err(ConfigError::at("tuple", format!("/tuple/{key}")));
    }
    Ok(())
}

fn parse_executables(value: &Value) -> Result<Map<String, Value>, ConfigError> {
    let map = object(value, "/executables")?;
    exact_keys(map, &["adapter", "comparator", "query_pg"], "/executables")?;
    for name in ["query_pg", "adapter", "comparator"] {
        let pointer = format!("/executables/{name}");
        let one = object(req(map, name, "/executables")?, &pointer)?;
        exact_keys(one, &["path", "sha256"], &pointer)?;
        let path = req_str(one, "path", &pointer)?;
        check_abs(path, &format!("{pointer}/path"))?;
        let digest = req_str(one, "sha256", &pointer)?;
        if !hex64(digest) {
            return Err(ConfigError::at("hash", format!("{pointer}/sha256")));
        }
    }
    Ok(map.clone())
}

fn parse_template_map(value: &Value) -> Result<BTreeMap<String, Value>, ConfigError> {
    let map = object(value, "/templates")?;
    if map.is_empty() {
        return Err(ConfigError::at("template", "/templates"));
    }
    let mut out = BTreeMap::new();
    for (role, template) in map {
        ident(role, MAX_ROLE, "/templates")?;
        let pointer = format!("/templates/{role}");
        let body = object(template, &pointer)?;
        exact_keys(body, &["binding_header", "request"], &pointer)?;
        out.insert(role.clone(), template.clone());
    }
    Ok(out)
}

fn verify_admission(admitted: &Value, input: &Input) -> Result<(), ConfigError> {
    let map = object(admitted, "/")?;
    exact_keys(
        map,
        &[
            "agent_view",
            "content_hashes",
            "manifest",
            "scorer_view",
            "version",
        ],
        "/",
    )?;
    if req_str(map, "version", "/")? != BANK_REPORT {
        return Err(ConfigError::at("schema", "/version"));
    }
    let hashes = object(req(map, "content_hashes", "/")?, "/content_hashes")?;
    exact_keys(
        hashes,
        &["agent_view", "manifest", "scorer_view"],
        "/content_hashes",
    )?;
    for (name, view) in [
        ("manifest", req(map, "manifest", "/")?),
        ("agent_view", req(map, "agent_view", "/")?),
        ("scorer_view", req(map, "scorer_view", "/")?),
    ] {
        let body = serde_json::to_string(view).map_err(|_| ConfigError::bare("json"))?;
        let got = sha256_hex(body.as_bytes());
        let published = req_str(hashes, name, "/content_hashes")?;
        if got != published || !hex64(published) {
            return Err(ConfigError::at("hash", format!("/content_hashes/{name}")));
        }
    }
    let manifest_hash = req_str(hashes, "manifest", "/content_hashes")?;
    if manifest_hash != input.manifest_sha256 {
        return Err(ConfigError::at("hash", "/manifest_sha256"));
    }
    verify_identity(map, input)
}

fn verify_identity(admitted: &Map<String, Value>, input: &Input) -> Result<(), ConfigError> {
    let manifest = object(req(admitted, "manifest", "/")?, "/manifest")?;
    let bank = object(&input.bank, "/bank")?;
    let dataset = object(req(bank, "dataset", "/bank")?, "/bank/dataset")?;
    let suite = object(req(bank, "suite", "/bank")?, "/bank/suite")?;
    same(
        req_str(manifest, "bank_version", "/manifest")?,
        req_str(bank, "bank_version", "/bank")?,
        "/bank/bank_version",
    )?;
    same(
        req_str(manifest, "dataset_id", "/manifest")?,
        req_str(dataset, "id", "/bank/dataset")?,
        "/bank/dataset/id",
    )?;
    same(
        req_str(manifest, "suite_id", "/manifest")?,
        req_str(suite, "id", "/bank/suite")?,
        "/bank/suite/id",
    )?;
    same(
        req_str(manifest, "distribution", "/manifest")?,
        req_str(dataset, "distribution", "/bank/dataset")?,
        "/bank/dataset/distribution",
    )?;
    if req_str(manifest, "distribution", "/manifest")? != "public_synthetic" {
        return Err(ConfigError::at("identity", "/bank/dataset/distribution"));
    }
    same(
        req_str(manifest, "scale", "/manifest")?,
        req_str(dataset, "scale", "/bank/dataset")?,
        "/bank/dataset/scale",
    )?;
    same(
        req_str(manifest, "track", "/manifest")?,
        req_str(suite, "track", "/bank/suite")?,
        "/bank/suite/track",
    )?;
    same(
        req_str(&input.tuple, "dataset", "/tuple")?,
        req_str(manifest, "dataset_id", "/manifest")?,
        "/tuple/dataset",
    )?;
    same(
        req_str(&input.tuple, "suite", "/tuple")?,
        req_str(manifest, "suite_id", "/manifest")?,
        "/tuple/suite",
    )?;
    let bank_questions = array(req(bank, "questions", "/bank")?, "/bank/questions")?;
    let manifest_questions = array(
        req(manifest, "questions", "/manifest")?,
        "/manifest/questions",
    )?;
    if bank_questions.len() != manifest_questions.len() {
        return Err(ConfigError::at("identity", "/bank/questions"));
    }
    for (index, (bank_question, manifest_question)) in
        bank_questions.iter().zip(manifest_questions).enumerate()
    {
        let bank_id = req_str(
            object(bank_question, "/bank/questions")?,
            "id",
            "/bank/questions",
        )?;
        let manifest_id = req_str(
            object(manifest_question, "/manifest/questions")?,
            "id",
            "/manifest/questions",
        )?;
        if bank_id != manifest_id {
            return Err(ConfigError::at(
                "identity",
                format!("/bank/questions/{index}/id"),
            ));
        }
    }
    Ok(())
}

fn same(left: &str, right: &str, pointer: &str) -> Result<(), ConfigError> {
    if left == right {
        Ok(())
    } else {
        Err(ConfigError::at("identity", pointer))
    }
}

fn select_group(admitted: &Value, input: &Input) -> Result<Vec<Planned>, ConfigError> {
    let scorer = object(
        req(object(admitted, "/")?, "scorer_view", "/")?,
        "/scorer_view",
    )?;
    let questions = array(
        req(scorer, "questions", "/scorer_view")?,
        "/scorer_view/questions",
    )?;
    let mut roles = BTreeSet::new();
    let mut group_indexes = Vec::new();
    let mut as_of_mismatch = false;
    for (index, question) in questions.iter().enumerate() {
        let map = object(question, "/scorer_view/questions")?;
        let asker = object(
            req(map, "asker", "/scorer_view/questions")?,
            "/scorer_view/questions",
        )?;
        let role = req_str(asker, "role", "/scorer_view/questions")?;
        roles.insert(role.to_owned());
        if role != input.role {
            continue;
        }
        let as_of = req_str(map, "as_of", "/scorer_view/questions")?;
        if as_of != input.as_of {
            as_of_mismatch = true;
        }
        group_indexes.push(index);
    }
    for role in input.templates.keys() {
        if !roles.contains(role) {
            return Err(ConfigError::at("template", "/templates"));
        }
    }
    if !input.templates.contains_key(&input.role) {
        return Err(ConfigError::at("template", "/templates"));
    }
    if group_indexes.is_empty() {
        return Err(ConfigError::at("group", "/selection/role"));
    }
    if as_of_mismatch {
        return Err(ConfigError::at("group", "/selection/as_of"));
    }
    if group_indexes.len() > MAX_CASES {
        return Err(ConfigError::at("budget", "/cases"));
    }
    let mut planned = Vec::with_capacity(group_indexes.len());
    for index in group_indexes {
        planned.push(plan_question(&questions[index], index)?);
    }
    Ok(planned)
}

fn plan_question(question: &Value, bank_index: usize) -> Result<Planned, ConfigError> {
    let map = object(question, "/scorer_view/questions")?;
    let id = req_str(map, "id", "/scorer_view/questions")?.to_owned();
    ident(
        &id,
        MAX_CASE_ID,
        &format!("/bank/questions/{bank_index}/id"),
    )?;
    let review = object(
        req(map, "review", "/scorer_view/questions")?,
        "/scorer_view/questions",
    )?;
    let annotation = match req_str(review, "status", "/scorer_view/questions")? {
        "fully_approved" => "score",
        "needs_reannotation" => "needs_reannotation",
        _ => {
            return Err(ConfigError::at(
                "schema",
                format!("/bank/questions/{bank_index}/review/status"),
            ));
        }
    };
    let expected = object(
        req(map, "expected", "/scorer_view/questions")?,
        "/scorer_view/questions",
    )?;
    let kind = req_str(expected, "kind", "/scorer_view/questions")?;
    let pointer = format!("/bank/questions/{bank_index}/expected");
    if kind == "clarify"
        || kind == "refuse_permission"
        || !matches!(kind, "table" | "scalar" | "top_k" | "not_found")
    {
        return Err(ConfigError::at("unsupported", format!("{pointer}/kind")));
    }
    let gold_sql = match expected.get("gold_sql") {
        Some(Value::String(sql))
            if !sql.is_empty() && sql.len() <= MAX_SQL && !sql.as_bytes().contains(&0) =>
        {
            sql.clone()
        }
        _ => return Err(ConfigError::at("gold", &pointer)),
    };
    let typed = match expected.get("typed_gold") {
        Some(value) if value.is_object() => value.clone(),
        _ => return Err(ConfigError::at("gold", &pointer)),
    };
    if let Some(columns) = expected.get("value_columns") {
        let list = array(columns, &pointer)?;
        for column in list {
            let column = object(column, &pointer)?;
            if req_str(column, "tolerance", &pointer)? != "exact" {
                return Err(ConfigError::at("gold", &pointer));
            }
        }
    }
    let typed_map = object(&typed, &pointer)?;
    let typed_kind = req_str(typed_map, "kind", &pointer)?;
    let not_found = kind == "not_found";
    let (row_comparator, columns) = if not_found {
        if typed_kind != "not_found" {
            return Err(ConfigError::at("gold", &pointer));
        }
        ("ordered".to_owned(), Vec::new())
    } else {
        if typed_kind != "rows" {
            return Err(ConfigError::at("gold", &pointer));
        }
        let ordered = match expected.get("ordered") {
            Some(Value::Bool(true)) => "ordered",
            Some(Value::Bool(false)) => "unordered",
            _ => return Err(ConfigError::at("gold", &pointer)),
        };
        let source = array(req(typed_map, "columns", &pointer)?, &pointer)?;
        let mut columns = Vec::with_capacity(source.len());
        for (ordinal, column) in source.iter().enumerate() {
            let column = object(column, &pointer)?;
            let name = req_str(column, "name", &pointer)?;
            let typed = req_str(column, "type", &pointer)?;
            if !matches!(typed, "bool" | "int64" | "decimal" | "text") {
                return Err(ConfigError::at("gold", &pointer));
            }
            let mut binding = Map::new();
            binding.insert("name".to_owned(), Value::String(name.to_owned()));
            binding.insert(
                "ordinal".to_owned(),
                Value::from(count_u64(ordinal, &pointer)?),
            );
            binding.insert("type".to_owned(), Value::String(typed.to_owned()));
            columns.push(Value::Object(binding));
        }
        (ordered.to_owned(), columns)
    };
    Ok(Planned {
        bank_index,
        id,
        annotation,
        gold_sql,
        row_comparator,
        typed_gold: typed,
        columns,
        not_found,
    })
}

fn check_templates(input: &Input) -> Result<BTreeMap<String, Template>, ConfigError> {
    let mut out = BTreeMap::new();
    for (role, value) in &input.templates {
        let pointer = format!("/templates/{role}");
        out.insert(role.clone(), check_template(role, value, &pointer)?);
    }
    Ok(out)
}

fn check_template(role: &str, value: &Value, pointer: &str) -> Result<Template, ConfigError> {
    let map = object(value, pointer)?;
    let request_pointer = format!("{pointer}/request");
    let header_pointer = format!("{pointer}/binding_header");
    let request = object(req(map, "request", pointer)?, &request_pointer)?;
    exact_keys(request, REQUEST_KEYS, &request_pointer)?;
    if req_str(request, "version", &request_pointer)? != QUERY {
        return Err(ConfigError::at(
            "schema",
            format!("{request_pointer}/version"),
        ));
    }
    let statement = object(
        req(request, "statement", &request_pointer)?,
        &request_pointer,
    )?;
    exact_keys(
        statement,
        STATEMENT_KEYS,
        &format!("{request_pointer}/statement"),
    )?;
    if req_str(statement, "source", &request_pointer)? != "request" {
        return Err(ConfigError::at(
            "schema",
            format!("{request_pointer}/statement/source"),
        ));
    }
    let sql = req_str(statement, "sql", &request_pointer)?;
    if sql.is_empty() || sql.len() > MAX_SQL || sql.as_bytes().contains(&0) {
        return Err(ConfigError::at(
            "schema",
            format!("{request_pointer}/statement/sql"),
        ));
    }
    let max_rows = json_u64(
        req(statement, "max_rows", &request_pointer)?,
        &format!("{request_pointer}/statement/max_rows"),
        0,
        MAX_ROWS,
    )?;
    let max_result_bytes = json_u64(
        req(statement, "max_result_bytes", &request_pointer)?,
        &format!("{request_pointer}/statement/max_result_bytes"),
        0,
        MAX_RESULT_BYTES,
    )?;
    let _wall = json_u64(
        req(statement, "wall_clock_ms", &request_pointer)?,
        &format!("{request_pointer}/statement/wall_clock_ms"),
        1,
        MAX_QUERY_WALL_MS,
    )?;
    let identity = object(
        req(request, "identity", &request_pointer)?,
        &request_pointer,
    )?;
    exact_keys(
        identity,
        IDENTITY_KEYS,
        &format!("{request_pointer}/identity"),
    )?;
    let identity_role = req_str(identity, "role", &request_pointer)?;
    if identity_role != format!("oo_r_{role}") {
        return Err(ConfigError::at(
            "role",
            format!("{request_pointer}/identity/role"),
        ));
    }
    if !hex64(req_str(identity, "policy_id", &request_pointer)?) {
        return Err(ConfigError::at(
            "schema",
            format!("{request_pointer}/identity/policy_id"),
        ));
    }
    bounded_label(
        req_str(identity, "project", &request_pointer)?,
        MAX_TUPLE_BYTES,
        &format!("{request_pointer}/identity/project"),
    )?;
    check_endpoint(
        req(request, "endpoint", &request_pointer)?,
        &request_pointer,
    )?;
    check_catalog(req(request, "catalog", &request_pointer)?, &request_pointer)?;
    let policies = array(
        req(request, "policies", &request_pointer)?,
        &request_pointer,
    )?;
    if policies.is_empty() || policies.len() > MAX_POLICIES {
        return Err(ConfigError::at(
            "budget",
            format!("{request_pointer}/policies"),
        ));
    }
    for policy in policies {
        check_policy(policy, role, &request_pointer)?;
    }
    let header = object(req(map, "binding_header", pointer)?, &header_pointer)?;
    exact_keys(header, HEADER_KEYS, &header_pointer)?;
    if req_str(header, "record", &header_pointer)? != "header" {
        return Err(ConfigError::at(
            "schema",
            format!("{header_pointer}/record"),
        ));
    }
    if req_str(header, "version", &header_pointer)? != ADAPTER {
        return Err(ConfigError::at(
            "schema",
            format!("{header_pointer}/version"),
        ));
    }
    let _turn = json_u64(
        req(header, "turn", &header_pointer)?,
        &format!("{header_pointer}/turn"),
        0,
        0,
    )?;
    let header_rows = json_u64(
        req(header, "max_rows", &header_pointer)?,
        &format!("{header_pointer}/max_rows"),
        0,
        MAX_ROWS,
    )?;
    let header_bytes = json_u64(
        req(header, "max_result_bytes", &header_pointer)?,
        &format!("{header_pointer}/max_result_bytes"),
        0,
        MAX_RESULT_BYTES,
    )?;
    if header_rows != max_rows || header_bytes != max_result_bytes {
        return Err(ConfigError::at("schema", &header_pointer));
    }
    let password_file = req_str(request, "password_file", &request_pointer)?;
    let ca_file = req_str(request, "ca_file", &request_pointer)?;
    let audit_file = req_str(request, "audit_file", &request_pointer)?;
    check_abs(password_file, &format!("{request_pointer}/password_file"))?;
    check_abs(ca_file, &format!("{request_pointer}/ca_file"))?;
    check_abs(audit_file, &format!("{request_pointer}/audit_file"))?;
    let secret = load_secret(password_file, &request_pointer)?;
    let raw = serde_json::to_vec(&Value::Object(request.clone()))
        .map_err(|_| ConfigError::bare("json"))?;
    if secret.exposes(&raw) {
        return Err(ConfigError::at("password", &request_pointer));
    }
    check_ca(ca_file, &request_pointer)?;
    Ok(Template {
        request: Value::Object(request.clone()),
        max_rows,
        max_result_bytes,
        secret,
    })
}

fn check_endpoint(value: &Value, pointer: &str) -> Result<(), ConfigError> {
    let map = object(value, pointer)?;
    let pointer = format!("{pointer}/endpoint");
    exact_keys(map, ENDPOINT_KEYS, &pointer)?;
    bounded_label(
        req_str(map, "host", &pointer)?,
        253,
        &format!("{pointer}/host"),
    )?;
    let _port = json_u64(
        req(map, "port", &pointer)?,
        &format!("{pointer}/port"),
        1,
        65_535,
    )?;
    bounded_label(
        req_str(map, "database", &pointer)?,
        63,
        &format!("{pointer}/database"),
    )?;
    bounded_label(
        req_str(map, "user", &pointer)?,
        63,
        &format!("{pointer}/user"),
    )?;
    Ok(())
}

fn check_catalog(value: &Value, pointer: &str) -> Result<(), ConfigError> {
    let map = object(value, pointer)?;
    let pointer = format!("{pointer}/catalog");
    exact_keys(map, CATALOG_KEYS, &pointer)?;
    let _tables = json_u64(
        req(map, "max_tables", &pointer)?,
        &format!("{pointer}/max_tables"),
        1,
        32,
    )?;
    let _columns = json_u64(
        req(map, "max_columns", &pointer)?,
        &format!("{pointer}/max_columns"),
        1,
        512,
    )?;
    let _functions = json_u64(
        req(map, "max_functions", &pointer)?,
        &format!("{pointer}/max_functions"),
        1,
        8,
    )?;
    let _operators = json_u64(
        req(map, "max_operators", &pointer)?,
        &format!("{pointer}/max_operators"),
        54,
        128,
    )?;
    let _threshold = json_u64(
        req(map, "large_table_row_threshold", &pointer)?,
        &format!("{pointer}/large_table_row_threshold"),
        0,
        u64::MAX,
    )?;
    match req_str(map, "unknown_rows", &pointer)? {
        "refuse" | "zero" | "saturate" => Ok(()),
        _ => Err(ConfigError::at("schema", format!("{pointer}/unknown_rows"))),
    }
}

fn check_policy(value: &Value, role: &str, pointer: &str) -> Result<(), ConfigError> {
    let map = object(value, pointer)?;
    let pointer = format!("{pointer}/policies");
    exact_keys(map, POLICY_KEYS, &pointer)?;
    if req_str(map, "role", &pointer)? != role {
        return Err(ConfigError::at("role", format!("{pointer}/role")));
    }
    if !hex64(req_str(map, "config_digest", &pointer)?) {
        return Err(ConfigError::at(
            "schema",
            format!("{pointer}/config_digest"),
        ));
    }
    let _generation = json_u64(
        req(map, "generation", &pointer)?,
        &format!("{pointer}/generation"),
        1,
        u64::MAX,
    )?;
    bounded_label(
        req_str(map, "login", &pointer)?,
        63,
        &format!("{pointer}/login"),
    )?;
    bounded_label(
        req_str(map, "project", &pointer)?,
        MAX_TUPLE_BYTES,
        &format!("{pointer}/project"),
    )?;
    let table = object(req(map, "table", &pointer)?, &pointer)?;
    exact_keys(table, TABLE_KEYS, &format!("{pointer}/table"))?;
    bounded_label(
        req_str(table, "namespace", &pointer)?,
        63,
        &format!("{pointer}/table/namespace"),
    )?;
    bounded_label(
        req_str(table, "name", &pointer)?,
        63,
        &format!("{pointer}/table/name"),
    )?;
    let columns = array(req(table, "columns", &pointer)?, &pointer)?;
    if columns.len() > 512 {
        return Err(ConfigError::at(
            "budget",
            format!("{pointer}/table/columns"),
        ));
    }
    for column in columns {
        let column = object(column, &pointer)?;
        exact_keys(
            column,
            &["name", "pii", "semantic_type", "status"],
            &format!("{pointer}/table/columns"),
        )?;
    }
    Ok(())
}

fn verify_executables(executables: &Map<String, Value>) -> Result<(), ConfigError> {
    for name in ["query_pg", "adapter", "comparator"] {
        let pointer = format!("/executables/{name}");
        let one = object(req(executables, name, "/executables")?, &pointer)?;
        let path = req_str(one, "path", &pointer)?;
        let expect = req_str(one, "sha256", &pointer)?;
        let bytes = read_regular(Path::new(path), EXECUTABLE_MAX, &format!("{pointer}/path"))?;
        if sha256_hex(&bytes) != expect {
            return Err(ConfigError::at("hash", format!("{pointer}/sha256")));
        }
    }
    Ok(())
}

fn render(
    input: &Input,
    template: &Template,
    admitted: &Value,
    planned: &[Planned],
    out_text: &str,
) -> Result<Rendered, ConfigError> {
    let admitted_map = object(admitted, "/")?;
    let hashes = object(req(admitted_map, "content_hashes", "/")?, "/content_hashes")?;
    let manifest_view = object(req(admitted_map, "manifest", "/")?, "/manifest")?;
    let mut case_values = Vec::with_capacity(planned.len());
    let mut question_files = Vec::new();
    let mut file_rows = Vec::new();
    for question in planned {
        let base = format!("{out_text}/questions/{}", question.id);
        let request_path = format!("{base}/request.json");
        let binding_path = format!("{base}/binding.json");
        let gold_path = format!("{base}/gold.json");
        check_abs(&request_path, "/out_dir")?;
        check_abs(&binding_path, "/out_dir")?;
        check_abs(&gold_path, "/out_dir")?;
        let request_bytes = request_bytes(&template.request, &question.gold_sql)?;
        let binding_bytes = binding_bytes(template, &question.columns, question.not_found)?;
        let gold_bytes = gold_bytes(&question.typed_gold, &question.row_comparator)?;
        if contains(&request_bytes, gold_path.as_bytes())
            || contains(&binding_bytes, gold_path.as_bytes())
        {
            return Err(ConfigError::at(
                "gold",
                format!("/bank/questions/{}/expected", question.bank_index),
            ));
        }
        let mut case = Map::new();
        case.insert(
            "annotation".to_owned(),
            Value::String(question.annotation.to_owned()),
        );
        case.insert(
            "binding_file".to_owned(),
            Value::String(binding_path.clone()),
        );
        case.insert("gold_file".to_owned(), Value::String(gold_path.clone()));
        case.insert("id".to_owned(), Value::String(question.id.clone()));
        case.insert("k".to_owned(), Value::from(input.k));
        case.insert(
            "request_file".to_owned(),
            Value::String(request_path.clone()),
        );
        case.insert("role".to_owned(), Value::String(input.role.clone()));
        case.insert("trial_wall_ms".to_owned(), Value::from(input.trial_wall_ms));
        case_values.push(Value::Object(case));
        file_rows.push((
            question.id.clone(),
            sha256_hex(&request_bytes),
            sha256_hex(&binding_bytes),
            sha256_hex(&gold_bytes),
        ));
        question_files.push((PathBuf::from(request_path), request_bytes));
        question_files.push((PathBuf::from(binding_path), binding_bytes));
        question_files.push((PathBuf::from(gold_path), gold_bytes));
    }
    let suite = suite_value(input, case_values)?;
    let suite_bytes = json_bytes(&suite)?;
    let suite_hash = sha256_hex(&suite_bytes);
    let agent = agent_value(admitted_map, planned)?;
    let agent_bytes = json_bytes(&agent)?;
    let agent_hash = sha256_hex(&agent_bytes);
    let manifest = manifest_value(
        input,
        manifest_view,
        hashes,
        planned,
        &file_rows,
        &suite_hash,
        &agent_hash,
    )?;
    let manifest_bytes = json_bytes(&manifest)?;
    let manifest_hash = sha256_hex(&manifest_bytes);
    let report = report_value(&manifest, &manifest_hash)?;
    let report_text = serde_json::to_string(&report).map_err(|_| ConfigError::bare("json"))?;
    let mut files = vec![
        (PathBuf::from(format!("{out_text}/suite.json")), suite_bytes),
        (
            PathBuf::from(format!("{out_text}/manifest.json")),
            manifest_bytes,
        ),
        (
            PathBuf::from(format!("{out_text}/agent-view.json")),
            agent_bytes,
        ),
    ];
    files.extend(question_files);
    Ok(Rendered {
        report: report_text,
        files,
    })
}

fn request_bytes(template: &Value, gold_sql: &str) -> Result<Vec<u8>, ConfigError> {
    let mut request = object(template, "/templates")?.clone();
    let statement = object_mut(request.get_mut("statement"))?;
    statement.insert("sql".to_owned(), Value::String(gold_sql.to_owned()));
    json_bytes(&Value::Object(request))
}

fn binding_bytes(
    template: &Template,
    columns: &[Value],
    not_found: bool,
) -> Result<Vec<u8>, ConfigError> {
    let mut header = Map::new();
    header.insert("columns".to_owned(), Value::Array(columns.to_vec()));
    if not_found {
        header.insert(
            "empty_result".to_owned(),
            Value::String("not_found_if_complete_empty".to_owned()),
        );
    }
    header.insert(
        "max_result_bytes".to_owned(),
        Value::from(template.max_result_bytes),
    );
    header.insert("max_rows".to_owned(), Value::from(template.max_rows));
    header.insert("record".to_owned(), Value::String("header".to_owned()));
    header.insert("turn".to_owned(), Value::from(0_u64));
    header.insert("version".to_owned(), Value::String(ADAPTER.to_owned()));
    json_bytes(&Value::Object(header))
}

fn gold_bytes(typed_gold: &Value, row_comparator: &str) -> Result<Vec<u8>, ConfigError> {
    let mut gold = Map::new();
    gold.insert("expected".to_owned(), typed_gold.clone());
    gold.insert(
        "row_comparator".to_owned(),
        Value::String(row_comparator.to_owned()),
    );
    json_bytes(&Value::Object(gold))
}

fn suite_value(input: &Input, cases: Vec<Value>) -> Result<Value, ConfigError> {
    let mut suite = Map::new();
    suite.insert("cases".to_owned(), Value::Array(cases));
    suite.insert(
        "executables".to_owned(),
        Value::Object(input.executables.clone()),
    );
    suite.insert("metadata".to_owned(), Value::String("declared".to_owned()));
    suite.insert("mode".to_owned(), Value::String(input.mode.clone()));
    suite.insert("tuple".to_owned(), Value::Object(input.tuple.clone()));
    suite.insert("version".to_owned(), Value::String(RUNNER.to_owned()));
    Ok(Value::Object(suite))
}

fn agent_value(admitted: &Map<String, Value>, planned: &[Planned]) -> Result<Value, ConfigError> {
    let agent = object(req(admitted, "agent_view", "/")?, "/agent_view")?;
    let questions = array(
        req(agent, "questions", "/agent_view")?,
        "/agent_view/questions",
    )?;
    let wanted: BTreeSet<&str> = planned
        .iter()
        .map(|question| question.id.as_str())
        .collect();
    let mut kept = Vec::new();
    for question in questions {
        let id = req_str(
            object(question, "/agent_view/questions")?,
            "id",
            "/agent_view/questions",
        )?;
        if wanted.contains(id) {
            kept.push(question.clone());
        }
    }
    if kept.len() != planned.len() {
        return Err(ConfigError::at("identity", "/agent_view/questions"));
    }
    let mut out = Map::new();
    out.insert(
        "bank_version".to_owned(),
        Value::String(req_str(agent, "bank_version", "/agent_view")?.to_owned()),
    );
    out.insert(
        "dataset_id".to_owned(),
        Value::String(req_str(agent, "dataset_id", "/agent_view")?.to_owned()),
    );
    out.insert("questions".to_owned(), Value::Array(kept));
    out.insert(
        "suite_id".to_owned(),
        Value::String(req_str(agent, "suite_id", "/agent_view")?.to_owned()),
    );
    out.insert("version".to_owned(), Value::String(AGENT.to_owned()));
    Ok(Value::Object(out))
}

fn manifest_value(
    input: &Input,
    manifest_view: &Map<String, Value>,
    hashes: &Map<String, Value>,
    planned: &[Planned],
    file_rows: &[(String, String, String, String)],
    suite_hash: &str,
    agent_hash: &str,
) -> Result<Value, ConfigError> {
    let excluded = excluded_ids(manifest_view, planned)?;
    let mut questions = Vec::new();
    for (index, question) in planned.iter().enumerate() {
        let mut row = Map::new();
        row.insert(
            "annotation".to_owned(),
            Value::String(question.annotation.to_owned()),
        );
        row.insert("id".to_owned(), Value::String(question.id.clone()));
        row.insert(
            "index".to_owned(),
            Value::from(count_u64(index, "/planned")?),
        );
        questions.push(Value::Object(row));
    }
    let mut files = Vec::new();
    for (id, request_hash, binding_hash, gold_hash) in file_rows {
        let mut row = Map::new();
        row.insert(
            "binding_sha256".to_owned(),
            Value::String(binding_hash.clone()),
        );
        row.insert("gold_sha256".to_owned(), Value::String(gold_hash.clone()));
        row.insert("id".to_owned(), Value::String(id.clone()));
        row.insert(
            "request_sha256".to_owned(),
            Value::String(request_hash.clone()),
        );
        files.push(Value::Object(row));
    }
    let mut file_map = Map::new();
    file_map.insert(
        "agent_view_sha256".to_owned(),
        Value::String(agent_hash.to_owned()),
    );
    file_map.insert("questions".to_owned(), Value::Array(files));
    file_map.insert(
        "suite_sha256".to_owned(),
        Value::String(suite_hash.to_owned()),
    );
    let trial_count = input
        .k
        .checked_mul(count_u64(planned.len(), "/cases")?)
        .ok_or_else(|| ConfigError::at("budget", "/cases"))?;
    let mut out = Map::new();
    out.insert(
        "agent_view_sha256".to_owned(),
        Value::String(agent_hash.to_owned()),
    );
    out.insert(
        "authentication".to_owned(),
        Value::String("not_claimed".to_owned()),
    );
    out.insert(
        "bank_agent_view_sha256".to_owned(),
        Value::String(req_str(hashes, "agent_view", "/content_hashes")?.to_owned()),
    );
    out.insert(
        "bank_manifest_sha256".to_owned(),
        Value::String(req_str(hashes, "manifest", "/content_hashes")?.to_owned()),
    );
    out.insert(
        "bank_scorer_view_sha256".to_owned(),
        Value::String(req_str(hashes, "scorer_view", "/content_hashes")?.to_owned()),
    );
    out.insert(
        "bank_version".to_owned(),
        Value::String(req_str(manifest_view, "bank_version", "/manifest")?.to_owned()),
    );
    out.insert(
        "dataset_id".to_owned(),
        Value::String(req_str(manifest_view, "dataset_id", "/manifest")?.to_owned()),
    );
    out.insert(
        "distribution".to_owned(),
        Value::String(req_str(manifest_view, "distribution", "/manifest")?.to_owned()),
    );
    out.insert("excluded_question_ids".to_owned(), Value::Array(excluded));
    out.insert("files".to_owned(), Value::Object(file_map));
    out.insert("k".to_owned(), Value::from(input.k));
    out.insert("model".to_owned(), Value::String("none".to_owned()));
    out.insert(
        "operator_document".to_owned(),
        Value::String("declared".to_owned()),
    );
    out.insert("planned".to_owned(), Value::Array(questions));
    out.insert("quality_claim".to_owned(), Value::String("none".to_owned()));
    out.insert(
        "question_count".to_owned(),
        Value::from(count_u64(planned.len(), "/cases")?),
    );
    out.insert(
        "scale".to_owned(),
        Value::String(req_str(manifest_view, "scale", "/manifest")?.to_owned()),
    );
    let mut selection = Map::new();
    selection.insert("as_of".to_owned(), Value::String(input.as_of.clone()));
    selection.insert("role".to_owned(), Value::String(input.role.clone()));
    out.insert("selection".to_owned(), Value::Object(selection));
    out.insert(
        "suite_id".to_owned(),
        Value::String(req_str(manifest_view, "suite_id", "/manifest")?.to_owned()),
    );
    out.insert(
        "suite_sha256".to_owned(),
        Value::String(suite_hash.to_owned()),
    );
    out.insert(
        "track".to_owned(),
        Value::String(req_str(manifest_view, "track", "/manifest")?.to_owned()),
    );
    out.insert("trial_count".to_owned(), Value::from(trial_count));
    out.insert("trial_wall_ms".to_owned(), Value::from(input.trial_wall_ms));
    out.insert("version".to_owned(), Value::String(MANIFEST.to_owned()));
    Ok(Value::Object(out))
}

fn report_value(manifest: &Value, manifest_hash: &str) -> Result<Value, ConfigError> {
    let mut report = object(manifest, "/manifest")?.clone();
    report.remove("files");
    report.insert(
        "manifest_sha256".to_owned(),
        Value::String(manifest_hash.to_owned()),
    );
    report.insert("version".to_owned(), Value::String(REPORT.to_owned()));
    Ok(Value::Object(report))
}

fn excluded_ids(
    manifest_view: &Map<String, Value>,
    planned: &[Planned],
) -> Result<Vec<Value>, ConfigError> {
    let questions = array(
        req(manifest_view, "questions", "/manifest")?,
        "/manifest/questions",
    )?;
    let wanted: BTreeSet<&str> = planned
        .iter()
        .map(|question| question.id.as_str())
        .collect();
    let mut excluded = Vec::new();
    for question in questions {
        let id = req_str(
            object(question, "/manifest/questions")?,
            "id",
            "/manifest/questions",
        )?;
        if !wanted.contains(id) {
            excluded.push(Value::String(id.to_owned()));
        }
    }
    Ok(excluded)
}

fn reject_secret_and_gold(rendered: &Rendered, secret: &Secret) -> Result<(), ConfigError> {
    for (_, bytes) in &rendered.files {
        if secret.exposes(bytes) {
            return Err(ConfigError::at("password", "/templates"));
        }
    }
    if secret.exposes(rendered.report.as_bytes()) {
        return Err(ConfigError::at("password", "/templates"));
    }
    Ok(())
}

fn write_tree(out_dir: &Path, files: &[(PathBuf, Vec<u8>)]) -> Result<(), ConfigError> {
    match fs::create_dir(out_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(ConfigError::at("out_dir", "/out_dir"));
        }
        Err(_) => return Err(ConfigError::at("io", "/out_dir")),
    }
    if let Err(error) = private_dir(out_dir).and_then(|()| write_files(out_dir, files)) {
        remove_owned(out_dir);
        return Err(error);
    }
    Ok(())
}

fn write_files(out_dir: &Path, files: &[(PathBuf, Vec<u8>)]) -> Result<(), ConfigError> {
    let questions = out_dir.join("questions");
    fs::create_dir(&questions).map_err(|_| ConfigError::at("io", "/out_dir"))?;
    private_dir(&questions)?;
    let mut dirs = BTreeSet::new();
    for (path, _) in files {
        if let Some(parent) = path.parent()
            && parent != out_dir
            && parent != questions
            && dirs.insert(parent.to_path_buf())
        {
            fs::create_dir(parent).map_err(|_| ConfigError::at("io", "/out_dir"))?;
            private_dir(parent)?;
        }
    }
    for (path, bytes) in files {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| ConfigError::at("io", "/out_dir"))?;
        file.write_all(bytes)
            .map_err(|_| ConfigError::at("io", "/out_dir"))?;
        private_file(path)?;
    }
    Ok(())
}

fn private_dir(path: &Path) -> Result<(), ConfigError> {
    set_mode(path, 0o700)
}

fn private_file(path: &Path) -> Result<(), ConfigError> {
    set_mode(path, 0o600)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), ConfigError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|_| ConfigError::at("io", "/out_dir"))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

fn remove_owned(path: &Path) {
    if path.components().count() >= 4 {
        let _ = fs::remove_dir_all(path);
    }
}

fn json_bytes(value: &Value) -> Result<Vec<u8>, ConfigError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| ConfigError::bare("json"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn object_mut(value: Option<&mut Value>) -> Result<&mut Map<String, Value>, ConfigError> {
    match value {
        Some(Value::Object(map)) => Ok(map),
        _ => Err(ConfigError::bare("json")),
    }
}

fn load_secret(path: &str, pointer: &str) -> Result<Secret, ConfigError> {
    let mut body = read_regular(Path::new(path), byte_cap(PASSWORD_FILE_MAX), pointer)?;
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
    let bytes = read_regular(Path::new(path), byte_cap(CA_MAX), pointer)?;
    if bytes.is_empty() || contains(&bytes, b"PRIVATE KEY") {
        return Err(ConfigError::at("schema", pointer));
    }
    Ok(())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn read_regular(path: &Path, max: u64, pointer: &str) -> Result<Vec<u8>, ConfigError> {
    let meta = fs::symlink_metadata(path).map_err(|_| ConfigError::at("path", pointer))?;
    if !meta.file_type().is_file() {
        return Err(ConfigError::at("path", pointer));
    }
    if meta.len() > max {
        return Err(ConfigError::at("budget", pointer));
    }
    let file = File::open(path).map_err(|_| ConfigError::at("io", pointer))?;
    let mut limited = file.take(max + 1);
    let mut buf = Vec::new();
    if std::io::Read::read_to_end(&mut limited, &mut buf).is_err() {
        return Err(ConfigError::at("io", pointer));
    }
    if u64::try_from(buf.len()).unwrap_or(u64::MAX) > max {
        return Err(ConfigError::at("budget", pointer));
    }
    Ok(buf)
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

fn reserve_out_dir(path: &Path) -> Result<(), ConfigError> {
    let Some(parent) = path.parent() else {
        return Err(ConfigError::at("out_dir", "/out_dir"));
    };
    if parent.as_os_str().is_empty() {
        return Err(ConfigError::at("out_dir", "/out_dir"));
    }
    match fs::symlink_metadata(parent) {
        Ok(meta) if meta.file_type().is_dir() => {}
        _ => return Err(ConfigError::at("out_dir", "/out_dir")),
    }
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(ConfigError::at("out_dir", "/out_dir")),
    }
}

fn path_text<'a>(path: &'a Path, pointer: &str) -> Result<&'a str, ConfigError> {
    path.to_str()
        .ok_or_else(|| ConfigError::at("path", pointer))
}

fn object<'a>(value: &'a Value, pointer: &str) -> Result<&'a Map<String, Value>, ConfigError> {
    match value {
        Value::Object(map) => Ok(map),
        Value::Null => Err(ConfigError::at("schema", pointer)),
        _ => Err(ConfigError::at("schema", pointer)),
    }
}

fn array<'a>(value: &'a Value, pointer: &str) -> Result<&'a [Value], ConfigError> {
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
        .ok_or_else(|| ConfigError::at("schema", format!("{pointer}/{key}")))
}

fn req_str<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    pointer: &str,
) -> Result<&'a str, ConfigError> {
    match req(map, key, pointer)? {
        Value::String(text) => Ok(text),
        Value::Null => Err(ConfigError::at("schema", format!("{pointer}/{key}"))),
        _ => Err(ConfigError::at("schema", format!("{pointer}/{key}"))),
    }
}

fn exact_keys(map: &Map<String, Value>, keys: &[&str], pointer: &str) -> Result<(), ConfigError> {
    let expected: BTreeSet<&str> = keys.iter().copied().collect();
    for key in map.keys() {
        if !expected.contains(key.as_str()) {
            return Err(ConfigError::at("schema", format!("{pointer}/{key}")));
        }
    }
    for key in keys {
        if !map.contains_key(*key) {
            return Err(ConfigError::at("schema", format!("{pointer}/{key}")));
        }
    }
    if map.values().any(Value::is_null) {
        return Err(ConfigError::at("schema", pointer));
    }
    Ok(())
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

fn hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

enum Spell {
    Value(u64),
    Negative,
    Overflow,
    NotInteger,
}

fn count_u64(value: usize, pointer: &str) -> Result<u64, ConfigError> {
    u64::try_from(value).map_err(|_| ConfigError::at("budget", pointer))
}

fn byte_cap(value: usize) -> u64 {
    u64::try_from(value).expect("byte cap fits")
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

fn spell_integer(text: &str) -> Spell {
    if text.is_empty() {
        return Spell::NotInteger;
    }
    let bytes = text.as_bytes();
    if bytes[0] == b'-' {
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
    let mut value = 0u64;
    for byte in bytes {
        let digit = u64::from(byte - b'0');
        if value > (u64::MAX - digit) / 10 {
            return Spell::Overflow;
        }
        value = value * 10 + digit;
    }
    Spell::Value(value)
}
