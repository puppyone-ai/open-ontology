//! Independent driver for the sealed question-bank binary.
//! A260-PASS is the review marker. It is not part of the product tree.
//! This file does not score a bank, invent a human annotator, or accept 300 questions.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const ALPHABET_SHA: &str = "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";
const PROTOCOL: &str = "oo.benchmark.question-bank/v1";
const BANK_VERSION: &str = "a260-public-1";
const AS_OF_FINANCE: &str = "2026-10-06T08:00:00+08:00";
const AS_OF_DISPATCH: &str = "2026-10-06T09:00:00+08:00";
const AS_OF_GAP: &str = "2026-10-05T16:00:00-04:00";
const GOLD_SQL: &str = "select count(*) as n from public_receipt where lane = 'east' /* A260_GOLD_SQL_CANARY */";
const GOLD_SQL_DISPATCH: &str = "select count(*) as n from public_receipt where lane = 'west'";
const CELL: &str = "A260_CELL_CANARY";
const TOTAL: &str = "A260_TOTAL_CANARY";
const NOTE: &str = "A260_NOTE_CANARY";
const CANARIES: [&str; 4] = [
    "A260_GOLD_SQL_CANARY",
    "A260_CELL_CANARY",
    "A260_TOTAL_CANARY",
    "A260_NOTE_CANARY",
];
const FORBIDDEN: [&str; 14] = [
    "accepted_interpretations",
    "annotations",
    "control_totals",
    "expected",
    "gold_sql",
    "key_columns",
    "ordered",
    "provenance",
    "review",
    "role_group",
    "tolerance",
    "typed_gold",
    "unresolved",
    "value_columns",
];

fn main() {
    if let Err(err) = run() {
        eprintln!("drive: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    check_sha256()?;
    let dir = proof_dir()?;
    fs::create_dir_all(&dir).map_err(|err| format!("mkdir: {err}"))?;
    hash_binary(&dir)?;

    let hand = hand_bank();
    let hand_bytes = emit(&hand, true);
    let report = expect_report("hand", &[], hand_bytes.as_bytes(), 1, &dir)?;
    check_hand(&report)?;
    let again = expect_report("hand-repeat", &[], hand_bytes.as_bytes(), 1, &dir)?;
    if again != report {
        return Err("repeat stdout differed".to_owned());
    }
    let reversed = emit(&hand, false);
    if reversed == hand_bytes {
        return Err("reversed key order matched sorted input".to_owned());
    }
    let keyed = expect_report("hand-keyorder", &[], reversed.as_bytes(), 1, &dir)?;
    if keyed != report {
        return Err("key order changed the report".to_owned());
    }

    let (agent, manifest, scorer) = hashes_of(&report)?;
    let reordered = swap_questions(&hand)?;
    let reordered_report = expect_report("reorder", &[], emit(&reordered, true).as_bytes(), 1, &dir)?;
    let (agent_r, manifest_r, scorer_r) = hashes_of(&reordered_report)?;
    if agent_r == agent || manifest_r == manifest || scorer_r == scorer {
        return Err("reorder left a hash unchanged".to_owned());
    }
    let ids = manifest_ids(&reordered_report)?;
    if ids != ["lane-dispatch", "lane-finance", "lane-gap"] {
        return Err(format!("reorder ids {ids:?}"));
    }

    let mut scaled = hand.clone();
    set_path(&mut scaled, &["dataset", "scale"], Json::from("s"))?;
    let scaled_report = expect_report("scale", &[], emit(&scaled, true).as_bytes(), 1, &dir)?;
    let (agent_s, manifest_s, scorer_s) = hashes_of(&scaled_report)?;
    if agent_s != agent || scorer_s != scorer || manifest_s == manifest {
        return Err("scale did not move only the manifest hash".to_owned());
    }
    if get_str(&parse_report(&scaled_report)?, &["manifest", "scale"])? != "s" {
        return Err("scale label was not s".to_owned());
    }

    let mut gold = hand.clone();
    set_path(
        &mut gold,
        &["questions", "0", "expected", "gold_sql"],
        Json::from("select count(*) as n from public_receipt where lane = 'east' and 1 = 1"),
    )?;
    let gold_report = expect_report("gold-sql", &[], emit(&gold, true).as_bytes(), 1, &dir)?;
    let (agent_g, manifest_g, scorer_g) = hashes_of(&gold_report)?;
    if agent_g != agent || scorer_g == scorer || manifest_g == manifest {
        return Err("gold sql did not move scorer and manifest only".to_owned());
    }

    let mut text = hand.clone();
    set_path(
        &mut text,
        &["questions", "0", "question"],
        Json::from("How many public synthetic receipts cleared on the east lane today?"),
    )?;
    let text_report = expect_report("question-text", &[], emit(&text, true).as_bytes(), 1, &dir)?;
    let (agent_t, manifest_t, scorer_t) = hashes_of(&text_report)?;
    if agent_t == agent || manifest_t == manifest || scorer_t == scorer {
        return Err("question text did not move every view that stores it".to_owned());
    }

    let mut version = hand.clone();
    set_path(
        &mut version,
        &["questions", "0", "version"],
        Json::from("a260-other-1"),
    )?;
    expect_config(
        "version",
        &[],
        emit(&version, true).as_bytes(),
        "version",
        Some("/questions/0/version"),
        &dir,
    )?;

    let mut as_z = hand.clone();
    set_path(
        &mut as_z,
        &["questions", "0", "as_of"],
        Json::from("2026-10-06T00:00:00Z"),
    )?;
    expect_config(
        "as-of-z",
        &[],
        emit(&as_z, true).as_bytes(),
        "as_of",
        Some("/questions/0/as_of"),
        &dir,
    )?;
    let mut as_zero = hand.clone();
    set_path(
        &mut as_zero,
        &["questions", "0", "as_of"],
        Json::from("2026-10-06T00:00:00+00:00"),
    )?;
    expect_config(
        "as-of-zero",
        &[],
        emit(&as_zero, true).as_bytes(),
        "as_of",
        Some("/questions/0/as_of"),
        &dir,
    )?;

    let mut same_role = hand.clone();
    set_path(
        &mut same_role,
        &["questions", "1", "asker", "role"],
        Json::from("finance_reader"),
    )?;
    expect_config(
        "same-role",
        &[],
        emit(&same_role, true).as_bytes(),
        "schema",
        Some("/questions/1/asker/role"),
        &dir,
    )?;

    let mut same_expected = hand.clone();
    let finance_expected = get_path(&hand, &["questions", "0", "expected"])?.clone();
    set_path(&mut same_expected, &["questions", "1", "expected"], finance_expected)?;
    expect_config(
        "same-expected",
        &[],
        emit(&same_expected, true).as_bytes(),
        "role",
        Some("/questions/1/expected"),
        &dir,
    )?;

    let mut customer = hand.clone();
    set_path(
        &mut customer,
        &["dataset", "distribution"],
        Json::from("customer"),
    )?;
    expect_config(
        "customer-distribution",
        &[],
        emit(&customer, true).as_bytes(),
        "schema",
        Some("/dataset/distribution"),
        &dir,
    )?;
    let mut private_dist = hand.clone();
    set_path(
        &mut private_dist,
        &["dataset", "distribution"],
        Json::from("private"),
    )?;
    expect_config(
        "private-distribution",
        &[],
        emit(&private_dist, true).as_bytes(),
        "schema",
        Some("/dataset/distribution"),
        &dir,
    )?;
    let mut holdout = hand.clone();
    set_path(&mut holdout, &["questions", "0", "split"], Json::from("holdout"))?;
    expect_config(
        "holdout-split",
        &[],
        emit(&holdout, true).as_bytes(),
        "schema",
        Some("/questions/0/split"),
        &dir,
    )?;
    let mut private_id = hand.clone();
    set_path(&mut private_id, &["dataset", "id"], Json::from("private"))?;
    expect_config(
        "private-id",
        &[],
        emit(&private_id, true).as_bytes(),
        "schema",
        Some("/dataset/id"),
        &dir,
    )?;
    let mut holdout_suite = hand.clone();
    set_path(&mut holdout_suite, &["suite", "id"], Json::from("holdout"))?;
    expect_config(
        "holdout-suite",
        &[],
        emit(&holdout_suite, true).as_bytes(),
        "schema",
        Some("/suite/id"),
        &dir,
    )?;
    let mut customer_q = hand.clone();
    set_path(&mut customer_q, &["questions", "0", "id"], Json::from("customer"))?;
    expect_config(
        "customer-question",
        &[],
        emit(&customer_q, true).as_bytes(),
        "schema",
        Some("/questions/0/id"),
        &dir,
    )?;
    let mut annotator = hand.clone();
    set_path(
        &mut annotator,
        &["questions", "0", "provenance", "annotator"],
        Json::from("li"),
    )?;
    expect_config(
        "human-annotator",
        &[],
        emit(&annotator, true).as_bytes(),
        "schema",
        Some("/questions/0/provenance/annotator"),
        &dir,
    )?;
    let mut reviewer = hand.clone();
    set_path(
        &mut reviewer,
        &["questions", "0", "provenance", "reviewed_by"],
        Json::from("ada"),
    )?;
    expect_config(
        "human-reviewer",
        &[],
        emit(&reviewer, true).as_bytes(),
        "schema",
        Some("/questions/0/provenance/reviewed_by"),
        &dir,
    )?;

    let mut relative = hand.clone();
    set_path(
        &mut relative,
        &["questions", "0", "expected", "value_columns", "0", "tolerance"],
        Json::from("relative"),
    )?;
    expect_config(
        "relative",
        &[],
        emit(&relative, true).as_bytes(),
        "tolerance",
        Some("/questions/0/expected/value_columns/0/tolerance"),
        &dir,
    )?;
    let mut unresolved = hand.clone();
    set_path(
        &mut unresolved,
        &["questions", "0", "expected", "unresolved"],
        Json::from("missing_gold"),
    )?;
    expect_config(
        "unresolved-approved",
        &[],
        emit(&unresolved, true).as_bytes(),
        "schema",
        Some("/questions/0/expected/unresolved"),
        &dir,
    )?;

    expect_config(
        "bad-decimal",
        &[],
        emit(&typed_cell_bank("0120", false, "decimal")?, true).as_bytes(),
        "decimal",
        Some("/questions/0/expected/typed_gold/rows/0/0/coefficient"),
        &dir,
    )?;
    expect_config(
        "negative-zero",
        &[],
        emit(&typed_cell_bank("0", true, "decimal")?, true).as_bytes(),
        "decimal",
        Some("/questions/0/expected/typed_gold/rows/0/0/negative"),
        &dir,
    )?;
    expect_config(
        "bad-int64",
        &[],
        emit(&typed_cell_bank("01", false, "int64")?, true).as_bytes(),
        "int64",
        Some("/questions/0/expected/typed_gold/rows/0/0/value"),
        &dir,
    )?;

    let mut empty = shell(Json::Array(Vec::new()));
    expect_config(
        "empty-questions",
        &[],
        emit(&empty, true).as_bytes(),
        "schema",
        Some("/questions"),
        &dir,
    )?;
    let sixty_five = shell(Json::Array(vec![Json::Null; 65]));
    expect_config(
        "questions-65",
        &[],
        emit(&sixty_five, true).as_bytes(),
        "budget",
        Some("/questions"),
        &dir,
    )?;
    let three_hundred = shell(Json::Array(vec![Json::Null; 300]));
    expect_config(
        "questions-300",
        &[],
        emit(&three_hundred, true).as_bytes(),
        "budget",
        Some("/questions"),
        &dir,
    )?;

    let (payload, payload_id) = payload_bank(472)?;
    let payload_report = expect_report("payload", &[], emit(&payload, true).as_bytes(), 0, &dir)?;
    let payload_root = parse_report(&payload_report)?;
    if get_str(&payload_root, &["manifest", "question_count"])? != "1" {
        return Err("payload question_count".to_owned());
    }
    if get_str(&payload_root, &["manifest", "fully_approved_count"])? != "1" {
        return Err("payload fully_approved_count".to_owned());
    }
    if get_str(&payload_root, &["agent_view", "questions", "0", "id"])? != payload_id {
        return Err("payload identity id was dropped or rewritten".to_owned());
    }
    let (payload_plus, _) = payload_bank(473)?;
    expect_config(
        "payload-plus",
        &[],
        emit(&payload_plus, true).as_bytes(),
        "budget",
        Some("/"),
        &dir,
    )?;

    let help = "question-bank reads oo.benchmark.question-bank/v1 from stdin.\nIt writes oo.benchmark.question-bank-report/v1 to stdout.\nexit 0 fully approved labels\nexit 1 needs reannotation\nexit 2 config\n";
    expect_stdout("help", &["--help"], b"NOT-READ", 0, help.as_bytes(), b"", &dir)?;
    expect_stdout("help-short", &["-h"], b"NOT-READ", 0, help.as_bytes(), b"", &dir)?;
    expect_stdout(
        "usage",
        &["extra"],
        b"NOT-READ",
        2,
        b"",
        b"question-bank: usage\n",
        &dir,
    )?;
    let mut big = vec![b' '; 1_048_577];
    big[0] = b'{';
    expect_config("oversize", &[], &big, "budget", Some("/"), &dir)?;
    expect_config(
        "duplicate-key",
        &[],
        br#"{"version":"oo.benchmark.question-bank/v1","version":"oo.benchmark.question-bank/v1"}"#,
        "json",
        None,
        &dir,
    )?;

    let mut scope = File::create(dir.join("scope.txt")).map_err(|err| format!("scope: {err}"))?;
    writeln!(
        scope,
        "hand_exit=1 question_count=3 fully_approved=2 needs_reannotation=1\nagent={agent}\nmanifest={manifest}\nscorer={scorer}\nreorder_changed=agent,manifest,scorer\nscale_changed=manifest\ngold_sql_changed=scorer,manifest\nquestion_text_changed=agent,scorer,manifest\n300_questions=budget /questions\nhuman_annotator=schema /questions/0/provenance/annotator\nhuman_reviewer=schema /questions/0/provenance/reviewed_by\ndual_annotation=declared_dual_agree attestation=label_only\nnot_approved=300 two-human 20-percent-dual private-holdout customer B-D06 release\nmarker=A260-PASS"
    )
    .map_err(|err| format!("scope write: {err}"))?;
    let _ = empty;
    println!("drive cases passed");
    println!("agent={agent}");
    println!("manifest={manifest}");
    println!("scorer={scorer}");
    Ok(())
}

fn check_hand(report: &str) -> Result<(), String> {
    if !report.ends_with('\n') || report.matches('\n').count() != 1 {
        return Err("hand report is not one JSON line".to_owned());
    }
    let root = parse_report(report)?;
    if emit(&root.value, true) != root.raw {
        return Err(format!("report is not canonical: {}", first_diff(&emit(&root.value, true), &root.raw)));
    }
    for name in ["agent_view", "manifest", "scorer_view"] {
        let node = get_path(&root.value, &[name])?;
        let raw = match node {
            Json::Object(_) | Json::Array(_) | Json::String(_) | Json::Number(_) | Json::Bool(_) | Json::Null => {
                field_raw(&root.raw, name)?
            }
        };
        if emit(node, true) != raw {
            return Err(format!("{name} is not canonical: {}", first_diff(&emit(node, true), &raw)));
        }
        let digest = sha256_hex(raw.as_bytes());
        let stated = get_str(&root, &["content_hashes", name])?;
        if stated != digest {
            return Err(format!("{name} hash {stated} != {digest}"));
        }
    }
    let agent_hash = get_str(&root, &["content_hashes", "agent_view"])?;
    let scorer_hash = get_str(&root, &["content_hashes", "scorer_view"])?;
    if get_str(&root, &["manifest", "agent_view_sha256"])? != agent_hash {
        return Err("manifest agent hash mismatch".to_owned());
    }
    if get_str(&root, &["manifest", "scorer_view_sha256"])? != scorer_hash {
        return Err("manifest scorer hash mismatch".to_owned());
    }
    if get_str(&root, &["version"])? != "oo.benchmark.question-bank-report/v1" {
        return Err("report version".to_owned());
    }
    if get_str(&root, &["manifest", "version"])? != "oo.benchmark.question-bank-manifest/v1" {
        return Err("manifest version".to_owned());
    }
    if get_str(&root, &["agent_view", "version"])? != "oo.benchmark.question-bank-agent-view/v1" {
        return Err("agent version".to_owned());
    }
    if get_str(&root, &["scorer_view", "version"])? != "oo.benchmark.question-bank-scorer-view/v1" {
        return Err("scorer version".to_owned());
    }
    for key in [
        "bank_version",
        "dataset_id",
        "distribution",
        "scale",
        "suite_id",
        "track",
        "question_count",
        "fully_approved_count",
        "needs_reannotation_count",
    ] {
        let _ = get_str(&root, &["manifest", key])?;
    }
    if get_str(&root, &["manifest", "bank_version"])? != BANK_VERSION
        || get_str(&root, &["manifest", "dataset_id"])? != "a260-synth"
        || get_str(&root, &["manifest", "distribution"])? != "public_synthetic"
        || get_str(&root, &["manifest", "scale"])? != "xs"
        || get_str(&root, &["manifest", "suite_id"])? != "a260-suite"
        || get_str(&root, &["manifest", "track"])? != "D"
        || get_str(&root, &["manifest", "question_count"])? != "3"
        || get_str(&root, &["manifest", "fully_approved_count"])? != "2"
        || get_str(&root, &["manifest", "needs_reannotation_count"])? != "1"
        || get_str(&root, &["scorer_view", "question_count"])? != "3"
    {
        return Err("manifest identity or denominator".to_owned());
    }
    let ids = manifest_ids(report)?;
    if ids != ["lane-finance", "lane-dispatch", "lane-gap"] {
        return Err(format!("manifest order {ids:?}"));
    }
    if agent_ids(&root)? != ids || scorer_ids(&root)? != ids {
        return Err("view order diverged from the manifest".to_owned());
    }
    if get_str(&root, &["manifest", "questions", "2", "review_status"])? != "needs_reannotation" {
        return Err("gap was dropped from the denominator".to_owned());
    }
    if get_str(&root, &["scorer_view", "questions", "2", "review", "status"])? != "needs_reannotation"
        || get_str(&root, &["scorer_view", "questions", "2", "expected", "unresolved"])? != "missing_gold"
    {
        return Err("unresolved gold was rewritten".to_owned());
    }
    if get_str(&root, &["agent_view", "questions", "0", "as_of"])? != AS_OF_FINANCE
        || get_str(&root, &["agent_view", "questions", "1", "as_of"])? != AS_OF_DISPATCH
        || get_str(&root, &["agent_view", "questions", "2", "as_of"])? != AS_OF_GAP
        || get_str(&root, &["agent_view", "questions", "0", "version"])? != BANK_VERSION
        || get_str(&root, &["agent_view", "questions", "0", "asker", "role"])? != "finance_reader"
        || get_str(&root, &["agent_view", "questions", "1", "asker", "role"])? != "dispatch_reader"
    {
        return Err("agent as_of version or role".to_owned());
    }
    let agent_keys = object_keys(get_path(&root.value, &["agent_view", "questions", "0"])?)?;
    let expect_keys = [
        "as_of",
        "asker",
        "category",
        "difficulty",
        "id",
        "question",
        "required_metrics",
        "split",
        "version",
    ];
    if agent_keys != expect_keys {
        return Err(format!("agent question keys {agent_keys:?}"));
    }
    let agent_raw = field_raw(&root.raw, "agent_view")?;
    let scorer_raw = field_raw(&root.raw, "scorer_view")?;
    for canary in CANARIES {
        if agent_raw.contains(canary) {
            return Err(format!("agent view contains {canary}"));
        }
        if !scorer_raw.contains(canary) {
            return Err(format!("scorer view lacks {canary}"));
        }
    }
    let mut found = Vec::new();
    walk_keys(get_path(&root.value, &["agent_view"])?, &mut found);
    if !found.is_empty() {
        return Err(format!("forbidden agent keys {found:?}"));
    }
    if get_str(
        &root,
        &[
            "scorer_view",
            "questions",
            "0",
            "expected",
            "typed_gold",
            "rows",
            "0",
            "0",
            "coefficient",
        ],
    )? != "120"
        || get_str(
            &root,
            &[
                "scorer_view",
                "questions",
                "0",
                "expected",
                "typed_gold",
                "rows",
                "0",
                "0",
                "scale",
            ],
        )? != "2"
        || get_str(
            &root,
            &[
                "scorer_view",
                "questions",
                "0",
                "expected",
                "typed_gold",
                "completion",
            ],
        )? != "complete"
        || get_str(
            &root,
            &["scorer_view", "questions", "0", "expected", "typed_gold", "kind"],
        )? != "rows"
    {
        return Err("typed gold was rewritten".to_owned());
    }
    let gold_keys = object_keys(get_path(
        &root.value,
        &["scorer_view", "questions", "0", "expected", "typed_gold"],
    )?)?;
    if gold_keys != ["columns", "completion", "kind", "rows", "warnings"] {
        return Err(format!("typed gold keys {gold_keys:?}"));
    }
    if get_str(&root, &["scorer_view", "questions", "0", "review", "attestation"])? != "label_only"
        || get_str(&root, &["scorer_view", "questions", "0", "review", "dual_annotation"])? != "declared_dual_agree"
        || get_str(&root, &["manifest", "questions", "0", "dual_annotation"])? != "declared_dual_agree"
    {
        return Err("dual label was treated as a signature or dropped".to_owned());
    }
    if report.contains("\"li\"") || report.contains("\"ada\"") || report.contains("two human") {
        return Err("report contains a human owner".to_owned());
    }
    if get_str(&root, &["scorer_view", "questions", "0", "expected", "gold_sql"])?
        == get_str(&root, &["scorer_view", "questions", "1", "expected", "gold_sql"])?
    {
        return Err("role variants share an expected answer".to_owned());
    }
    Ok(())
}

fn hashes_of(report: &str) -> Result<(String, String, String), String> {
    let root = parse_report(report)?;
    Ok((
        get_str(&root, &["content_hashes", "agent_view"])?,
        get_str(&root, &["content_hashes", "manifest"])?,
        get_str(&root, &["content_hashes", "scorer_view"])?,
    ))
}

fn manifest_ids(report: &str) -> Result<Vec<String>, String> {
    let root = parse_report(report)?;
    ids_at(&root, &["manifest", "questions"])
}

fn agent_ids(root: &Node) -> Result<Vec<String>, String> {
    ids_at(root, &["agent_view", "questions"])
}

fn scorer_ids(root: &Node) -> Result<Vec<String>, String> {
    ids_at(root, &["scorer_view", "questions"])
}

fn ids_at(root: &Node, path: &[&str]) -> Result<Vec<String>, String> {
    let Json::Array(items) = get_path(&root.value, path)? else {
        return Err(format!("{path:?} is not an array"));
    };
    let mut ids = Vec::new();
    for item in items {
        ids.push(get_str_value(item, &["id"])?);
    }
    Ok(ids)
}

fn swap_questions(bank: &Json) -> Result<Json, String> {
    let mut out = bank.clone();
    let Json::Array(items) = get_path(&out, &["questions"])?.clone() else {
        return Err("questions".to_owned());
    };
    if items.len() != 3 {
        return Err("swap length".to_owned());
    }
    let swapped = Json::Array(vec![items[1].clone(), items[0].clone(), items[2].clone()]);
    set_path(&mut out, &["questions"], swapped)?;
    Ok(out)
}

fn hand_bank() -> Json {
    let finance = question(Question {
        id: "lane-finance",
        text: "How many public synthetic receipts cleared on the east lane?",
        role: "finance_reader",
        role_group: "lane-receipts",
        as_of: AS_OF_FINANCE,
        status: "fully_approved",
        dual: "declared_dual_agree",
        unresolved: None,
        gold_sql: Some(GOLD_SQL),
        ordered: Some(false),
        key_columns: Some(vec![]),
        value_columns: Some(vec![("amount", "exact"), ("note", "exact")]),
        typed: Some(rows_gold(
            &[("amount", "decimal"), ("note", "text")],
            &[vec![
                decimal_cell("120", false, 2),
                text_cell(CELL),
            ]],
        )),
        totals: vec![("receipt_n", TOTAL)],
        notes: vec![("public_note", NOTE)],
        claims: vec![("lane", vec!["east"])],
        category: vec!["single_table"],
        metrics: vec!["receipt_count"],
        difficulty: "easy",
    });
    let dispatch = question(Question {
        id: "lane-dispatch",
        text: "Which public synthetic lane may a dispatcher count?",
        role: "dispatch_reader",
        role_group: "lane-receipts",
        as_of: AS_OF_DISPATCH,
        status: "fully_approved",
        dual: "not_declared",
        unresolved: None,
        gold_sql: Some(GOLD_SQL_DISPATCH),
        ordered: Some(true),
        key_columns: Some(vec!["lane".to_owned()]),
        value_columns: Some(vec![("n", "exact")]),
        typed: Some(rows_gold(
            &[("lane", "text"), ("n", "decimal")],
            &[vec![text_cell("west"), decimal_cell("240", false, 2)]],
        )),
        totals: vec![],
        notes: vec![],
        claims: vec![],
        category: vec!["permission"],
        metrics: vec!["receipt_count"],
        difficulty: "medium",
    });
    let gap = question(Question {
        id: "lane-gap",
        text: "Which public synthetic receipt total is still missing gold?",
        role: "auditor",
        role_group: "lane-gap",
        as_of: AS_OF_GAP,
        status: "needs_reannotation",
        dual: "not_declared",
        unresolved: Some("missing_gold"),
        gold_sql: None,
        ordered: None,
        key_columns: None,
        value_columns: None,
        typed: None,
        totals: vec![],
        notes: vec![],
        claims: vec![],
        category: vec!["data_quality"],
        metrics: vec![],
        difficulty: "hard",
    });
    shell(Json::Array(vec![finance, dispatch, gap]))
}

struct Question<'a> {
    id: &'a str,
    text: &'a str,
    role: &'a str,
    role_group: &'a str,
    as_of: &'a str,
    status: &'a str,
    dual: &'a str,
    unresolved: Option<&'a str>,
    gold_sql: Option<&'a str>,
    ordered: Option<bool>,
    key_columns: Option<Vec<String>>,
    value_columns: Option<Vec<(&'a str, &'a str)>>,
    typed: Option<Json>,
    totals: Vec<(&'a str, &'a str)>,
    notes: Vec<(&'a str, &'a str)>,
    claims: Vec<(&'a str, Vec<&'a str>)>,
    category: Vec<&'a str>,
    metrics: Vec<&'a str>,
    difficulty: &'a str,
}

fn question(q: Question<'_>) -> Json {
    let mut expected = vec![("kind", Json::from("table"))];
    if let Some(sql) = q.gold_sql {
        expected.push(("gold_sql", Json::from(sql)));
    }
    if let Some(ordered) = q.ordered {
        expected.push(("ordered", Json::Bool(ordered)));
    }
    if let Some(keys) = &q.key_columns {
        expected.push(("key_columns", Json::Array(keys.iter().cloned().map(Json::from).collect())));
    }
    if let Some(columns) = &q.value_columns {
        expected.push((
            "value_columns",
            Json::Array(
                columns
                    .iter()
                    .map(|(name, tolerance)| {
                        obj(&[("name", Json::from(*name)), ("tolerance", Json::from(*tolerance))])
                    })
                    .collect(),
            ),
        ));
    }
    if let Some(typed) = q.typed {
        expected.push(("typed_gold", typed));
    }
    if let Some(reason) = q.unresolved {
        expected.push(("unresolved", Json::from(reason)));
    }
    let claims = if q.claims.is_empty() {
        Json::Object(Vec::new())
    } else {
        Json::Object(
            q.claims
                .into_iter()
                .map(|(key, values)| {
                    (
                        key.to_owned(),
                        Json::Array(values.into_iter().map(Json::from).collect()),
                    )
                })
                .collect(),
        )
    };
    obj(&[
        ("id", Json::from(q.id)),
        ("question", Json::from(q.text)),
        (
            "asker",
            obj(&[("role", Json::from(q.role)), ("claims", claims)]),
        ),
        (
            "category",
            Json::Array(q.category.into_iter().map(Json::from).collect()),
        ),
        ("difficulty", Json::from(q.difficulty)),
        ("expected", Json::Object(pairs(expected))),
        (
            "required_metrics",
            Json::Array(q.metrics.into_iter().map(Json::from).collect()),
        ),
        ("as_of", Json::from(q.as_of)),
        (
            "accepted_interpretations",
            Json::Array(Vec::new()),
        ),
        (
            "provenance",
            obj(&[
                ("origin", Json::from("public_synthetic_fixture")),
                ("annotator", Json::from("unassigned")),
                ("reviewed_by", Json::from("unassigned")),
            ]),
        ),
        ("split", Json::from("public")),
        ("version", Json::from(BANK_VERSION)),
        ("role_group", Json::from(q.role_group)),
        (
            "review",
            obj(&[
                ("attestation", Json::from("label_only")),
                ("status", Json::from(q.status)),
                ("dual_annotation", Json::from(q.dual)),
            ]),
        ),
        (
            "control_totals",
            Json::Array(
                q.totals
                    .into_iter()
                    .map(|(name, value)| obj(&[("name", Json::from(name)), ("value", Json::from(value))]))
                    .collect(),
            ),
        ),
        (
            "annotations",
            Json::Array(
                q.notes
                    .into_iter()
                    .map(|(label, text)| obj(&[("label", Json::from(label)), ("text", Json::from(text))]))
                    .collect(),
            ),
        ),
    ])
}

fn shell(questions: Json) -> Json {
    obj(&[
        ("version", Json::from(PROTOCOL)),
        ("bank_version", Json::from(BANK_VERSION)),
        (
            "dataset",
            obj(&[
                ("id", Json::from("a260-synth")),
                ("distribution", Json::from("public_synthetic")),
                ("scale", Json::from("xs")),
            ]),
        ),
        (
            "suite",
            obj(&[("id", Json::from("a260-suite")), ("track", Json::from("D"))]),
        ),
        ("questions", questions),
    ])
}

fn rows_gold(columns: &[(&str, &str)], rows: &[Vec<Json>]) -> Json {
    obj(&[
        (
            "columns",
            Json::Array(
                columns
                    .iter()
                    .map(|(name, ty)| obj(&[("name", Json::from(*name)), ("type", Json::from(*ty))]))
                    .collect(),
            ),
        ),
        ("completion", Json::from("complete")),
        ("kind", Json::from("rows")),
        (
            "rows",
            Json::Array(rows.iter().cloned().map(Json::Array).collect()),
        ),
        ("warnings", Json::Array(Vec::new())),
    ])
}

fn decimal_cell(coefficient: &str, negative: bool, scale: i64) -> Json {
    obj(&[
        ("presence", Json::from("value")),
        ("negative", Json::Bool(negative)),
        ("coefficient", Json::from(coefficient)),
        ("scale", Json::Number(scale.to_string())),
    ])
}

fn text_cell(text: &str) -> Json {
    obj(&[("presence", Json::from("value")), ("value", Json::from(text))])
}

fn typed_cell_bank(spelling: &str, negative: bool, ty: &str) -> Result<Json, String> {
    let cell = if ty == "decimal" {
        decimal_cell(spelling, negative, 2)
    } else {
        obj(&[
            ("presence", Json::from("value")),
            ("value", Json::from(spelling)),
        ])
    };
    let mut bank = shell(Json::Array(vec![question(Question {
        id: "lane-finance",
        text: "How many public synthetic receipts cleared on the east lane?",
        role: "finance_reader",
        role_group: "lane-receipts",
        as_of: AS_OF_FINANCE,
        status: "fully_approved",
        dual: "not_declared",
        unresolved: None,
        gold_sql: Some("select n from public_receipt"),
        ordered: Some(true),
        key_columns: Some(vec![]),
        value_columns: Some(vec![("n", "exact")]),
        typed: Some(rows_gold(&[("n", ty)], &[vec![cell]])),
        totals: vec![],
        notes: vec![],
        claims: vec![],
        category: vec!["metric"],
        metrics: vec![],
        difficulty: "easy",
    })]));
    let _ = &mut bank;
    Ok(bank)
}

fn payload_bank(last_len: usize) -> Result<(Json, String), String> {
    let piece = "a".repeat(64);
    let bank_version = format!("b{}", "1".repeat(63));
    let dataset_id = format!("d{}", "a".repeat(63));
    let suite_id = format!("s{}", "a".repeat(63));
    let question_id = format!("q{}", "0".repeat(63));
    let role = format!("r{}", "_".repeat(63));
    let role_group = format!("g{}", "a".repeat(63));
    let mut rows = Vec::new();
    for index in 0..27 {
        let text = if index == 26 {
            "t".repeat(last_len)
        } else {
            "t".repeat(500)
        };
        rows.push(vec![text_cell(&text)]);
    }
    let claims = Json::Object(vec![
        (
            "lane".to_owned(),
            Json::Array(vec![Json::from(piece.clone()), Json::from(piece.clone())]),
        ),
        (
            "region".to_owned(),
            Json::Array(vec![
                Json::from(piece.clone()),
                Json::from(piece.clone()),
                Json::from(piece.clone()),
                Json::from(piece),
            ]),
        ),
    ]);
    let one = obj(&[
        ("id", Json::from(question_id.clone())),
        ("question", Json::from("q".repeat(500))),
        ("asker", obj(&[("role", Json::from(role)), ("claims", claims)])),
        ("category", Json::Array(vec![Json::from("metric")])),
        ("difficulty", Json::from("easy")),
        (
            "expected",
            obj(&[
                ("kind", Json::from("table")),
                ("gold_sql", Json::from("s".repeat(2000))),
                ("ordered", Json::Bool(true)),
                ("key_columns", Json::Array(Vec::new())),
                (
                    "value_columns",
                    Json::Array(vec![obj(&[
                        ("name", Json::from("c")),
                        ("tolerance", Json::from("exact")),
                    ])]),
                ),
                (
                    "typed_gold",
                    rows_gold(&[("c", "text")], &rows),
                ),
            ]),
        ),
        ("required_metrics", Json::Array(Vec::new())),
        ("as_of", Json::from("2026-10-01T00:00:00+08:00")),
        ("accepted_interpretations", Json::Array(Vec::new())),
        (
            "provenance",
            obj(&[
                ("origin", Json::from("public_synthetic_fixture")),
                ("annotator", Json::from("unassigned")),
                ("reviewed_by", Json::from("unassigned")),
            ]),
        ),
        ("split", Json::from("public")),
        ("version", Json::from(bank_version.clone())),
        ("role_group", Json::from(role_group)),
        (
            "review",
            obj(&[
                ("attestation", Json::from("label_only")),
                ("status", Json::from("fully_approved")),
                ("dual_annotation", Json::from("not_declared")),
            ]),
        ),
        ("control_totals", Json::Array(Vec::new())),
        ("annotations", Json::Array(Vec::new())),
    ]);
    let bank = obj(&[
        ("version", Json::from(PROTOCOL)),
        ("bank_version", Json::from(bank_version)),
        (
            "dataset",
            obj(&[
                ("id", Json::from(dataset_id)),
                ("distribution", Json::from("public_synthetic")),
                ("scale", Json::from("xs")),
            ]),
        ),
        (
            "suite",
            obj(&[("id", Json::from(suite_id)), ("track", Json::from("E"))]),
        ),
        ("questions", Json::Array(vec![one])),
    ]);
    Ok((bank, question_id))
}

#[derive(Clone, Debug)]
enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn from(text: impl Into<String>) -> Self {
        Self::String(text.into())
    }
}

struct Node {
    value: Json,
    raw: String,
}

fn obj(pairs_in: &[(&str, Json)]) -> Json {
    Json::Object(pairs(pairs_in.iter().map(|(key, value)| (*key, value.clone())).collect()))
}

fn pairs(items: Vec<(&str, Json)>) -> Vec<(String, Json)> {
    items.into_iter().map(|(key, value)| (key.to_owned(), value)).collect()
}

fn emit(value: &Json, sort: bool) -> String {
    let mut out = String::new();
    emit_into(value, sort, &mut out);
    out
}

fn emit_into(value: &Json, sort: bool, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(text) => out.push_str(text),
        Json::String(text) => push_string(out, text),
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                emit_into(item, sort, out);
            }
            out.push(']');
        }
        Json::Object(fields) => {
            let mut owned = fields.clone();
            if sort {
                owned.sort_by(|left, right| left.0.cmp(&right.0));
            } else {
                owned.reverse();
            }
            out.push('{');
            for (index, (key, child)) in owned.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                push_string(out, key);
                out.push(':');
                emit_into(child, sort, out);
            }
            out.push('}');
        }
    }
}

fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", other as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

fn parse_report(report: &str) -> Result<Node, String> {
    let body = report.strip_suffix('\n').unwrap_or(report);
    let mut parser = Parser { s: body.as_bytes(), i: 0 };
    let node = parser.parse_value()?;
    parser.skip();
    if parser.i != parser.s.len() {
        return Err("trailing report bytes".to_owned());
    }
    Ok(node)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn parse_value(&mut self) -> Result<Node, String> {
        self.skip();
        let start = self.i;
        let value = self.parse_inner()?;
        let raw = std::str::from_utf8(&self.s[start..self.i])
            .map_err(|_| "raw utf8".to_owned())?
            .to_owned();
        Ok(Node { value, raw })
    }

    fn parse_inner(&mut self) -> Result<Json, String> {
        if self.i >= self.s.len() {
            return Err("eof".to_owned());
        }
        match self.s[self.i] {
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b'"' => Ok(Json::String(self.parse_string()?)),
            b't' => self.eat(b"true", Json::Bool(true)),
            b'f' => self.eat(b"false", Json::Bool(false)),
            b'n' => self.eat(b"null", Json::Null),
            b'-' | b'0'..=b'9' => Ok(Json::Number(self.parse_number()?)),
            other => Err(format!("json byte {other}")),
        }
    }

    fn parse_object(&mut self) -> Result<Json, String> {
        self.i += 1;
        let mut fields = Vec::new();
        self.skip();
        if self.peek(b'}') {
            self.i += 1;
            return Ok(Json::Object(fields));
        }
        loop {
            self.skip();
            if self.i >= self.s.len() || self.s[self.i] != b'"' {
                return Err("object key".to_owned());
            }
            let key = self.parse_string()?;
            self.skip();
            self.expect(b':')?;
            let node = self.parse_value()?;
            if fields.iter().any(|(have, _)| have == &key) {
                return Err(format!("duplicate key {key}"));
            }
            fields.push((key, node.value));
            self.skip();
            if self.peek(b',') {
                self.i += 1;
                continue;
            }
            self.expect(b'}')?;
            break;
        }
        Ok(Json::Object(fields))
    }

    fn parse_array(&mut self) -> Result<Json, String> {
        self.i += 1;
        let mut items = Vec::new();
        self.skip();
        if self.peek(b']') {
            self.i += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.parse_value()?.value);
            self.skip();
            if self.peek(b',') {
                self.i += 1;
                continue;
            }
            self.expect(b']')?;
            break;
        }
        Ok(Json::Array(items))
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        while self.i < self.s.len() {
            let byte = self.s[self.i];
            if byte == b'"' {
                self.i += 1;
                return Ok(out);
            }
            if byte == b'\\' {
                self.i += 1;
                let esc = self.bump()?;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let mut hex = [0u8; 4];
                        for slot in &mut hex {
                            *slot = self.bump()?;
                        }
                        let text = std::str::from_utf8(&hex).map_err(|_| "hex".to_owned())?;
                        let code = u32::from_str_radix(text, 16).map_err(|_| "hex".to_owned())?;
                        out.push(char::from_u32(code).ok_or("bad unicode")?);
                    }
                    _ => return Err("bad escape".to_owned()),
                }
                continue;
            }
            if byte < 0x20 {
                return Err("raw control".to_owned());
            }
            let width = utf8_width(byte)?;
            if self.i + width > self.s.len() {
                return Err("utf8".to_owned());
            }
            let text = std::str::from_utf8(&self.s[self.i..self.i + width]).map_err(|_| "utf8".to_owned())?;
            out.push_str(text);
            self.i += width;
        }
        Err("unterminated string".to_owned())
    }

    fn parse_number(&mut self) -> Result<String, String> {
        let start = self.i;
        if self.peek(b'-') {
            self.i += 1;
        }
        if self.i >= self.s.len() || !self.s[self.i].is_ascii_digit() {
            return Err("number".to_owned());
        }
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i])
            .map(|text| text.to_owned())
            .map_err(|_| "number utf8".to_owned())
    }

    fn eat(&mut self, literal: &[u8], value: Json) -> Result<Json, String> {
        if self.s.get(self.i..self.i + literal.len()) != Some(literal) {
            return Err("literal".to_owned());
        }
        self.i += literal.len();
        Ok(value)
    }

    fn skip(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&self, byte: u8) -> bool {
        self.s.get(self.i) == Some(&byte)
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek(byte) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected {byte}"))
        }
    }

    fn bump(&mut self) -> Result<u8, String> {
        let byte = *self.s.get(self.i).ok_or("eof")?;
        self.i += 1;
        Ok(byte)
    }
}

fn utf8_width(byte: u8) -> Result<usize, String> {
    if byte < 0x80 {
        Ok(1)
    } else if byte & 0xe0 == 0xc0 {
        Ok(2)
    } else if byte & 0xf0 == 0xe0 {
        Ok(3)
    } else if byte & 0xf8 == 0xf0 {
        Ok(4)
    } else {
        Err("utf8 lead".to_owned())
    }
}

fn field_raw(raw_object: &str, key: &str) -> Result<String, String> {
    let node = parse_report(&format!("{raw_object}\n"))?;
    let Json::Object(fields) = &node.value else {
        return Err("field raw object".to_owned());
    };
    let mut parser = Parser { s: raw_object.as_bytes(), i: 0 };
    parser.expect(b'{')?;
    loop {
        parser.skip();
        let have = parser.parse_string()?;
        parser.skip();
        parser.expect(b':')?;
        let value = parser.parse_value()?;
        if have == key {
            return Ok(value.raw);
        }
        if !fields.iter().any(|(name, _)| name == &have) {
            return Err(format!("missing field {key}"));
        }
        parser.skip();
        if parser.peek(b',') {
            parser.i += 1;
            continue;
        }
        break;
    }
    Err(format!("missing field {key}"))
}

fn get_path<'a>(value: &'a Json, path: &[&str]) -> Result<&'a Json, String> {
    let mut current = value;
    for (index, key) in path.iter().enumerate() {
        if key.bytes().all(|byte| byte.is_ascii_digit()) && !key.is_empty() {
            let Json::Array(items) = current else {
                return Err(format!("not array at {path:?}"));
            };
            let nth: usize = key.parse().map_err(|_| "index".to_owned())?;
            current = items.get(nth).ok_or_else(|| format!("index {nth}"))?;
            continue;
        }
        let Json::Object(fields) = current else {
            return Err(format!("not object at {} in {path:?}", path[..index].join("/")));
        };
        current = fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, child)| child)
            .ok_or_else(|| format!("missing {key}"))?;
    }
    Ok(current)
}

fn set_path(value: &mut Json, path: &[&str], next: Json) -> Result<(), String> {
    if path.is_empty() {
        *value = next;
        return Ok(());
    }
    let key = path[0];
    if key.bytes().all(|byte| byte.is_ascii_digit()) && !key.is_empty() {
        let Json::Array(items) = value else {
            return Err(format!("set not array {key}"));
        };
        let nth: usize = key.parse().map_err(|_| "index".to_owned())?;
        let child = items.get_mut(nth).ok_or_else(|| format!("set index {nth}"))?;
        return set_path(child, &path[1..], next);
    }
    let Json::Object(fields) = value else {
        return Err(format!("set not object {key}"));
    };
    if path.len() == 1 {
        if let Some((_, child)) = fields.iter_mut().find(|(name, _)| name == key) {
            *child = next;
        } else {
            fields.push((key.to_owned(), next));
        }
        return Ok(());
    }
    let child = fields
        .iter_mut()
        .find(|(name, _)| name == key)
        .map(|(_, child)| child)
        .ok_or_else(|| format!("set missing {key}"))?;
    set_path(child, &path[1..], next)
}

fn get_str(root: &Node, path: &[&str]) -> Result<String, String> {
    get_str_value(get_path(&root.value, path)?, &[])
}

fn get_str_value(value: &Json, path: &[&str]) -> Result<String, String> {
    let value = if path.is_empty() { value } else { get_path(value, path)? };
    match value {
        Json::String(text) | Json::Number(text) => Ok(text.clone()),
        _ => Err(format!("not a scalar at {path:?}")),
    }
}

fn object_keys(value: &Json) -> Result<Vec<String>, String> {
    let Json::Object(fields) = value else {
        return Err("keys".to_owned());
    };
    let mut keys: Vec<String> = fields.iter().map(|(key, _)| key.clone()).collect();
    keys.sort();
    Ok(keys)
}

fn walk_keys(value: &Json, found: &mut Vec<String>) {
    match value {
        Json::Object(fields) => {
            for (key, child) in fields {
                if FORBIDDEN.contains(&key.as_str()) {
                    found.push(key.clone());
                }
                walk_keys(child, found);
            }
        }
        Json::Array(items) => {
            for item in items {
                walk_keys(item, found);
            }
        }
        _ => {}
    }
}

struct Captured {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn expect_report(name: &str, args: &[&str], stdin: &[u8], code: i32, dir: &PathBuf) -> Result<String, String> {
    let captured = invoke(name, args, stdin, dir)?;
    if captured.code != code {
        return Err(format!(
            "{name} exit {} != {code} stderr {}",
            captured.code,
            String::from_utf8_lossy(&captured.stderr)
        ));
    }
    if !captured.stderr.is_empty() {
        return Err(format!("{name} stderr not empty"));
    }
    String::from_utf8(captured.stdout).map_err(|_| format!("{name} stdout utf8"))
}

fn expect_config(
    name: &str,
    args: &[&str],
    stdin: &[u8],
    token: &str,
    pointer: Option<&str>,
    dir: &PathBuf,
) -> Result<(), String> {
    let line = match pointer {
        Some(pointer) => format!("question-bank: {token} {pointer}\n"),
        None => format!("question-bank: {token}\n"),
    };
    expect_stdout(name, args, stdin, 2, b"", line.as_bytes(), dir)
}

fn expect_stdout(
    name: &str,
    args: &[&str],
    stdin: &[u8],
    code: i32,
    stdout: &[u8],
    stderr: &[u8],
    dir: &PathBuf,
) -> Result<(), String> {
    let captured = invoke(name, args, stdin, dir)?;
    if captured.code != code || captured.stdout != stdout || captured.stderr != stderr {
        return Err(format!(
            "{name} got exit {} stdout {} stderr {}",
            captured.code,
            String::from_utf8_lossy(&captured.stdout),
            String::from_utf8_lossy(&captured.stderr)
        ));
    }
    Ok(())
}

fn invoke(name: &str, args: &[&str], stdin: &[u8], dir: &PathBuf) -> Result<Captured, String> {
    let bin = env::var("OO_QUESTION_BANK").map_err(|_| "OO_QUESTION_BANK".to_owned())?;
    let mut child = Command::new(&bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn {name}: {err}"))?;
    {
        let mut pipe = child.stdin.take().ok_or("stdin")?;
        pipe.write_all(stdin).map_err(|err| format!("write {name}: {err}"))?;
    }
    let output = child.wait_with_output().map_err(|err| format!("wait {name}: {err}"))?;
    let code = output.status.code().ok_or_else(|| format!("{name} signal"))?;
    fs::write(dir.join(format!("{name}.exit")), format!("{code}\n")).map_err(|err| format!("save: {err}"))?;
    fs::write(dir.join(format!("{name}.stdout")), &output.stdout).map_err(|err| format!("save: {err}"))?;
    fs::write(dir.join(format!("{name}.stderr")), &output.stderr).map_err(|err| format!("save: {err}"))?;
    if stdin.len() <= 65_536 {
        fs::write(dir.join(format!("{name}.stdin")), stdin).map_err(|err| format!("save: {err}"))?;
    } else {
        fs::write(
            dir.join(format!("{name}.stdin.note")),
            format!("bytes={}\nsha256={}\n", stdin.len(), sha256_hex(stdin)),
        )
        .map_err(|err| format!("save: {err}"))?;
    }
    Ok(Captured { code, stdout: output.stdout, stderr: output.stderr })
}

fn proof_dir() -> Result<PathBuf, String> {
    env::var("OO_PROOF_DIR").map(PathBuf::from).map_err(|_| "OO_PROOF_DIR".to_owned())
}

fn hash_binary(dir: &PathBuf) -> Result<(), String> {
    let bin = env::var("OO_QUESTION_BANK").map_err(|_| "OO_QUESTION_BANK".to_owned())?;
    let bytes = fs::read(&bin).map_err(|err| format!("read bin: {err}"))?;
    fs::write(
        dir.join("bin-sha256.txt"),
        format!("{} bytes={} path={bin}\n", sha256_hex(&bytes), bytes.len()),
    )
    .map_err(|err| format!("bin hash: {err}"))?;
    Ok(())
}

fn first_diff(left: &str, right: &str) -> String {
    let n = left.len().min(right.len());
    let lb = left.as_bytes();
    let rb = right.as_bytes();
    for index in 0..n {
        if lb[index] != rb[index] {
            return format!("at {index} left={} right={} lens {} {}", lb[index], rb[index], left.len(), right.len());
        }
    }
    format!("prefix lens {} {}", left.len(), right.len())
}

fn check_sha256() -> Result<(), String> {
    if sha256_hex(b"") != EMPTY_SHA || sha256_hex(b"abc") != ABC_SHA {
        return Err("sha256 self-check failed".to_owned());
    }
    let alphabet = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    if sha256_hex(alphabet) != ALPHABET_SHA {
        return Err("sha256 alphabet self-check failed".to_owned());
    }
    Ok(())
}

fn sha256_hex(input: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let digest = sha256(input);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut state = [
        0x6a09_e667_u32,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bit_len = u64::try_from(input.len()).expect("len").checked_mul(8).expect("bits");
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut word = [0u32; 64];
        for (index, slot) in word.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *slot = u32::from_be_bytes(chunk[start..start + 4].try_into().expect("word"));
        }
        for index in 16..64 {
            let earlier = word[index - 15];
            let later = word[index - 2];
            let s0 = earlier.rotate_right(7) ^ earlier.rotate_right(18) ^ (earlier >> 3);
            let s1 = later.rotate_right(17) ^ later.rotate_right(19) ^ (later >> 10);
            word[index] = word[index - 16]
                .wrapping_add(s0)
                .wrapping_add(word[index - 7])
                .wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(word[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}
