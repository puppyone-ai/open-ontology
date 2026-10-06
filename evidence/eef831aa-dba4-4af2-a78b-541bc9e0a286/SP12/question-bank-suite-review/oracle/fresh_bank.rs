//! Fresh five-question hand bank for the independent review.
//! Admission is `oo_question_bank::evaluate`. Compilation and the trial are
//! the question-bank-suite and sql-runner binaries. This is not a 300-question
//! bank, a human annotation, or a quality claim.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const ROLE: &str = "review_north";
const OTHER: &str = "review_south";
const AS_OF: &str = "2026-10-06T00:00:00+08:00";
const AS_OF_OTHER: &str = "2026-10-05T00:00:00+08:00";
const DATASET: &str = "oo-review";
const SUITE: &str = "public-review";
const PASSWORD: &str = "pw-a286-review-secret";
const CONTROL: &str = "RV_CONTROL_CANARY";
const ANNOTATION: &str = "RV_ANNOTATION_CANARY";
const CELL_TABLE: &str = "RV_CELL_TABLE";
const SQL_TABLE: &str = "select name, amount from review_receipts where desk = 'north' /* RV_SQL_TABLE */";
const SQL_SOUTH: &str = "select name from review_receipts where desk = 'south' /* RV_SQL_SOUTH */";
const SQL_SCALAR: &str = "select qty from review_receipts where desk = 'north' /* RV_SQL_SCALAR */";
const SQL_TOPK: &str = "select name, rank from review_receipts where desk = 'north' /* RV_SQL_TOPK */";
const SQL_ABSENT: &str = "select id from review_parties where name = 'absent' /* RV_SQL_ABSENT */";
const SQL_NOTE: &str = "select note from review_receipts where desk = 'north' /* RV_SQL_NOTE */";

fn main() {
    if let Err(err) = run() {
        eprintln!("fresh stage=fail detail={err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let suite_bin = env_path("OO_A286_SUITE")?;
    let runner_bin = env_path("OO_A286_SQL_RUNNER")?;
    let adapter = env_path("OO_A286_ADAPTER")?;
    let comparator = env_path("OO_A286_COMPARATOR")?;
    let query_pg = env_path("OO_A286_QUERY_PG")?;
    let fresh = PathBuf::from(env_or("OO_A286_FRESH_DIR", "/evidence/fresh"));
    if fresh.exists() {
        fs::remove_dir_all(&fresh).map_err(|err| format!("remove fresh: {err}"))?;
    }
    fs::create_dir_all(&fresh).map_err(|err| format!("mkdir fresh: {err}"))?;
    let scratch = PathBuf::from("/tmp/a286review");
    if scratch.exists() {
        fs::remove_dir_all(&scratch).map_err(|err| format!("remove scratch: {err}"))?;
    }
    fs::create_dir_all(&scratch).map_err(|err| format!("mkdir scratch: {err}"))?;
    let password = scratch.join("password");
    let ca = scratch.join("ca.pem");
    fs::write(&password, format!("{PASSWORD}\n")).map_err(|err| format!("password: {err}"))?;
    fs::write(
        &ca,
        "-----BEGIN CERTIFICATE-----\nMIIB-a286\n-----END CERTIFICATE-----\n",
    )
    .map_err(|err| format!("ca: {err}"))?;
    let world = World {
        password,
        ca,
        query_pg,
        adapter,
        comparator,
    };

    let bank = main_bank();
    let evaluation = oo_question_bank::evaluate(&serde_json::to_string(&bank).map_err(|err| err.to_string())?)
        .map_err(|err| format!("evaluate: {}", err.stderr_line()))?;
    let admitted: Value = serde_json::from_str(&evaluation.report).map_err(|err| err.to_string())?;
    let hashes = admitted["content_hashes"]
        .as_object()
        .ok_or("content_hashes")?;
    let manifest = hashes["manifest"].as_str().ok_or("manifest hash")?.to_owned();
    let agent_hash = hashes["agent_view"].as_str().ok_or("agent hash")?.to_owned();
    let scorer_hash = hashes["scorer_view"].as_str().ok_or("scorer hash")?.to_owned();
    if agent_hash == scorer_hash {
        return Err("agent and scorer hashes match".to_owned());
    }
    let agent_text = admitted["agent_view"].to_string();
    let scorer_text = admitted["scorer_view"].to_string();
    if agent_text.contains("gold_sql")
        || agent_text.contains(CONTROL)
        || agent_text.contains(ANNOTATION)
        || agent_text.contains("RV_SQL_")
    {
        return Err("agent view contains scorer material".to_owned());
    }
    if !scorer_text.contains("gold_sql") || !scorer_text.contains(CONTROL) || !scorer_text.contains(SQL_TABLE)
    {
        return Err("scorer view dropped gold".to_owned());
    }
    println!("scorer_agent_separated=1");
    println!("manifest_sha256={manifest}");

    expect_refuse(
        &suite_bin,
        &bad_decimal_document(),
        &scratch.join("bad-decimal"),
        "bank",
        Some("/bank/questions/0/expected/typed_gold/rows/0/0/coefficient"),
    )?;
    println!("bad_decimal=bank");
    expect_refuse(&suite_bin, "null", &scratch.join("null-root"), "schema", Some("/"))?;
    println!("top_level_null=schema");

    let mut mixed = main_bank();
    mixed["questions"][0]["as_of"] = json!(AS_OF_OTHER);
    let mixed_hash = manifest_of(&mixed)?;
    expect_refuse(
        &suite_bin,
        &document(&mixed, &mixed_hash, &world, ROLE, AS_OF, 2)?,
        &scratch.join("mixed-as-of"),
        "group",
        Some("/selection/as_of"),
    )?;
    println!("mixed_as_of=group");

    expect_refuse(
        &suite_bin,
        &document(&refuse_bank(), &manifest_of(&refuse_bank())?, &world, ROLE, AS_OF, 1)?,
        &scratch.join("unsupported"),
        "unsupported",
        Some("/bank/questions/1/expected/kind"),
    )?;
    println!("unsupported_whole_compile=1");

    let mut flipped = manifest.clone();
    let last = flipped.len() - 1;
    let replacement = if flipped.as_bytes()[last] == b'0' { b'1' } else { b'0' };
    flipped.replace_range(last.., std::str::from_utf8(&[replacement]).unwrap_or("0"));
    expect_refuse(
        &suite_bin,
        &document(&bank, &flipped, &world, ROLE, AS_OF, 2)?,
        &scratch.join("bad-hash"),
        "hash",
        None,
    )?;
    println!("flipped_hash=refused");

    let mut opted = serde_json::from_str::<Value>(&document(&bank, &manifest, &world, ROLE, AS_OF, 2)?)
        .map_err(|err| err.to_string())?;
    opted["templates"][ROLE]["binding_header"]["empty_result"] = json!("not_found_if_complete_empty");
    expect_refuse(
        &suite_bin,
        &opted.to_string(),
        &scratch.join("template-opt"),
        "schema",
        Some("/templates/review_north/binding_header/empty_result"),
    )?;
    println!("template_empty_result=schema");

    let null_bank = bank_of(vec![null_row("rv-null")]);
    let null_out = scratch.join("null-cell");
    let null_doc = document(&null_bank, &manifest_of(&null_bank)?, &world, ROLE, AS_OF, 1)?;
    compile_ok(&suite_bin, &null_doc, &null_out)?;
    let null_gold = fs::read_to_string(null_out.join("questions/rv-null/gold.json"))
        .map_err(|err| format!("null gold: {err}"))?;
    if !null_gold.contains("\"presence\":\"null\"") {
        return Err(format!("null cell was rewritten: {null_gold}"));
    }
    let null_binding: Value = read_json(&null_out.join("questions/rv-null/binding.json"))?;
    if null_binding.get("empty_result").is_some() {
        return Err("null-cell binding invented empty_result".to_owned());
    }
    println!("null_cell_copied=1");

    let out = fresh.join("suite");
    let report_text = compile_ok(&suite_bin, &document(&bank, &manifest, &world, ROLE, AS_OF, 2)?, &out)?;
    fs::write(fresh.join("compiler.stdout"), &report_text).map_err(|err| err.to_string())?;
    scan_secret(&report_text)?;
    let report: Value = serde_json::from_str(report_text.trim_end()).map_err(|err| err.to_string())?;
    eq(&report["version"], &json!("oo.benchmark.question-bank-suite-report/v1"), "report version")?;
    eq(&report["question_count"], &json!(5), "question_count")?;
    eq(&report["trial_count"], &json!(10), "trial_count")?;
    eq(&report["k"], &json!(2), "k")?;
    eq(&report["quality_claim"], &json!("none"), "quality_claim")?;
    eq(&report["model"], &json!("none"), "model")?;
    eq(&report["authentication"], &json!("not_claimed"), "authentication")?;
    eq(&report["distribution"], &json!("public_synthetic"), "distribution")?;
    eq(&report["selection"]["role"], &json!(ROLE), "selected role")?;
    eq(&report["selection"]["as_of"], &json!(AS_OF), "selected as_of")?;
    eq(&report["bank_manifest_sha256"], &json!(manifest), "validated manifest hash")?;
    eq(&report["bank_agent_view_sha256"], &json!(agent_hash), "agent hash")?;
    eq(&report["bank_scorer_view_sha256"], &json!(scorer_hash), "scorer hash")?;
    eq(&report["excluded_question_ids"], &json!(["rv-south"]), "excluded")?;
    let planned = report["planned"].as_array().ok_or("planned")?;
    if planned.len() != 5 {
        return Err(format!("planned len {}", planned.len()));
    }
    let expected = ["rv-table", "rv-scalar", "rv-topk", "rv-absent", "rv-note"];
    for (index, (item, id)) in planned.iter().zip(expected).enumerate() {
        eq(&item["id"], &json!(id), "planned id")?;
        eq(&item["index"], &json!(index), "planned index")?;
        let annotation = if id == "rv-note" { "needs_reannotation" } else { "score" };
        eq(&item["annotation"], &json!(annotation), "annotation")?;
    }
    if report_text.contains(SQL_TABLE)
        || report_text.contains(CELL_TABLE)
        || report_text.contains(CONTROL)
        || report_text.contains(ANNOTATION)
        || report_text.contains(PASSWORD)
        || report_text.contains("RV_SQL_")
    {
        return Err("compiler report leaked gold or the password".to_owned());
    }
    let agent_file = fs::read_to_string(out.join("agent-view.json")).map_err(|err| err.to_string())?;
    let agent_view: Value = serde_json::from_str(&agent_file).map_err(|err| err.to_string())?;
    let agent_questions = agent_view["questions"].as_array().ok_or("agent questions")?;
    if agent_questions.len() != 5 {
        return Err(format!("agent view len {}", agent_questions.len()));
    }
    if agent_file.contains("gold_sql")
        || agent_file.contains("rv-south")
        || agent_file.contains(CONTROL)
        || agent_file.contains(ANNOTATION)
        || agent_file.contains("typed_gold")
    {
        return Err("written agent view is not separated".to_owned());
    }
    println!("reannotation_kept=1");
    println!("selected_role_as_of=1");

    let absent_gold = fs::read_to_string(out.join("questions/rv-absent/gold.json")).map_err(|err| err.to_string())?;
    let absent_gold_value: Value = serde_json::from_str(&absent_gold).map_err(|err| err.to_string())?;
    eq(&absent_gold_value["expected"]["kind"], &json!("not_found"), "absent gold kind")?;
    if absent_gold_value["expected"].get("rows").is_some() || absent_gold.contains("\"rows\"") {
        return Err(format!("absent gold was reinterpreted: {absent_gold}"));
    }
    let absent_binding: Value = read_json(&out.join("questions/rv-absent/binding.json"))?;
    eq(
        &absent_binding["empty_result"],
        &json!("not_found_if_complete_empty"),
        "absent binding",
    )?;
    let table_binding: Value = read_json(&out.join("questions/rv-table/binding.json"))?;
    if table_binding.get("empty_result").is_some() {
        return Err("table binding has empty_result".to_owned());
    }
    let table_gold: Value = read_json(&out.join("questions/rv-table/gold.json"))?;
    eq(&table_gold["row_comparator"], &json!("unordered"), "table comparator")?;
    eq(&table_gold["expected"]["rows"][0][1]["coefficient"], &json!("450"), "coefficient")?;
    let suite_text = fs::read_to_string(out.join("suite.json")).map_err(|err| err.to_string())?;
    if suite_text.contains(SQL_SOUTH) || suite_text.contains("rv-south") {
        return Err("excluded south question was planned".to_owned());
    }
    println!("empty_gold_not_reinterpreted=1");

    write_responses(&world.query_pg)?;
    let run_dir = fresh.join("run");
    let output = Command::new(&runner_bin)
        .args([
            "--suite",
            out.join("suite.json").to_str().ok_or("suite path")?,
            "--run-dir",
            run_dir.to_str().ok_or("run path")?,
        ])
        .output()
        .map_err(|err| format!("spawn runner: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    fs::write(fresh.join("runner.stdout"), &stdout).map_err(|err| err.to_string())?;
    fs::write(fresh.join("runner.stderr"), &stderr).map_err(|err| err.to_string())?;
    let code = output.status.code().ok_or("runner signal")?;
    if code != 1 || !stderr.is_empty() {
        return Err(format!("runner code={code} stderr={stderr} stdout={stdout}"));
    }
    let report_bytes = fs::read(run_dir.join("report.json")).map_err(|err| format!("runner report: {err}"))?;
    if output.stdout != report_bytes {
        return Err("runner stdout is not report.json".to_owned());
    }
    scan_secret(&stdout)?;
    scan_tree(&out)?;
    scan_tree(&run_dir)?;
    let run_report: Value = serde_json::from_slice(&report_bytes).map_err(|err| err.to_string())?;
    eq(&run_report["question_count"], &json!(5), "runner questions")?;
    eq(&run_report["trial_count"], &json!(10), "runner trials")?;
    eq(&run_report["pass_count"], &json!(8), "pass_count")?;
    eq(&run_report["fail_count"], &json!(0), "fail_count")?;
    eq(&run_report["reannotation_count"], &json!(2), "reannotation_count")?;
    eq(&run_report["not_run_count"], &json!(0), "not_run")?;
    eq(&run_report["passed"], &json!(false), "passed")?;
    eq(&run_report["tuple"]["model"], &json!("none"), "runner model")?;
    eq(&run_report["tuple"]["harness"], &json!("sql-runner"), "runner harness")?;
    let trials = run_report["trials"].as_array().ok_or("trials")?;
    if trials.len() != 10 {
        return Err(format!("trial len {}", trials.len()));
    }
    let mut seen = [0u32; 5];
    for trial in trials {
        let id = trial["case_id"].as_str().ok_or("case_id")?;
        let slot = match id {
            "rv-table" => 0,
            "rv-scalar" => 1,
            "rv-topk" => 2,
            "rv-absent" => 3,
            "rv-note" => 4,
            other => return Err(format!("unexpected case {other}")),
        };
        seen[slot] += 1;
        let status = trial["status"].as_str().unwrap_or("");
        let verdict = trial["comparator_verdict"].as_str().unwrap_or("");
        if id == "rv-note" {
            if status != "needs_reannotation" || verdict != "match" {
                return Err(format!("note trial {trial}"));
            }
        } else if status != "pass" || verdict != "match" {
            return Err(format!("trial {trial}"));
        }
    }
    if seen != [2, 2, 2, 2, 2] {
        return Err(format!("denominator {seen:?}"));
    }
    let table_adapter = fs::read_to_string(run_dir.join("case-rv-table/trial-0/adapter.stdout"))
        .map_err(|err| format!("table adapter: {err}"))?;
    if !table_adapter.contains("\"coefficient\":\"450\"") || table_adapter.contains("4.50") {
        return Err(format!("decimal adapter stdout={table_adapter}"));
    }
    let absent_adapter = fs::read_to_string(run_dir.join("case-rv-absent/trial-0/adapter.stdout"))
        .map_err(|err| format!("absent adapter: {err}"))?;
    if absent_adapter != "{\"kind\":\"not_found\",\"completion\":\"complete\",\"warnings\":[]}\n" {
        return Err(format!("absent adapter stdout={absent_adapter}"));
    }
    assert_header_preserved(&out, &run_dir, "rv-absent", Some("not_found_if_complete_empty"))?;
    assert_header_preserved(&out, &run_dir, "rv-table", None)?;
    println!("denominator_5x2=1");
    println!("runner_header_preserved=1");
    println!("fresh_bank_ok");
    Ok(())
}

struct World {
    password: PathBuf,
    ca: PathBuf,
    query_pg: String,
    adapter: String,
    comparator: String,
}

fn main_bank() -> Value {
    bank_of(vec![
        rows_question(Rows {
            id: "rv-table",
            role: ROLE,
            group: "north-receipts",
            as_of: AS_OF,
            kind: "table",
            ordered: false,
            sql: SQL_TABLE,
            keys: &["name"],
            values: &[("amount", "exact")],
            columns: &[("name", "text"), ("amount", "decimal")],
            row: vec![text_cell(CELL_TABLE), decimal_cell("450", 2)],
            question: "What amount did the north review desk post?",
            category: "metric",
            status: "fully_approved",
            dual: "not_declared",
            unresolved: None,
            annotation: Some(ANNOTATION),
            control: Some(CONTROL),
        }),
        rows_question(Rows {
            id: "rv-south",
            role: OTHER,
            group: "south-receipts",
            as_of: AS_OF,
            kind: "table",
            ordered: true,
            sql: SQL_SOUTH,
            keys: &[],
            values: &[("name", "exact")],
            columns: &[("name", "text")],
            row: vec![text_cell("RV_CELL_SOUTH")],
            question: "What name did the south review desk post?",
            category: "metric",
            status: "fully_approved",
            dual: "not_declared",
            unresolved: None,
            annotation: None,
            control: None,
        }),
        rows_question(Rows {
            id: "rv-scalar",
            role: ROLE,
            group: "north-scalar",
            as_of: AS_OF,
            kind: "scalar",
            ordered: true,
            sql: SQL_SCALAR,
            keys: &[],
            values: &[("qty", "exact")],
            columns: &[("qty", "int64")],
            row: vec![int_cell("11")],
            question: "What quantity did the north review desk post?",
            category: "metric",
            status: "fully_approved",
            dual: "not_declared",
            unresolved: None,
            annotation: None,
            control: None,
        }),
        rows_question(Rows {
            id: "rv-topk",
            role: ROLE,
            group: "north-topk",
            as_of: AS_OF,
            kind: "top_k",
            ordered: true,
            sql: SQL_TOPK,
            keys: &["name"],
            values: &[("rank", "exact")],
            columns: &[("name", "text"), ("rank", "int64")],
            row: vec![text_cell("RV_CELL_TOPK"), int_cell("3")],
            question: "Which north review receipt ranks first?",
            category: "top_k",
            status: "fully_approved",
            dual: "not_declared",
            unresolved: None,
            annotation: None,
            control: None,
        }),
        not_found_question("rv-absent", SQL_ABSENT),
        rows_question(Rows {
            id: "rv-note",
            role: ROLE,
            group: "north-note",
            as_of: AS_OF,
            kind: "table",
            ordered: true,
            sql: SQL_NOTE,
            keys: &[],
            values: &[("note", "exact")],
            columns: &[("note", "text")],
            row: vec![text_cell("RV_CELL_NOTE")],
            question: "Which north review note is still disputed?",
            category: "data_quality",
            status: "needs_reannotation",
            dual: "declared_dual_disagree",
            unresolved: Some("dual_disagree"),
            annotation: None,
            control: None,
        }),
    ])
}

fn refuse_bank() -> Value {
    let mut questions = vec![rows_question(Rows {
        id: "rv-ok",
        role: ROLE,
        group: "north-ok",
        as_of: AS_OF,
        kind: "table",
        ordered: true,
        sql: "select 1 /* RV_SQL_OK */",
        keys: &[],
        values: &[("n", "exact")],
        columns: &[("n", "text")],
        row: vec![text_cell("ok")],
        question: "Count the review fixture row.",
        category: "metric",
        status: "fully_approved",
        dual: "not_declared",
        unresolved: None,
        annotation: None,
        control: None,
    })];
    questions.push(shell(
        "rv-refuse",
        ROLE,
        "north-refuse",
        AS_OF,
        "May this role read the review total?",
        "permission",
        "fully_approved",
        "not_declared",
        json!({"gold_sql": "select 1 where false", "kind": "refuse_permission"}),
        None,
        None,
    ));
    bank_of(questions)
}

fn bad_decimal_document() -> String {
    let bank = bank_of(vec![shell(
        "rv-bad",
        ROLE,
        "north-bad",
        AS_OF,
        "What amount did the review fixture post?",
        "metric",
        "fully_approved",
        "not_declared",
        json!({
            "gold_sql": "select amount from review_receipts",
            "key_columns": [],
            "kind": "scalar",
            "ordered": true,
            "typed_gold": {
                "columns": [{"name": "amount", "type": "decimal"}],
                "completion": "complete",
                "kind": "rows",
                "rows": [[{
                    "coefficient": "0450",
                    "negative": false,
                    "presence": "value",
                    "scale": 2
                }]],
                "warnings": []
            },
            "value_columns": [{"name": "amount", "tolerance": "exact"}]
        }),
        None,
        None,
    )]);
    json!({
        "bank": bank,
        "executables": {
            "adapter": {"path": "/tmp/a286review/adapter", "sha256": "a".repeat(64)},
            "comparator": {"path": "/tmp/a286review/comparator", "sha256": "b".repeat(64)},
            "query_pg": {"path": "/tmp/a286review/query-pg", "sha256": "c".repeat(64)}
        },
        "k": 1,
        "manifest_sha256": "ab".repeat(32),
        "mode": "synthetic",
        "selection": {"as_of": AS_OF, "role": ROLE},
        "templates": {ROLE: {"binding_header": {}, "request": {}}},
        "trial_wall_ms": 30000,
        "tuple": tuple(ROLE),
        "version": "oo.benchmark.question-bank-suite/v1"
    })
    .to_string()
}

fn null_row(id: &str) -> Value {
    rows_question(Rows {
        id,
        role: ROLE,
        group: "north-null",
        as_of: AS_OF,
        kind: "table",
        ordered: true,
        sql: "select note from review_receipts where desk = 'north'",
        keys: &[],
        values: &[("n", "exact")],
        columns: &[("n", "text")],
        row: vec![json!({"presence": "null"})],
        question: "Which north review note was left blank?",
        category: "metric",
        status: "fully_approved",
        dual: "not_declared",
        unresolved: None,
        annotation: None,
        control: None,
    })
}

fn not_found_question(id: &str, sql: &str) -> Value {
    shell(
        id,
        ROLE,
        "north-absent",
        AS_OF,
        "Which review party names the absent desk?",
        "not_found",
        "fully_approved",
        "not_declared",
        json!({
            "gold_sql": sql,
            "kind": "not_found",
            "typed_gold": {"completion": "complete", "kind": "not_found", "warnings": []}
        }),
        None,
        None,
    )
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
        Value::Array(spec.keys.iter().copied().map(|text| json!(text)).collect()),
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
    shell(
        spec.id,
        spec.role,
        spec.group,
        spec.as_of,
        spec.question,
        spec.category,
        spec.status,
        spec.dual,
        Value::Object(expected),
        spec.annotation,
        spec.control,
    )
}

fn shell(
    id: &str,
    role: &str,
    group: &str,
    as_of: &str,
    question: &str,
    category: &str,
    status: &str,
    dual: &str,
    expected: Value,
    annotation: Option<&str>,
    control: Option<&str>,
) -> Value {
    json!({
        "accepted_interpretations": [],
        "annotations": match annotation {
            Some(text) => json!([{"label": "fixture_note", "text": text}]),
            None => json!([])
        },
        "as_of": as_of,
        "asker": {"claims": {}, "role": role},
        "category": [category],
        "control_totals": match control {
            Some(text) => json!([{"name": "row_count", "value": text}]),
            None => json!([])
        },
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

fn bank_of(questions: Vec<Value>) -> Value {
    json!({
        "bank_version": "public-fixture-1",
        "dataset": {"distribution": "public_synthetic", "id": DATASET, "scale": "xs"},
        "questions": questions,
        "suite": {"id": SUITE, "track": "E"},
        "version": "oo.benchmark.question-bank/v1"
    })
}

fn document(bank: &Value, manifest: &str, world: &World, role: &str, as_of: &str, k: u64) -> Result<String, String> {
    let mut roles = Vec::new();
    for question in bank["questions"].as_array().ok_or("questions")? {
        let found = question["asker"]["role"].as_str().ok_or("role")?.to_owned();
        if !roles.contains(&found) {
            roles.push(found);
        }
    }
    let mut templates = serde_json::Map::new();
    for found in roles {
        templates.insert(found.clone(), template(world, &found));
    }
    Ok(json!({
        "bank": bank,
        "executables": {
            "adapter": exe(&world.adapter)?,
            "comparator": exe(&world.comparator)?,
            "query_pg": exe(&world.query_pg)?
        },
        "k": k,
        "manifest_sha256": manifest,
        "mode": "synthetic",
        "selection": {"as_of": as_of, "role": role},
        "templates": templates,
        "trial_wall_ms": 30000,
        "tuple": tuple(role),
        "version": "oo.benchmark.question-bank-suite/v1"
    })
    .to_string())
}

fn tuple(role: &str) -> Value {
    json!({
        "as_of": "2026-10-06",
        "budget": "k2",
        "dataset": DATASET,
        "harness": "sql-runner",
        "model": "none",
        "project": "review",
        "role": role,
        "scorer_version": "oo.benchmark.sql-runner/v1",
        "snapshot": "fixture-a286",
        "suite": SUITE
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
            "audit_file": "/tmp/a286review/audit-placeholder",
            "ca_file": world.ca.display().to_string(),
            "catalog": {
                "large_table_row_threshold": 100000,
                "max_columns": 64,
                "max_functions": 4,
                "max_operators": 64,
                "max_tables": 8,
                "unknown_rows": "refuse"
            },
            "endpoint": {"database": "bench", "host": "db.example", "port": 5432, "user": "runner"},
            "identity": {
                "policy_id": "a".repeat(64),
                "project": "review",
                "role": format!("oo_r_{role}")
            },
            "password_file": world.password.display().to_string(),
            "policies": [{
                "config_digest": "b".repeat(64),
                "generation": 1,
                "login": "runner",
                "project": "review",
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
                "wall_clock_ms": 30000
            },
            "version": "oo.gateway.query-pg/v1"
        }
    })
}

fn exe(path: &str) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|err| format!("read {path}: {err}"))?;
    Ok(json!({"path": path, "sha256": oo_question_bank::sha256_hex(&bytes)}))
}

fn manifest_of(bank: &Value) -> Result<String, String> {
    let evaluation = oo_question_bank::evaluate(&serde_json::to_string(bank).map_err(|err| err.to_string())?)
        .map_err(|err| format!("evaluate: {}", err.stderr_line()))?;
    let report: Value = serde_json::from_str(&evaluation.report).map_err(|err| err.to_string())?;
    report["content_hashes"]["manifest"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "manifest hash missing".to_owned())
}

fn compile_ok(bin: &str, document: &str, out: &Path) -> Result<String, String> {
    let input = out.with_extension("input.json");
    if let Some(parent) = input.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(&input, document).map_err(|err| err.to_string())?;
    let output = Command::new(bin)
        .args([
            "--input",
            input.to_str().ok_or("input path")?,
            "--out-dir",
            out.to_str().ok_or("out path")?,
        ])
        .output()
        .map_err(|err| format!("spawn suite: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.code() != Some(0) || !stderr.is_empty() {
        return Err(format!(
            "compile code={:?} stderr={stderr} stdout={stdout}",
            output.status.code()
        ));
    }
    Ok(stdout)
}

fn expect_refuse(bin: &str, document: &str, out: &Path, token: &str, pointer: Option<&str>) -> Result<(), String> {
    if out.exists() {
        return Err(format!("{} already exists", out.display()));
    }
    let input = PathBuf::from(format!("{}.json", out.display()));
    fs::write(&input, document).map_err(|err| err.to_string())?;
    let output = Command::new(bin)
        .args([
            "--input",
            input.to_str().ok_or("input")?,
            "--out-dir",
            out.to_str().ok_or("out")?,
        ])
        .output()
        .map_err(|err| format!("spawn suite: {err}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.code() != Some(2) || !stdout.is_empty() {
        return Err(format!("refuse code={:?} stdout={stdout} stderr={stderr}", output.status.code()));
    }
    let line = stderr.trim_end();
    let mut parts = line.splitn(3, ' ');
    let prefix = parts.next().unwrap_or("");
    let got = parts.next().unwrap_or("");
    let rest = parts.next();
    if prefix != "question-bank-suite:" || got != token {
        return Err(format!("want token {token} got {stderr}"));
    }
    if let Some(pointer) = pointer {
        if rest != Some(pointer) {
            return Err(format!("want pointer {pointer} got {stderr}"));
        }
    }
    if out.exists() {
        return Err(format!("refuse created {}", out.display()));
    }
    Ok(())
}

fn write_responses(query_pg: &str) -> Result<(), String> {
    let path = Path::new(query_pg)
        .parent()
        .ok_or("query-pg parent")?
        .join("responses.json");
    let mut body = String::from("OOQRESP1\n5\n");
    push(&mut body, SQL_TABLE, &[row(&[gateway_text(CELL_TABLE), gateway_text("4.50")])]);
    push(&mut body, SQL_SCALAR, &[row(&[gateway_int("11")])]);
    push(&mut body, SQL_TOPK, &[row(&[gateway_text("RV_CELL_TOPK"), gateway_int("3")])]);
    push(&mut body, SQL_ABSENT, &[]);
    push(&mut body, SQL_NOTE, &[row(&[gateway_text("RV_CELL_NOTE")])]);
    fs::write(&path, body).map_err(|err| format!("responses: {err}"))
}

fn push(body: &mut String, sql: &str, rows: &[String]) {
    body.push_str(sql);
    body.push('\n');
    body.push_str(&rows.len().to_string());
    body.push('\n');
    for line in rows {
        body.push_str(line);
        body.push('\n');
    }
}

fn row(cells: &[Value]) -> String {
    serde_json::to_string(&json!({"cells": cells, "record": "row", "turn": 0})).unwrap_or_default()
}

fn text_cell(value: &str) -> Value {
    json!({"presence": "value", "value": value})
}

fn int_cell(value: &str) -> Value {
    json!({"presence": "value", "value": value})
}

fn gateway_text(value: &str) -> Value {
    json!({"kind": "text", "value": value})
}

fn gateway_int(value: &str) -> Value {
    json!({"kind": "int", "value": value})
}

fn decimal_cell(coefficient: &str, scale: i32) -> Value {
    json!({"coefficient": coefficient, "negative": false, "presence": "value", "scale": scale})
}

fn assert_header_preserved(suite: &Path, run: &Path, id: &str, empty: Option<&str>) -> Result<(), String> {
    let binding: Value = read_json(&suite.join(format!("questions/{id}/binding.json")))?;
    let text = fs::read_to_string(run.join(format!("case-{id}/trial-0/adapter.stdin")))
        .map_err(|err| format!("adapter stdin {id}: {err}"))?;
    let line = text.lines().next().ok_or("empty adapter stdin")?;
    let header: Value = serde_json::from_str(line).map_err(|err| format!("header {id}: {err}"))?;
    if header != binding {
        return Err(format!("runner changed the {id} header\nbinding={binding}\nheader={header}"));
    }
    match empty {
        Some(value) => eq(&header["empty_result"], &json!(value), "preserved empty_result")?,
        None => {
            if header.get("empty_result").is_some() {
                return Err(format!("{id} header gained empty_result"));
            }
        }
    }
    if line.contains("expected") || line.contains("RV_SQL_") || line.contains(PASSWORD) {
        return Err(format!("{id} header is gold driven: {line}"));
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("json {}: {err}", path.display()))
}

fn eq(actual: &Value, expected: &Value, name: &str) -> Result<(), String> {
    if actual != expected {
        return Err(format!("{name}: {actual} != {expected}"));
    }
    Ok(())
}

fn scan_secret(text: &str) -> Result<(), String> {
    if text.contains(PASSWORD) {
        return Err("password bytes in output".to_owned());
    }
    Ok(())
}

fn scan_tree(root: &Path) -> Result<(), String> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).map_err(|err| format!("read {}: {err}", dir.display()))? {
            let entry = entry.map_err(|err| err.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let bytes = fs::read(&path).map_err(|err| format!("read {}: {err}", path.display()))?;
            if bytes.windows(PASSWORD.len()).any(|window| window == PASSWORD.as_bytes()) {
                return Err(format!("password bytes in {}", path.display()));
            }
        }
    }
    Ok(())
}

fn env_path(name: &str) -> Result<String, String> {
    let value = std::env::var(name).map_err(|_| format!("{name} unset"))?;
    if !Path::new(&value).is_file() {
        return Err(format!("{name} is not a file: {value}"));
    }
    Ok(value)
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}
