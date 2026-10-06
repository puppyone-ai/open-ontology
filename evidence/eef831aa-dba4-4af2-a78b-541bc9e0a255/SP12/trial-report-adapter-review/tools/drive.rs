//! Independent review driver.
//!
//! Compiles the purpose-built stub, runs the archived sql-runner, feeds the
//! two real reports to the archived trial-report-adapter, then feeds that
//! document to the archived trial-statistics binary. This file is not a
//! product test. It does not open PostgreSQL and it does not score a model.

#![forbid(unsafe_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SECRET: &str = "pw-a255-secret";
const SNAPSHOT: &str = "declared-a255";
const SCORER: &str = "oo.benchmark.sql-runner/v1";
const OTHER_SCORER: &str = "oo.benchmark.other-scorer/v1";
const DIGEST: &str = "a255a255a255a255a255a255a255a255a255a255a255a255a255a255a255a255";

struct Tools {
    query: PathBuf,
    adapter: PathBuf,
    comparator: PathBuf,
    digest: String,
}

struct Paths {
    proof: PathBuf,
    scratch: PathBuf,
    sql_runner: PathBuf,
    statistics: PathBuf,
    adapter: PathBuf,
    stub_src: PathBuf,
    binding: PathBuf,
    gold_full: PathBuf,
    gold_miss: PathBuf,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("drive: {err}");
        if let Some(dir) = std::env::var_os("OO_PROOF_DIR") {
            let path = PathBuf::from(dir).join("WHY.txt");
            let _ = fs::write(path, format!("drive: {err}\n"));
        }
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    if std::env::var_os("PGPASSWORD").is_some() {
        return Err("PGPASSWORD must stay unset".to_owned());
    }
    check_sha256()?;
    self_check_drop()?;
    let paths = paths()?;
    fs::create_dir_all(&paths.proof).map_err(|err| format!("proof dir: {err}"))?;
    fs::write(
        paths.proof.join("scope.txt"),
        "postgres=not_used\nprivate_tls=not_claimed\npgpassword=unset\nsnapshot=declared_label\nmodel_lane=trusted_sql\n",
    )
    .map_err(|err| format!("scope: {err}"))?;
    if paths.scratch.exists() {
        fs::remove_dir_all(&paths.scratch).map_err(|err| format!("clear scratch: {err}"))?;
    }
    fs::create_dir_all(&paths.scratch).map_err(|err| format!("scratch: {err}"))?;
    write_secrets(&paths)?;
    let tools = compile_stub(&paths)?;
    hash_bins(&paths)?;

    let desk = Desk {
        scratch: &paths.scratch,
        binding: &paths.binding,
        gold_full: &paths.gold_full,
        gold_miss: &paths.gold_miss,
    };

    let (left, right) = k2(&paths, &tools, &desk)?;
    let paired = pair(&left, &right, "k2", "live-k2")?;
    let adapted = adapt_ok(&paths, "k2", &paired)?;
    expect_shape(
        &adapted,
        "left desk:pass,fail;gate:fail,fail;note:needs_reannotation,needs_reannotation;slow:fail,fail;drop:pass,pass\nright desk:pass,pass;gate:fail,fail;note:needs_reannotation,needs_reannotation;slow:pass,pass;drop:pass,fail",
    )?;
    expect_trusted(&adapted, "live-k2", "k2", &tools.digest)?;
    let stats = statistics(&paths, "k2", adapted.as_bytes(), 1)?;
    expect_stats(&stats, "failed", 5, 1, 2, &[1, 2])?;

    let (failed_left, failed_right) = allfail(&paths, &tools, &desk)?;
    let failed = pair(&failed_left, &failed_right, "k4", "live-allfail")?;
    let failed_input = adapt_ok(&paths, "allfail", &failed)?;
    expect_shape(
        &failed_input,
        "left desk:fail,fail,fail,fail\nright desk:fail,fail,fail,fail",
    )?;
    if failed_input.contains("\"status\":\"pass\"") || failed_input.contains("needs_reannotation") {
        return Err("allfail adapter output kept a pass or a reannotation".to_owned());
    }
    expect_trusted(&failed_input, "live-allfail", "k4", &tools.digest)?;
    let failed_stats = statistics(&paths, "allfail", failed_input.as_bytes(), 0)?;
    expect_stats(&failed_stats, "passed", 1, 0, 4, &[0, 0])?;
    fs::write(
        paths.proof.join("structural-gate.txt"),
        "\
allfail_statistics_exit=0
allfail_gate=passed
allfail_question_count=1
allfail_reannotation_count=0
allfail_trial_count=4
allfail_pass_k_count=0,0
meaning=structural
quality_winner=no
reason=The consumed trial-statistics gate is passed when reannotation_count is 0. Both pass_k_count values are 0. The word passed does not say either side won.
",
    )
    .map_err(|err| format!("structural gate: {err}"))?;

    refuse_tuple_order(&paths, &tools, &desk)?;
    refuse_budget(&paths, &tools, &desk)?;
    refuse_labels_and_scorer(&paths)?;
    refuse_missing(&paths, &tools, &desk)?;
    fs::write(paths.proof.join("preserved.txt"), "ok\n").map_err(|err| format!("preserved: {err}"))?;
    Ok(())
}

struct Desk<'a> {
    scratch: &'a Path,
    binding: &'a Path,
    gold_full: &'a Path,
    gold_miss: &'a Path,
}

fn paths() -> Result<Paths, String> {
    let fixtures = required("OO_FIXTURES")?;
    Ok(Paths {
        proof: required("OO_PROOF_DIR")?,
        scratch: required("OO_SCRATCH")?,
        sql_runner: required("OO_SQL_RUNNER")?,
        statistics: required("OO_TRIAL_STATISTICS")?,
        adapter: required("OO_ADAPTER")?,
        stub_src: required("OO_STUB")?,
        binding: fixtures.join("note-header.json"),
        gold_full: fixtures.join("gold-full.json"),
        gold_miss: PathBuf::from("/target/a255-scratch/gold-miss.json"),
    })
}

fn required(key: &str) -> Result<PathBuf, String> {
    let text = std::env::var(key).map_err(|_| format!("{key} is required"))?;
    let path = PathBuf::from(&text);
    if !path.is_absolute() {
        return Err(format!("{key} must be absolute"));
    }
    Ok(path)
}

fn compile_stub(paths: &Paths) -> Result<Tools, String> {
    let dir = PathBuf::from("/target/a255-bins");
    fs::create_dir_all(&dir).map_err(|err| format!("bin dir: {err}"))?;
    let stub = dir.join("stub");
    let output = Command::new("rustc")
        .args(["--edition", "2021", "-D", "unsafe_code", "-C", "debuginfo=0", "-o"])
        .arg(&stub)
        .arg(&paths.stub_src)
        .output()
        .map_err(|err| format!("rustc: {err}"))?;
    fs::write(paths.proof.join("stub-rustc.stdout"), &output.stdout).map_err(|err| err.to_string())?;
    fs::write(paths.proof.join("stub-rustc.stderr"), &output.stderr).map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "stub rustc: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let query = dir.join("query-pg");
    let adapter = dir.join("query-result-adapter");
    let comparator = dir.join("result-comparator");
    link_three(&stub, &query, &adapter, &comparator)?;
    let bytes = fs::read(&stub).map_err(|err| format!("stub bytes: {err}"))?;
    Ok(Tools {
        query,
        adapter,
        comparator,
        digest: hex(&sha256(&bytes)),
    })
}

fn link_three(stub: &Path, query: &Path, adapter: &Path, comparator: &Path) -> Result<(), String> {
    for path in [query, adapter, comparator] {
        let _ = fs::remove_file(path);
        fs::hard_link(stub, path).map_err(|err| format!("link {}: {err}", path.display()))?;
    }
    Ok(())
}

fn restore(tools: &Tools) -> Result<(), String> {
    let _ = fs::remove_file(&tools.query);
    fs::hard_link(&tools.adapter, &tools.query).map_err(|err| format!("restore query-pg: {err}"))
}

fn hash_bins(paths: &Paths) -> Result<(), String> {
    let mut body = String::new();
    for (name, path) in [
        ("sql-runner", &paths.sql_runner),
        ("trial-statistics", &paths.statistics),
        ("trial-report-adapter", &paths.adapter),
    ] {
        let bytes = fs::read(path).map_err(|err| format!("read {name}: {err}"))?;
        body.push_str(&format!(
            "{name} bytes={} sha256={} consumed_not_reviewed\n",
            bytes.len(),
            hex(&sha256(&bytes))
        ));
    }
    fs::write(paths.proof.join("bin-sha256.txt"), body).map_err(|err| err.to_string())
}

fn write_secrets(paths: &Paths) -> Result<(), String> {
    let password = paths.scratch.join("login.pass");
    let ca = paths.scratch.join("ca.crt");
    if password.display().to_string().contains(SECRET) {
        return Err("password path contains the secret".to_owned());
    }
    fs::write(&password, format!("{SECRET}\n")).map_err(|err| format!("password: {err}"))?;
    fs::write(
        &ca,
        "-----BEGIN CERTIFICATE-----\nMIIB-a255\n-----END CERTIFICATE-----\n",
    )
    .map_err(|err| format!("ca: {err}"))?;
    let ca_bytes = fs::read(&ca).map_err(|err| format!("read ca: {err}"))?;
    scan("ca", &ca_bytes)?;
    fs::write(
        &paths.gold_miss,
        "{\"expected\":{\"note\":\"GOLD-MISS\"},\"row_comparator\":\"ordered\"}\n",
    )
    .map_err(|err| format!("gold: {err}"))?;
    Ok(())
}

fn k2(paths: &Paths, tools: &Tools, desk: &Desk<'_>) -> Result<(String, String), String> {
    let left_cases = vec![
        case(desk, "desk", "A255-SPLIT", "score", 30_000, 2, desk.gold_full)?,
        case(desk, "gate", "A255-GATE", "score", 400, 2, desk.gold_full)?,
        case(desk, "note", "A255-NOTE", "needs_reannotation", 30_000, 2, desk.gold_full)?,
        case(desk, "slow", "A255-SLOW", "score", 400, 2, desk.gold_full)?,
        case(desk, "drop", "A255-PASS", "score", 30_000, 2, desk.gold_full)?,
    ];
    let right_cases = vec![
        case(desk, "desk", "A255-PASS", "score", 30_000, 2, desk.gold_full)?,
        case(desk, "gate", "A255-PASS", "score", 30_000, 2, desk.gold_miss)?,
        case(desk, "note", "A255-NOTE", "needs_reannotation", 30_000, 2, desk.gold_full)?,
        case(desk, "slow", "A255-PASS", "score", 30_000, 2, desk.gold_full)?,
        case(desk, "drop", "A255-DISARM", "score", 30_000, 2, desk.gold_full)?,
    ];
    let left = run_suite(
        paths,
        tools,
        "k2-left",
        &suite("synthetic", "dispatch", "live-k2", "k2", &left_cases, tools)?,
        1,
    )?;
    restore(tools)?;
    let right = run_suite(
        paths,
        tools,
        "k2-right",
        &suite("frozen_eval", "dispatch", "live-k2", "k2", &right_cases, tools)?,
        1,
    )?;
    restore(tools)?;
    expect_rows(
        &left,
        "\
desk:0:pass:match
desk:1:fail:mismatch
gate:0:fail:refused
gate:1:timeout:timeout
note:0:needs_reannotation:needs_reannotation
note:1:needs_reannotation:needs_reannotation
slow:0:timeout:timeout
slow:1:timeout:timeout
drop:0:pass:match
drop:1:pass:match",
    )?;
    expect_rows(
        &right,
        "\
desk:0:pass:match
desk:1:pass:match
gate:0:fail:mismatch
gate:1:fail:mismatch
note:0:needs_reannotation:needs_reannotation
note:1:needs_reannotation:needs_reannotation
slow:0:pass:match
slow:1:pass:match
drop:0:pass:match
drop:1:not_run:not_run",
    )?;
    let cleanup = cleanups(&right)?;
    if cleanup != "unknown,unknown,unknown,unknown,unknown,unknown,unknown,unknown,unknown,null" {
        return Err(format!("k2 right cleanup: {cleanup}"));
    }
    if !left.contains("\"model\":\"none\"") || !left.contains(&format!("\"snapshot\":\"{SNAPSHOT}\"")) {
        return Err("k2 left tuple labels missing".to_owned());
    }
    if !right.contains("\"mode\":\"frozen_eval\"") || !left.contains("\"mode\":\"synthetic\"") {
        return Err("k2 modes were not preserved on the runner reports".to_owned());
    }
    fs::write(
        paths.proof.join("vectors-runner.txt"),
        format!("k2-left\n{}\n\nk2-right\n{}\n", rows(&left)?, rows(&right)?),
    )
    .map_err(|err| err.to_string())?;
    Ok((left, right))
}

fn allfail(paths: &Paths, tools: &Tools, desk: &Desk<'_>) -> Result<(String, String), String> {
    let left_cases = vec![case(desk, "desk", "A255-QUAD", "score", 400, 4, desk.gold_full)?];
    let right_cases = vec![case(desk, "desk", "A255-PASS", "score", 30_000, 4, desk.gold_miss)?];
    let left = run_suite(
        paths,
        tools,
        "allfail-left",
        &suite("synthetic", "dispatch", "live-allfail", "k4", &left_cases, tools)?,
        1,
    )?;
    restore(tools)?;
    let right = run_suite(
        paths,
        tools,
        "allfail-right",
        &suite("synthetic", "dispatch", "live-allfail", "k4", &right_cases, tools)?,
        1,
    )?;
    restore(tools)?;
    expect_rows(
        &left,
        "\
desk:0:timeout:timeout
desk:1:fail:refused
desk:2:fail:mismatch
desk:3:not_run:not_run",
    )?;
    expect_rows(
        &right,
        "\
desk:0:fail:mismatch
desk:1:fail:mismatch
desk:2:fail:mismatch
desk:3:fail:mismatch",
    )?;
    if cleanups(&left)? != "unknown,unknown,unknown,null" {
        return Err(format!("allfail left cleanup: {}", cleanups(&left)?));
    }
    Ok((left, right))
}

fn refuse_tuple_order(paths: &Paths, tools: &Tools, desk: &Desk<'_>) -> Result<(), String> {
    let one = vec![case(desk, "desk", "A255-PASS", "score", 30_000, 1, desk.gold_full)?];
    let left = run_suite(
        paths,
        tools,
        "tuple-left",
        &suite("synthetic", "dispatch", "live-tuple", "k1", &one, tools)?,
        0,
    )?;
    let right_case = vec![case(desk, "desk", "A255-PASS", "score", 30_000, 1, desk.gold_full)?];
    let right = run_suite(
        paths,
        tools,
        "tuple-right",
        &suite("synthetic", "other", "live-tuple", "k1b", &right_case, tools)?,
        0,
    )?;
    let (code, stdout, stderr) = adapt(paths, "tuple", &pair(&left, &right, "k1", "live-tuple")?)?;
    expect_refuse(code, &stdout, &stderr, "trial-report-adapter: tuple /reports/1/tuple/project\n")?;
    let order_left_cases = vec![
        case(desk, "desk", "A255-PASS", "score", 30_000, 1, desk.gold_full)?,
        case(desk, "gate", "A255-PASS", "score", 30_000, 1, desk.gold_full)?,
    ];
    let order_right_cases = vec![
        case(desk, "gate", "A255-PASS", "score", 30_000, 1, desk.gold_full)?,
        case(desk, "desk", "A255-PASS", "score", 30_000, 1, desk.gold_full)?,
    ];
    let order_left = run_suite(
        paths,
        tools,
        "order-left",
        &suite("synthetic", "dispatch", "live-order", "k1", &order_left_cases, tools)?,
        0,
    )?;
    let order_right = run_suite(
        paths,
        tools,
        "order-right",
        &suite("synthetic", "dispatch", "live-order", "k1", &order_right_cases, tools)?,
        0,
    )?;
    let (code, stdout, stderr) = adapt(
        paths,
        "order",
        &pair(&order_left, &order_right, "k1", "live-order")?,
    )?;
    expect_refuse(
        code,
        &stdout,
        &stderr,
        "trial-report-adapter: schema /reports/1/trials/0/case_id\n",
    )?;
    fs::write(paths.proof.join("tuple-left-keep.txt"), &left).map_err(|err| err.to_string())?;
    Ok(())
}

fn refuse_budget(paths: &Paths, tools: &Tools, desk: &Desk<'_>) -> Result<(), String> {
    let cases = vec![case(desk, "desk", "A255-PASS", "score", 30_000, 1, desk.gold_full)?];
    let left = run_suite(
        paths,
        tools,
        "budget-left",
        &suite("synthetic", "dispatch", "live-budget", "k1", &cases, tools)?,
        0,
    )?;
    let right = run_suite(
        paths,
        tools,
        "budget-right",
        &suite("synthetic", "dispatch", "live-budget", "k1b", &cases, tools)?,
        0,
    )?;
    let (code, stdout, stderr) = adapt(paths, "budget", &pair(&left, &right, "k1", "live-budget")?)?;
    expect_refuse(code, &stdout, &stderr, "trial-report-adapter: tuple /reports/1/tuple/budget\n")?;
    let (code, stdout, stderr) = adapt(paths, "label-budget", &pair_budget(&left, &left, "other", SCORER)?)?;
    expect_refuse(code, &stdout, &stderr, "trial-report-adapter: tuple /labels/budget\n")?;
    fs::write(paths.proof.join("budget-left-keep.txt"), left).map_err(|err| err.to_string())
}

fn refuse_labels_and_scorer(paths: &Paths) -> Result<(), String> {
    let left = fs::read_to_string(paths.proof.join("budget-left-keep.txt"))
        .map_err(|err| format!("budget left: {err}"))?;
    let (code, stdout, stderr) = adapt(
        paths,
        "label-scorer",
        &pair_budget(&left, &left, "k1", OTHER_SCORER)?,
    )?;
    expect_refuse(
        code,
        &stdout,
        &stderr,
        "trial-report-adapter: tuple /labels/scorer_version\n",
    )?;
    let mutated = mutate_scorer(&left)?;
    let (code, stdout, stderr) = adapt(paths, "scorer", &pair(&left, &mutated, "k1", "live-budget")?)?;
    expect_refuse(
        code,
        &stdout,
        &stderr,
        "trial-report-adapter: tuple /reports/1/tuple/scorer_version\n",
    )?;
    Ok(())
}

fn refuse_missing(paths: &Paths, tools: &Tools, desk: &Desk<'_>) -> Result<(), String> {
    let cases = vec![case(desk, "desk", "A255-PASS", "score", 30_000, 2, desk.gold_full)?];
    let report = run_suite(
        paths,
        tools,
        "missing-base",
        &suite("synthetic", "dispatch", "live-missing", "k2", &cases, tools)?,
        0,
    )?;
    expect_rows(&report, "desk:0:pass:match\ndesk:1:pass:match")?;
    let short = drop_second_trial(&report)?;
    if short.contains("\"index\":1") {
        return Err("short report still has trial 1".to_owned());
    }
    let (code, stdout, stderr) = adapt(paths, "missing", &pair(&report, &short, "k2", "live-missing")?)?;
    expect_refuse(code, &stdout, &stderr, "trial-report-adapter: trials /reports/1/trials\n")?;
    Ok(())
}

fn case(
    desk: &Desk<'_>,
    id: &str,
    marker: &str,
    annotation: &str,
    wall: u64,
    k: u64,
    gold: &Path,
) -> Result<String, String> {
    let request = desk.scratch.join(format!("request-{id}-{marker}.json"));
    write_request(&request, marker, desk.scratch, gold)?;
    let body = format!(
        "{{\"annotation\":\"{annotation}\",\"binding_file\":\"{}\",\"gold_file\":\"{}\",\"id\":\"{id}\",\"k\":{k},\"request_file\":\"{}\",\"role\":\"desk\",\"trial_wall_ms\":{wall}}}",
        json_path(desk.binding)?,
        json_path(gold)?,
        json_path(&request)?,
    );
    Ok(body)
}

fn write_request(path: &Path, marker: &str, scratch: &Path, gold: &Path) -> Result<(), String> {
    let password = scratch.join("login.pass");
    let ca = scratch.join("ca.crt");
    let body = format!(
        "{{\"version\":\"oo.gateway.query-pg/v1\",\"endpoint\":{{\"host\":\"db.example\",\"port\":5432,\"database\":\"oo_a255\",\"user\":\"oo_a255_login\"}},\"password_file\":\"{}\",\"ca_file\":\"{}\",\"audit_file\":\"/target/a255-scratch/placeholder-audit.jsonl\",\"identity\":{{\"project\":\"dispatch\",\"role\":\"oo_r_desk\",\"policy_id\":\"{DIGEST}\"}},\"policies\":[{{\"project\":\"dispatch\",\"config_digest\":\"{DIGEST}\",\"generation\":1,\"role\":\"desk\",\"login\":\"oo_a255_login\",\"table\":{{\"namespace\":\"dispatch\",\"name\":\"ticket\",\"columns\":[]}}}}],\"statement\":{{\"source\":\"request\",\"sql\":\"SELECT note FROM dispatch.ticket /* {marker} */\",\"max_rows\":20,\"max_result_bytes\":4096,\"wall_clock_ms\":30000}}}}",
        json_path(&password)?,
        json_path(&ca)?,
    );
    let bytes = body.into_bytes();
    scan("request", &bytes)?;
    let gold_text = gold.display().to_string();
    if bytes.windows(gold_text.len()).any(|window| window == gold_text.as_bytes()) {
        return Err("request contains the gold path".to_owned());
    }
    fs::write(path, bytes).map_err(|err| format!("write request: {err}"))
}

fn suite(
    mode: &str,
    project: &str,
    suite_name: &str,
    budget: &str,
    cases: &[String],
    tools: &Tools,
) -> Result<String, String> {
    let joined = cases.join(",");
    Ok(format!(
        "{{\"version\":\"oo.benchmark.sql-runner/v1\",\"mode\":\"{mode}\",\"metadata\":\"declared\",\"tuple\":{{\"project\":\"{project}\",\"snapshot\":\"{SNAPSHOT}\",\"as_of\":\"2026-10-06\",\"dataset\":\"desk\",\"suite\":\"{suite_name}\",\"model\":\"none\",\"harness\":\"sql-runner\",\"role\":\"desk\",\"budget\":\"{budget}\",\"scorer_version\":\"{SCORER}\"}},\"executables\":{{\"query_pg\":{{\"path\":\"{}\",\"sha256\":\"{}\"}},\"adapter\":{{\"path\":\"{}\",\"sha256\":\"{}\"}},\"comparator\":{{\"path\":\"{}\",\"sha256\":\"{}\"}}}},\"cases\":[{joined}]}}",
        json_path(&tools.query)?,
        tools.digest,
        json_path(&tools.adapter)?,
        tools.digest,
        json_path(&tools.comparator)?,
        tools.digest,
    ))
}

fn json_path(path: &Path) -> Result<String, String> {
    let text = path.to_str().ok_or("path is not utf-8")?;
    if text.bytes().any(|byte| byte == b'"' || byte == b'\\') {
        return Err(format!("path needs escaping: {text}"));
    }
    Ok(text.to_owned())
}

fn run_suite(
    paths: &Paths,
    tools: &Tools,
    name: &str,
    suite_body: &str,
    expect: i32,
) -> Result<String, String> {
    restore(tools)?;
    let root = paths.scratch.join(name);
    fs::create_dir_all(&root).map_err(|err| format!("{name} dir: {err}"))?;
    let suite_path = root.join("suite.json");
    scan(name, suite_body.as_bytes())?;
    fs::write(&suite_path, suite_body).map_err(|err| format!("write suite: {err}"))?;
    let run = root.join("run");
    if run.exists() {
        return Err(format!("{name} run dir already exists"));
    }
    let output = Command::new(&paths.sql_runner)
        .args([
            "--suite",
            suite_path.to_str().ok_or("suite path")?,
            "--run-dir",
            run.to_str().ok_or("run path")?,
        ])
        .env_remove("PGPASSWORD")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| format!("sql-runner: {err}"))?;
    let code = output.status.code().unwrap_or(127);
    save_io(paths, &format!("{name}-runner"), code, &output.stdout, &output.stderr)?;
    if code != expect {
        return Err(format!(
            "{name} runner exit {code}, want {expect}: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !output.stderr.is_empty() {
        return Err(format!("{name} runner stderr is not empty"));
    }
    if !output.stdout.ends_with(b"\n") {
        return Err(format!("{name} runner stdout has no final newline"));
    }
    let report = String::from_utf8(output.stdout).map_err(|_| format!("{name} report is not utf-8"))?;
    let stored = fs::read(run.join("report.json")).map_err(|err| format!("{name} report.json: {err}"))?;
    if stored != report.as_bytes() {
        return Err(format!("{name} stdout and report.json differ"));
    }
    Ok(report)
}

fn pair(left: &str, right: &str, budget: &str, suite_name: &str) -> Result<Vec<u8>, String> {
    pair_budget(left, right, budget, SCORER).and_then(|bytes| {
        let text = String::from_utf8(bytes).map_err(|_| "pair utf-8".to_owned())?;
        if !text.contains(&format!("\"suite\":\"{suite_name}\"")) {
            return Err(format!("pair missing suite {suite_name}"));
        }
        Ok(text.into_bytes())
    })
}

fn pair_budget(left: &str, right: &str, budget: &str, scorer: &str) -> Result<Vec<u8>, String> {
    let left = trim_report(left)?;
    let right = trim_report(right)?;
    let body = format!(
        "{{\"algorithm\":\"oo.benchmark.paired-question-bootstrap/v1\",\"labels\":{{\"budget\":\"{budget}\",\"scorer_version\":\"{scorer}\",\"variants\":[{{\"id\":\"left\"}},{{\"id\":\"right\"}}]}},\"reports\":[{left},{right}],\"resamples\":8,\"seed\":\"11\",\"version\":\"oo.benchmark.trial-report-adapter/v1\"}}"
    );
    scan("pair", body.as_bytes())?;
    Ok(body.into_bytes())
}

fn trim_report(report: &str) -> Result<&str, String> {
    let text = report.strip_suffix('\n').unwrap_or(report);
    if !text.starts_with('{') || !text.ends_with('}') {
        return Err("runner report is not one object".to_owned());
    }
    Ok(text)
}

fn adapt_ok(paths: &Paths, name: &str, input: &[u8]) -> Result<String, String> {
    let (code, stdout, stderr) = adapt(paths, name, input)?;
    if code != 0 {
        return Err(format!(
            "{name} adapter exit {code}: {}",
            String::from_utf8_lossy(&stderr)
        ));
    }
    if !stderr.is_empty() {
        return Err(format!("{name} adapter stderr is not empty"));
    }
    String::from_utf8(stdout).map_err(|_| format!("{name} adapter stdout is not utf-8"))
}

fn adapt(paths: &Paths, name: &str, input: &[u8]) -> Result<(i32, Vec<u8>, Vec<u8>), String> {
    scan(name, input)?;
    let mut child = Command::new(&paths.adapter)
        .env_remove("PGPASSWORD")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("adapter spawn: {err}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("adapter stdin")?;
        stdin.write_all(input).map_err(|err| format!("adapter write: {err}"))?;
    }
    let output = child.wait_with_output().map_err(|err| format!("adapter wait: {err}"))?;
    let code = output.status.code().unwrap_or(127);
    save_io(paths, &format!("{name}-adapter"), code, &output.stdout, &output.stderr)?;
    fs::write(paths.proof.join(format!("{name}-adapter.stdin")), input).map_err(|err| err.to_string())?;
    scan(&format!("{name} adapter stdout"), &output.stdout)?;
    Ok((code, output.stdout, output.stderr))
}

fn statistics(paths: &Paths, name: &str, input: &[u8], expect: i32) -> Result<String, String> {
    let mut child = Command::new(&paths.statistics)
        .env_remove("PGPASSWORD")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("statistics spawn: {err}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("statistics stdin")?;
        stdin.write_all(input).map_err(|err| format!("statistics write: {err}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|err| format!("statistics wait: {err}"))?;
    let code = output.status.code().unwrap_or(127);
    save_io(paths, &format!("{name}-statistics"), code, &output.stdout, &output.stderr)?;
    if code != expect {
        return Err(format!(
            "{name} statistics exit {code}, want {expect}: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !output.stderr.is_empty() {
        return Err(format!("{name} statistics stderr is not empty"));
    }
    if !output.stdout.ends_with(b"\n") {
        return Err(format!("{name} statistics stdout has no final newline"));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{name} statistics is not utf-8"))
}

fn save_io(paths: &Paths, name: &str, code: i32, stdout: &[u8], stderr: &[u8]) -> Result<(), String> {
    scan(&format!("{name} stdout"), stdout)?;
    scan(&format!("{name} stderr"), stderr)?;
    fs::write(paths.proof.join(format!("{name}.stdout")), stdout).map_err(|err| err.to_string())?;
    fs::write(paths.proof.join(format!("{name}.stderr")), stderr).map_err(|err| err.to_string())?;
    fs::write(paths.proof.join(format!("{name}.exit")), format!("{code}\n")).map_err(|err| err.to_string())
}

fn expect_refuse(code: i32, stdout: &[u8], stderr: &[u8], want: &str) -> Result<(), String> {
    if code != 2 {
        return Err(format!("refuse exit {code}, stderr {}", String::from_utf8_lossy(stderr)));
    }
    if !stdout.is_empty() {
        return Err("refuse wrote a document".to_owned());
    }
    if stderr != want.as_bytes() {
        return Err(format!(
            "refuse stderr {:?}, want {:?}",
            String::from_utf8_lossy(stderr),
            want
        ));
    }
    Ok(())
}

fn expect_rows(report: &str, want: &str) -> Result<(), String> {
    let got = rows(report)?;
    if got != want {
        return Err(format!("rows\n{got}\nwant\n{want}"));
    }
    Ok(())
}

fn rows(report: &str) -> Result<String, String> {
    let mut rows = Vec::new();
    let mut rest = report;
    while let Some(pos) = rest.find("\"case_id\":\"") {
        let after = &rest[pos + "\"case_id\":\"".len()..];
        let id_end = after.find('"').ok_or("case id")?;
        let id = &after[..id_end];
        let tail = &after[id_end..];
        let index = raw_after(tail, "\"index\":")?;
        let reason = str_after(tail, "\"reason\":\"")?;
        let status = str_after(tail, "\"status\":\"")?;
        rows.push(format!("{id}:{index}:{status}:{reason}"));
        rest = &after[id_end..];
    }
    if rows.is_empty() {
        return Err("report has no trials".to_owned());
    }
    Ok(rows.join("\n"))
}

fn cleanups(report: &str) -> Result<String, String> {
    let mut found = Vec::new();
    let mut rest = report;
    while let Some(pos) = rest.find("\"backend_cleanup\":") {
        let after = &rest[pos + "\"backend_cleanup\":".len()..];
        if let Some(text) = after.strip_prefix("null") {
            found.push("null");
            rest = text;
        } else if let Some(text) = after.strip_prefix("\"unknown\"") {
            found.push("unknown");
            rest = text;
        } else {
            return Err("unexpected backend_cleanup".to_owned());
        }
    }
    Ok(found.join(","))
}

fn expect_shape(document: &str, want: &str) -> Result<(), String> {
    let got = shape(document)?;
    let path = PathBuf::from(std::env::var("OO_PROOF_DIR").map_err(|_| "OO_PROOF_DIR is required")?)
        .join("vectors-adapter.txt");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| format!("vectors: {err}"))?;
    writeln!(file, "{got}\n").map_err(|err| format!("vectors: {err}"))?;
    if got != want {
        return Err(format!("adapter shape\n{got}\nwant\n{want}"));
    }
    Ok(())
}

fn shape(document: &str) -> Result<String, String> {
    let events = events(document)?;
    let mut lines = Vec::new();
    let mut index = 0;
    while index < events.len() {
        let (kind, text) = &events[index];
        if *kind != "id" {
            return Err(format!("expected variant id, saw {text}"));
        }
        let variant = text.clone();
        index += 1;
        let mut parts = Vec::new();
        while index < events.len() && events[index].0 == "id" && events[index].1 != "left" && events[index].1 != "right"
        {
            let case_id = events[index].1.clone();
            index += 1;
            let mut trials = Vec::new();
            while index < events.len() && events[index].0 == "status" {
                trials.push(events[index].1.clone());
                index += 1;
            }
            if trials.is_empty() {
                return Err(format!("case {case_id} has no trials"));
            }
            parts.push(format!("{case_id}:{}", trials.join(",")));
        }
        lines.push(format!("{variant} {}", parts.join(";")));
    }
    Ok(lines.join("\n"))
}

fn events(document: &str) -> Result<Vec<(String, String)>, String> {
    let mut found = Vec::new();
    let bytes = document.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let rest = &document[index..];
        let id_at = rest.find("\"id\":\"");
        let status_at = rest.find("\"status\":\"");
        let next = match (id_at, status_at) {
            (Some(id), Some(status)) if id <= status => ("id", id),
            (Some(_), Some(status)) => ("status", status),
            (Some(id), None) => ("id", id),
            (None, Some(status)) => ("status", status),
            (None, None) => break,
        };
        let key_len = if next.0 == "id" { "\"id\":\"".len() } else { "\"status\":\"".len() };
        let start = index + next.1 + key_len;
        let tail = document.get(start..).ok_or("event tail")?;
        let end = tail.find('"').ok_or("event end")?;
        found.push((next.0.to_owned(), tail[..end].to_owned()));
        index = start + end + 1;
    }
    Ok(found)
}

fn expect_trusted(document: &str, suite_name: &str, budget: &str, digest: &str) -> Result<(), String> {
    if !document.contains("\"model\":\"trusted_sql\"") {
        return Err("adapter model is not trusted_sql".to_owned());
    }
    if document.contains("\"model\":\"none\"") {
        return Err("adapter kept model none".to_owned());
    }
    if document.contains(digest) {
        return Err("adapter copied an executable digest".to_owned());
    }
    for absent in ["pass_k", "percentile", "certified", "backend_cleanup", "\"mode\""] {
        if document.contains(absent) {
            return Err(format!("adapter output contains {absent}"));
        }
    }
    for present in [
        format!("\"snapshot\":\"{SNAPSHOT}\""),
        format!("\"suite\":\"{suite_name}\""),
        format!("\"budget\":\"{budget}\""),
        "\"version\":\"oo.benchmark.trial-statistics/v1\"".to_owned(),
        "\"seed\":\"11\"".to_owned(),
        "\"resamples\":8".to_owned(),
    ] {
        if !document.contains(&present) {
            return Err(format!("adapter output missing {present}"));
        }
    }
    Ok(())
}

fn expect_stats(report: &str, gate: &str, questions: u64, reannotation: u64, k: u64, pass_k: &[u64]) -> Result<(), String> {
    if !report.contains("\"version\":\"oo.benchmark.trial-statistics-report/v1\"") {
        return Err("statistics version missing".to_owned());
    }
    if !report.contains(&format!("\"gate\":\"{gate}\"")) {
        return Err(format!("statistics gate is not {gate}"));
    }
    expect_num(report, "question_count", questions)?;
    expect_num(report, "reannotation_count", reannotation)?;
    expect_num(report, "trial_count", k)?;
    if !report.contains("\"seed\":\"11\"") || !report.contains("\"resamples\":8") {
        return Err("statistics did not keep seed and resamples".to_owned());
    }
    let got = numbers(report, "pass_k_count")?;
    if got != pass_k {
        return Err(format!("pass_k_count {got:?}, want {pass_k:?}"));
    }
    Ok(())
}

fn expect_num(report: &str, key: &str, want: u64) -> Result<(), String> {
    let got = numbers(report, key)?;
    if got.as_slice() != [want] {
        return Err(format!("{key} {got:?}, want {want}"));
    }
    Ok(())
}

fn numbers(report: &str, key: &str) -> Result<Vec<u64>, String> {
    let needle = format!("\"{key}\":");
    let mut found = Vec::new();
    let mut rest = report;
    while let Some(pos) = rest.find(&needle) {
        let after = &rest[pos + needle.len()..];
        let end = after
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(after.len());
        if end == 0 {
            return Err(format!("{key} is not an integer"));
        }
        let value: u64 = after[..end].parse().map_err(|_| format!("{key} parse"))?;
        found.push(value);
        rest = &after[end..];
    }
    if found.is_empty() {
        return Err(format!("{key} missing"));
    }
    Ok(found)
}

fn raw_after<'a>(text: &'a str, key: &str) -> Result<&'a str, String> {
    let pos = text.find(key).ok_or_else(|| format!("missing {key}"))?;
    let after = &text[pos + key.len()..];
    let end = after
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(after.len());
    if end == 0 {
        return Err(format!("{key} empty"));
    }
    Ok(&after[..end])
}

fn str_after<'a>(text: &'a str, key: &str) -> Result<&'a str, String> {
    let pos = text.find(key).ok_or_else(|| format!("missing {key}"))?;
    let after = &text[pos + key.len()..];
    let end = after.find('"').ok_or_else(|| format!("{key} end"))?;
    Ok(&after[..end])
}

fn mutate_scorer(report: &str) -> Result<String, String> {
    let from = format!("\"scorer_version\":\"{SCORER}\"");
    let to = format!("\"scorer_version\":\"{OTHER_SCORER}\"");
    let count = report.matches(&from).count();
    if count != 1 {
        return Err(format!("scorer field count {count}"));
    }
    Ok(report.replacen(&from, &to, 1))
}

fn drop_second_trial(report: &str) -> Result<String, String> {
    let mut text = report.trim_end().to_owned();
    replace_one(&mut text, "\"pass_count\":2", "\"pass_count\":1")?;
    replace_one(&mut text, "\"trial_count\":2", "\"trial_count\":1")?;
    let marker = "\"trials\":[";
    let start = text.find(marker).ok_or("trials array")?;
    let array_at = start + marker.len();
    let first_end = value_end(&text, array_at)?;
    let mut cursor = first_end;
    while text.as_bytes().get(cursor) == Some(&b' ') {
        cursor += 1;
    }
    if text.as_bytes().get(cursor) != Some(&b',') {
        return Err("trials array has one object".to_owned());
    }
    cursor += 1;
    while text.as_bytes().get(cursor) == Some(&b' ') {
        cursor += 1;
    }
    let second_end = value_end(&text, cursor)?;
    if text.as_bytes().get(second_end) != Some(&b']') {
        return Err("trials array does not close after two objects".to_owned());
    }
    let mut short = String::new();
    short.push_str(&text[..first_end]);
    short.push_str(&text[second_end..]);
    Ok(short)
}

fn replace_one(text: &mut String, from: &str, to: &str) -> Result<(), String> {
    let count = text.matches(from).count();
    if count != 1 {
        return Err(format!("count of {from} is {count}"));
    }
    *text = text.replacen(from, to, 1);
    Ok(())
}

fn self_check_drop() -> Result<(), String> {
    let sample = "{\"pass_count\":2,\"trial_count\":2,\"trials\":[{\"index\":0,\"status\":\"pass\"},{\"index\":1,\"status\":\"pass\"}]}\n";
    let short = drop_second_trial(sample)?;
    let want = "{\"pass_count\":1,\"trial_count\":1,\"trials\":[{\"index\":0,\"status\":\"pass\"}]}";
    if short != want {
        return Err(format!("drop self-check got {short}"));
    }
    Ok(())
}

fn value_end(text: &str, start: usize) -> Result<usize, String> {
    let bytes = text.as_bytes();
    if bytes.get(start) != Some(&b'{') && bytes.get(start) != Some(&b'[') {
        return Err("value is not an object or array".to_owned());
    }
    let open = bytes[start];
    let close = if open == b'{' { b'}' } else { b']' };
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, byte) in bytes[start..].iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        if *byte == b'"' {
            in_string = true;
        } else if *byte == open {
            depth += 1;
        } else if *byte == close {
            depth -= 1;
            if depth == 0 {
                return Ok(start + offset + 1);
            }
        }
    }
    Err("unclosed value".to_owned())
}

fn scan(label: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.windows(SECRET.len()).any(|window| window == SECRET.as_bytes()) {
        return Err(format!("{label} contains the fixture password"));
    }
    if bytes.windows(11).any(|window| window == b"PRIVATE KEY") {
        return Err(format!("{label} contains a private key marker"));
    }
    Ok(())
}

fn check_sha256() -> Result<(), String> {
    if hex(&sha256(b"")) != "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" {
        return Err("sha256 empty self-check failed".to_owned());
    }
    if hex(&sha256(b"abc")) != "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" {
        return Err("sha256 abc self-check failed".to_owned());
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
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
            let t1 = h.wrapping_add(s1).wrapping_add(choose).wrapping_add(K[index]).wrapping_add(word[index]);
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
