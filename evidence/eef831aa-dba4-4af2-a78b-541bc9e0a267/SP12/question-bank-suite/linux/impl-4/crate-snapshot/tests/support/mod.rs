//! Hand-written public banks and operator documents for the suite compiler.
//!
//! The question text is authored here. Nothing in this module is a
//! 300-question bank, a human roster, or a customer holdout.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use oo_question_bank_suite::sha256_hex;

pub const AS_OF: &str = "2026-10-01T00:00:00+08:00";
pub const AS_OF_OTHER: &str = "2026-09-30T16:00:00+08:00";
pub const ROLE: &str = "finance_east";
pub const OTHER_ROLE: &str = "finance_west";
pub const DATASET: &str = "oo-logistics";
pub const SUITE: &str = "public-smoke";
pub const PASSWORD: &str = "pw-a267-secret";
pub const SQL_TABLE: &str =
    "select name, amount from public_receipts where desk = 'east' /* GOLD_SQL_TABLE */";
pub const CELL_TABLE: &str = "GOLD_CELL_TABLE";
pub const SQL_SCALAR: &str =
    "select qty from public_receipts where desk = 'east' /* GOLD_SQL_SCALAR */";
pub const SQL_TOPK: &str =
    "select name, rank from public_receipts where desk = 'east' /* GOLD_SQL_TOPK */";
pub const CELL_TOPK: &str = "GOLD_CELL_TOPK";
pub const SQL_ABSENT: &str =
    "select id from public_parties where name = 'absent' /* GOLD_SQL_NOT_FOUND */";
pub const SQL_NOTE: &str = "select note from public_receipts where desk = 'east' /* GOLD_SQL_RE */";
pub const CELL_NOTE: &str = "GOLD_CELL_RE";
pub const SQL_WEST: &str =
    "select name from public_receipts where desk = 'west' /* EXCLUDED_GOLD_SQL */";
pub const CELL_WEST: &str = "EXCLUDED_CELL";
pub const CONTROL: &str = "CONTROL_TOTAL_CANARY";
pub const ANNOTATION: &str = "ANNOTATION_CANARY";

pub struct World {
    pub root: PathBuf,
    pub password: PathBuf,
    pub ca: PathBuf,
    pub query_pg: PathBuf,
    pub adapter: PathBuf,
    pub comparator: PathBuf,
}

impl World {
    pub fn new(name: &str) -> Self {
        let root = scratch(name);
        let query_pg = root.join("query-pg");
        let adapter = root.join("adapter-bin");
        let comparator = root.join("comparator-bin");
        fs::write(&query_pg, b"query-pg-bytes\n").unwrap();
        fs::write(&adapter, b"adapter-bytes\n").unwrap();
        fs::write(&comparator, b"comparator-bytes\n").unwrap();
        let world = Self {
            password: root.join("password"),
            ca: root.join("ca.pem"),
            query_pg,
            adapter,
            comparator,
            root,
        };
        world.write_secrets();
        world
    }

    pub fn with_executables(
        name: &str,
        query_pg: &Path,
        adapter: &Path,
        comparator: &Path,
    ) -> Self {
        let root = scratch(name);
        let world = Self {
            password: root.join("password"),
            ca: root.join("ca.pem"),
            query_pg: query_pg.to_path_buf(),
            adapter: adapter.to_path_buf(),
            comparator: comparator.to_path_buf(),
            root,
        };
        world.write_secrets();
        world
    }

    fn write_secrets(&self) {
        fs::write(&self.password, format!("{PASSWORD}\n")).unwrap();
        fs::write(
            &self.ca,
            "-----BEGIN CERTIFICATE-----\nMIIB-a267\n-----END CERTIFICATE-----\n",
        )
        .unwrap();
    }

    pub fn out(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

pub fn scratch(name: &str) -> PathBuf {
    let path = PathBuf::from(format!("/tmp/oo-a267-{}-{name}", std::process::id()));
    if path.exists() {
        fs::remove_dir_all(&path).unwrap();
    }
    fs::create_dir(&path).unwrap();
    path
}

pub fn file_hash(path: &Path) -> String {
    sha256_hex(&fs::read(path).unwrap())
}

pub fn hand_bank() -> Value {
    bank(vec![
        rows_question(Rows {
            id: "pub-table",
            role: ROLE,
            group: "east-receipts",
            as_of: AS_OF,
            kind: "table",
            ordered: false,
            sql: SQL_TABLE,
            keys: &["name"],
            values: &[("amount", "exact")],
            columns: &[("name", "text"), ("amount", "decimal")],
            row: vec![text_cell(CELL_TABLE), decimal_cell("120", 2)],
            question: "How many receipts did the east desk post?",
            category: "metric",
            status: "fully_approved",
            dual: "not_declared",
            unresolved: None,
            annotation: Some(ANNOTATION),
            control: Some(CONTROL),
        }),
        rows_question(Rows {
            id: "pub-west",
            role: OTHER_ROLE,
            group: "east-receipts",
            as_of: AS_OF,
            kind: "table",
            ordered: true,
            sql: SQL_WEST,
            keys: &[],
            values: &[("name", "exact")],
            columns: &[("name", "text")],
            row: vec![text_cell(CELL_WEST)],
            question: "How many receipts did the west desk post?",
            category: "metric",
            status: "fully_approved",
            dual: "not_declared",
            unresolved: None,
            annotation: None,
            control: None,
        }),
        rows_question(Rows {
            id: "pub-scalar",
            role: ROLE,
            group: "east-scalar",
            as_of: AS_OF,
            kind: "scalar",
            ordered: true,
            sql: SQL_SCALAR,
            keys: &[],
            values: &[("qty", "exact")],
            columns: &[("qty", "int64")],
            row: vec![int_cell("7")],
            question: "What quantity did the east desk post?",
            category: "metric",
            status: "fully_approved",
            dual: "not_declared",
            unresolved: None,
            annotation: None,
            control: None,
        }),
        rows_question(Rows {
            id: "pub-topk",
            role: ROLE,
            group: "east-topk",
            as_of: AS_OF,
            kind: "top_k",
            ordered: true,
            sql: SQL_TOPK,
            keys: &["name"],
            values: &[("rank", "exact")],
            columns: &[("name", "text"), ("rank", "int64")],
            row: vec![text_cell(CELL_TOPK), int_cell("2")],
            question: "Which east receipt ranks first?",
            category: "top_k",
            status: "fully_approved",
            dual: "not_declared",
            unresolved: None,
            annotation: None,
            control: None,
        }),
        not_found_question("pub-absent", ROLE, "east-absent", AS_OF, SQL_ABSENT),
        rows_question(Rows {
            id: "pub-note",
            role: ROLE,
            group: "east-note",
            as_of: AS_OF,
            kind: "table",
            ordered: true,
            sql: SQL_NOTE,
            keys: &[],
            values: &[("note", "exact")],
            columns: &[("note", "text")],
            row: vec![text_cell(CELL_NOTE)],
            question: "Which east note is still disputed?",
            category: "data_quality",
            status: "needs_reannotation",
            dual: "declared_dual_disagree",
            unresolved: Some("dual_disagree"),
            annotation: None,
            control: None,
        }),
    ])
}

pub fn document(bank: &Value, world: &World, role: &str, as_of: &str, k: u64) -> String {
    let bank_text = serde_json::to_string(bank).unwrap();
    let evaluation = oo_question_bank::evaluate(&bank_text).unwrap();
    let report: Value = serde_json::from_str(&evaluation.report).unwrap();
    let manifest = report["content_hashes"]["manifest"]
        .as_str()
        .unwrap()
        .to_owned();
    document_with_hash(bank, world, role, as_of, k, &manifest, (DATASET, SUITE))
}

pub fn document_with_hash(
    bank: &Value,
    world: &World,
    role: &str,
    as_of: &str,
    k: u64,
    manifest: &str,
    names: (&str, &str),
) -> String {
    let (dataset, suite) = names;
    let mut roles = Vec::new();
    for question in bank["questions"].as_array().unwrap() {
        let found = question["asker"]["role"].as_str().unwrap();
        if !roles.contains(&found) {
            roles.push(found);
        }
    }
    let mut templates = serde_json::Map::new();
    for found in roles {
        templates.insert(found.to_owned(), template(world, found));
    }
    let body = json!({
        "bank": bank,
        "executables": {
            "adapter": exe(&world.adapter),
            "comparator": exe(&world.comparator),
            "query_pg": exe(&world.query_pg)
        },
        "k": k,
        "manifest_sha256": manifest,
        "mode": "synthetic",
        "selection": {"as_of": as_of, "role": role},
        "templates": templates,
        "trial_wall_ms": 30_000,
        "tuple": tuple(role, dataset, suite),
        "version": "oo.benchmark.question-bank-suite/v1"
    });
    serde_json::to_string(&body).unwrap()
}

pub fn stub_document(bank: &Value, manifest: &str, role: &str, as_of: &str, k: u64) -> String {
    let body = json!({
        "bank": bank,
        "executables": {
            "adapter": {"path": "/tmp/oo-a267-adapter", "sha256": "a".repeat(64)},
            "comparator": {"path": "/tmp/oo-a267-comparator", "sha256": "b".repeat(64)},
            "query_pg": {"path": "/tmp/oo-a267-query-pg", "sha256": "c".repeat(64)}
        },
        "k": k,
        "manifest_sha256": manifest,
        "mode": "synthetic",
        "selection": {"as_of": as_of, "role": role},
        "templates": {
            role: {
                "binding_header": {},
                "request": {}
            }
        },
        "trial_wall_ms": 30_000,
        "tuple": tuple(role, DATASET, SUITE),
        "version": "oo.benchmark.question-bank-suite/v1"
    });
    serde_json::to_string(&body).unwrap()
}

fn exe(path: &Path) -> Value {
    json!({
        "path": path.to_str().unwrap(),
        "sha256": file_hash(path)
    })
}

fn tuple(role: &str, dataset: &str, suite: &str) -> Value {
    json!({
        "as_of": "2026-10-01",
        "budget": "k2",
        "dataset": dataset,
        "harness": "sql-runner",
        "model": "none",
        "project": "dispatch",
        "role": role,
        "scorer_version": "oo.benchmark.sql-runner/v1",
        "snapshot": "fixture-a267",
        "suite": suite
    })
}

fn template(world: &World, role: &str) -> Value {
    json!({
        "binding_header": {
            "max_result_bytes": 4096,
            "max_rows": 20,
            "record": "header",
            "turn": 0,
            "version": "oo.benchmark.query-result-adapter/v1"
        },
        "request": {
            "audit_file": format!("{}/audit-placeholder", world.root.display()),
            "ca_file": world.ca.display().to_string(),
            "catalog": {
                "large_table_row_threshold": 100_000,
                "max_columns": 64,
                "max_functions": 4,
                "max_operators": 64,
                "max_tables": 8,
                "unknown_rows": "refuse"
            },
            "endpoint": {
                "database": "bench",
                "host": "db.example",
                "port": 5432,
                "user": "runner"
            },
            "identity": {
                "policy_id": "a".repeat(64),
                "project": "dispatch",
                "role": format!("oo_r_{role}")
            },
            "password_file": world.password.display().to_string(),
            "policies": [{
                "config_digest": "b".repeat(64),
                "generation": 1,
                "login": "runner",
                "project": "dispatch",
                "role": role,
                "table": {
                    "columns": [{
                        "name": "amount",
                        "pii": {"class": "none"},
                        "semantic_type": "amount",
                        "status": "confirmed"
                    }],
                    "name": "receipts",
                    "namespace": "public"
                }
            }],
            "statement": {
                "max_result_bytes": 4096,
                "max_rows": 20,
                "source": "request",
                "sql": "select 1",
                "wall_clock_ms": 30_000
            },
            "version": "oo.gateway.query-pg/v1"
        }
    })
}

pub fn bank(questions: Vec<Value>) -> Value {
    json!({
        "bank_version": "public-fixture-1",
        "dataset": {
            "distribution": "public_synthetic",
            "id": DATASET,
            "scale": "xs"
        },
        "questions": questions,
        "suite": {"id": SUITE, "track": "E"},
        "version": "oo.benchmark.question-bank/v1"
    })
}

struct Rows<'a> {
    id: &'a str,
    role: &'a str,
    group: &'a str,
    as_of: &'a str,
    kind: &'a str,
    ordered: bool,
    sql: &'a str,
    keys: &'a [&'a str],
    values: &'a [(&'a str, &'a str)],
    columns: &'a [(&'a str, &'a str)],
    row: Vec<Value>,
    question: &'a str,
    category: &'a str,
    status: &'a str,
    dual: &'a str,
    unresolved: Option<&'a str>,
    annotation: Option<&'a str>,
    control: Option<&'a str>,
}

fn rows_question(spec: Rows<'_>) -> Value {
    let mut expected = serde_json::Map::new();
    expected.insert("gold_sql".to_owned(), json!(spec.sql));
    expected.insert(
        "key_columns".to_owned(),
        Value::Array(spec.keys.iter().copied().map(json_string).collect()),
    );
    expected.insert("kind".to_owned(), json!(spec.kind));
    expected.insert("ordered".to_owned(), json!(spec.ordered));
    expected.insert(
        "typed_gold".to_owned(),
        json!({
            "columns": spec.columns.iter().map(|(name, ty)| json!({"name": name, "type": ty})).collect::<Vec<_>>(),
            "completion": "complete",
            "kind": "rows",
            "rows": [spec.row],
            "warnings": []
        }),
    );
    expected.insert(
        "value_columns".to_owned(),
        Value::Array(
            spec.values
                .iter()
                .map(|(name, tolerance)| json!({"name": name, "tolerance": tolerance}))
                .collect(),
        ),
    );
    if let Some(reason) = spec.unresolved {
        expected.insert("unresolved".to_owned(), json!(reason));
    }
    shell(Shell {
        id: spec.id,
        role: spec.role,
        group: spec.group,
        as_of: spec.as_of,
        question: spec.question,
        category: spec.category,
        status: spec.status,
        dual: spec.dual,
        expected: Value::Object(expected),
        annotation: spec.annotation,
        control: spec.control,
    })
}

pub fn tiny_rows(id: &str, role: &str, group: &str, as_of: &str) -> Value {
    rows_question(Rows {
        id,
        role,
        group,
        as_of,
        kind: "table",
        ordered: true,
        sql: "select 1",
        keys: &[],
        values: &[("n", "exact")],
        columns: &[("n", "text")],
        row: vec![text_cell("a")],
        question: "Count the public fixture row.",
        category: "metric",
        status: "fully_approved",
        dual: "not_declared",
        unresolved: None,
        annotation: None,
        control: None,
    })
}

pub fn not_found_question(id: &str, role: &str, group: &str, as_of: &str, sql: &str) -> Value {
    shell(Shell {
        id,
        role,
        group,
        as_of,
        question: "Which public party names the absent desk?",
        category: "not_found",
        status: "fully_approved",
        dual: "not_declared",
        expected: json!({
            "gold_sql": sql,
            "kind": "not_found",
            "typed_gold": {
                "completion": "complete",
                "kind": "not_found",
                "warnings": []
            }
        }),
        annotation: None,
        control: None,
    })
}

pub fn missing_gold_question(id: &str, role: &str, group: &str) -> Value {
    shell(Shell {
        id,
        role,
        group,
        as_of: AS_OF,
        question: "What is the west desk receipt total?",
        category: "data_quality",
        status: "needs_reannotation",
        dual: "not_declared",
        expected: json!({"kind": "table", "unresolved": "missing_gold"}),
        annotation: None,
        control: None,
    })
}

pub fn refuse_question(id: &str, role: &str, group: &str) -> Value {
    shell(Shell {
        id,
        role,
        group,
        as_of: AS_OF,
        question: "May this role read the finance receipt total?",
        category: "permission",
        status: "fully_approved",
        dual: "not_declared",
        expected: json!({
            "gold_sql": "select 1 where false",
            "kind": "refuse_permission"
        }),
        annotation: None,
        control: None,
    })
}

pub fn relative_question(id: &str, role: &str, group: &str) -> Value {
    rows_question(Rows {
        id,
        role,
        group,
        as_of: AS_OF,
        kind: "table",
        ordered: true,
        sql: "select amount from public_receipts",
        keys: &[],
        values: &[("amount", "relative")],
        columns: &[("amount", "decimal")],
        row: vec![decimal_cell("120", 2)],
        question: "What amount did the east dock post?",
        category: "metric",
        status: "needs_reannotation",
        dual: "not_declared",
        unresolved: Some("tolerance_not_exact"),
        annotation: None,
        control: None,
    })
}

struct Shell<'a> {
    id: &'a str,
    role: &'a str,
    group: &'a str,
    as_of: &'a str,
    question: &'a str,
    category: &'a str,
    status: &'a str,
    dual: &'a str,
    expected: Value,
    annotation: Option<&'a str>,
    control: Option<&'a str>,
}

fn shell(spec: Shell<'_>) -> Value {
    let Shell {
        id,
        role,
        group,
        as_of,
        question,
        category,
        status,
        dual,
        expected,
        annotation,
        control,
    } = spec;
    let annotations = match annotation {
        Some(text) => json!([{"label": "fixture_note", "text": text}]),
        None => json!([]),
    };
    let totals = match control {
        Some(text) => json!([{"name": "row_count", "value": text}]),
        None => json!([]),
    };
    json!({
        "accepted_interpretations": [],
        "annotations": annotations,
        "as_of": as_of,
        "asker": {"claims": {}, "role": role},
        "category": [category],
        "control_totals": totals,
        "difficulty": "easy",
        "expected": expected,
        "id": id,
        "provenance": {
            "annotator": "unassigned",
            "origin": "public_synthetic_fixture",
            "reviewed_by": "unassigned"
        },
        "question": question,
        "required_metrics": [],
        "review": {
            "attestation": "label_only",
            "dual_annotation": dual,
            "status": status
        },
        "role_group": group,
        "split": "public",
        "version": "public-fixture-1"
    })
}

fn text_cell(value: &str) -> Value {
    json!({"presence": "value", "value": value})
}

fn int_cell(value: &str) -> Value {
    json!({"presence": "value", "value": value})
}

fn decimal_cell(coefficient: &str, scale: i32) -> Value {
    json!({
        "coefficient": coefficient,
        "negative": false,
        "presence": "value",
        "scale": scale
    })
}

fn json_string(text: &str) -> Value {
    Value::String(text.to_owned())
}

pub fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap();
    serde_json::from_str(&text).unwrap()
}

pub fn assert_clean(bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).unwrap();
    for needle in [
        "GOLD_SQL",
        "GOLD_CELL",
        "EXCLUDED_GOLD_SQL",
        "EXCLUDED_CELL",
        CONTROL,
        ANNOTATION,
        PASSWORD,
        "PRIVATE KEY",
    ] {
        assert!(!text.contains(needle), "{needle} leaked");
    }
}
