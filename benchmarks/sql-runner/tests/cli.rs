use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde_json::{Value, json};

const HELP: &str = "\
sql-runner --suite <path> --run-dir <path>
sql-runner --help

sql-runner reads one oo.benchmark.sql-runner/v1 suite and writes one trial report.
Each trial is a new query-pg process, then the result adapter, then the result comparator.
exit 0 gate passed
exit 1 gate failed
exit 2 config
";

const SECRET: &str = "pw-a231-secret";
const CANARY: &str = "GOLD-CANARY-a231-9f3c";

struct Tools {
    query: PathBuf,
    adapter: PathBuf,
    comparator: PathBuf,
    hash: String,
}

fn tools() -> &'static Tools {
    static TOOLS: OnceLock<Tools> = OnceLock::new();
    static GATE: Mutex<()> = Mutex::new(());
    TOOLS.get_or_init(|| {
        let _guard = GATE.lock().expect("stub gate");
        compile_stub()
    })
}

fn compile_stub() -> Tools {
    let mut dir = std::env::current_exe().expect("current_exe");
    dir.pop();
    dir.pop();
    dir.push("a231-sql-runner-stub");
    fs::create_dir_all(&dir).expect("stub dir");
    let compiled = dir.join("stub");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/stub_bin.rs");
    let output = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned()))
        .args(["--edition=2021", "-D", "unsafe_code", "-o"])
        .arg(&compiled)
        .arg(&source)
        .output()
        .expect("rustc");
    assert!(
        output.status.success(),
        "rustc stub failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let query = dir.join("query-pg");
    let adapter = dir.join("query-result-adapter");
    let comparator = dir.join("result-comparator");
    for path in [&query, &adapter, &comparator] {
        if path.exists() {
            fs::remove_file(path).expect("remove old link");
        }
        fs::hard_link(&compiled, path).expect("hard link");
    }
    let hash = oo_sql_runner::sha256_hex(&fs::read(&query).expect("stub bytes"));
    Tools {
        query,
        adapter,
        comparator,
        hash,
    }
}

fn scratch(label: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let path = std::env::temp_dir().join(format!(
        "oo-a231-{}-{}-{}",
        std::process::id(),
        label,
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).expect("scratch");
    path
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).expect("json")).expect("write");
}

fn secrets(dir: &Path) {
    fs::write(dir.join("login.pass"), format!("{SECRET}\n")).expect("password");
    fs::write(
        dir.join("ca.crt"),
        "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n",
    )
    .expect("ca");
}

fn request(dir: &Path, sql: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let path = dir.join(format!(
        "request-{}.json",
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let value = json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": {"host": "db.example", "port": 5432, "database": "oo_a231", "user": "oo_a231_login"},
        "password_file": dir.join("login.pass").display().to_string(),
        "ca_file": dir.join("ca.crt").display().to_string(),
        "audit_file": dir.join("placeholder-audit.jsonl").display().to_string(),
        "identity": {"project": "dispatch", "role": "oo_r_desk", "policy_id": "abababababababababababababababababababababababababababababababab"},
        "policies": [{
            "project": "dispatch",
            "config_digest": "abababababababababababababababababababababababababababababababab",
            "generation": 1,
            "role": "desk",
            "login": "oo_a231_login",
            "table": {"namespace": "dispatch", "name": "ticket", "columns": []}
        }],
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1000000,
            "unknown_rows": "refuse"
        },
        "statement": {
            "source": "request",
            "sql": sql,
            "max_rows": 20,
            "max_result_bytes": 4096,
            "wall_clock_ms": 30000
        }
    });
    write_json(&path, &value);
    path
}

fn exe(path: &Path, hash: &str) -> Value {
    json!({"path": path.display().to_string(), "sha256": hash})
}

fn tuple() -> Value {
    json!({
        "project": "dispatch",
        "snapshot": "fixture-a231",
        "as_of": "2026-10-06",
        "dataset": "desk",
        "suite": "synthetic-desk",
        "model": "none",
        "harness": "sql-runner",
        "role": "desk",
        "budget": "k8",
        "scorer_version": "oo.benchmark.sql-runner/v1"
    })
}

fn suite(mode: &str, cases: Vec<Value>) -> Value {
    let tools = tools();
    json!({
        "version": "oo.benchmark.sql-runner/v1",
        "mode": mode,
        "metadata": "declared",
        "tuple": tuple(),
        "executables": {
            "query_pg": exe(&tools.query, &tools.hash),
            "adapter": exe(&tools.adapter, &tools.hash),
            "comparator": exe(&tools.comparator, &tools.hash)
        },
        "cases": cases
    })
}

fn case(id: &str, k: u64, annotation: &str, wall: u64, request: &Path, gold: &str) -> Value {
    json!({
        "id": id,
        "role": "desk",
        "k": k,
        "annotation": annotation,
        "trial_wall_ms": wall,
        "request_file": request.display().to_string(),
        "binding_file": fixture("note-header.json").display().to_string(),
        "gold_file": fixture(gold).display().to_string()
    })
}

fn run_at(suite_path: &Path, run: &Path) -> (i32, Vec<u8>, Vec<u8>) {
    let output = Command::new(env!("CARGO_BIN_EXE_sql-runner"))
        .args([
            "--suite",
            suite_path.to_str().expect("suite"),
            "--run-dir",
            run.to_str().expect("run"),
        ])
        .env("PGPASSWORD", SECRET)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run");
    (
        output.status.code().unwrap_or(127),
        output.stdout,
        output.stderr,
    )
}

fn write_suite(dir: &Path, value: &Value) -> PathBuf {
    let path = dir.join("suite.json");
    write_json(&path, value);
    path
}

fn assert_config(dir: &Path, value: &Value, stderr: &str) {
    let suite_path = write_suite(dir, value);
    let run = dir.join("run");
    let (code, stdout, err) = run_at(&suite_path, &run);
    assert_eq!(code, 2, "{}", String::from_utf8_lossy(&err));
    assert!(stdout.is_empty());
    assert_eq!(String::from_utf8(err).expect("stderr"), stderr);
    assert!(!run.exists());
}

fn parse_report(stdout: &[u8]) -> Value {
    assert!(stdout.ends_with(b"\n"), "report newline");
    let body = &stdout[..stdout.len() - 1];
    let value: Value = serde_json::from_slice(body).expect("report json");
    let again = serde_json::to_vec(&value).expect("reencode");
    assert_eq!(again.as_slice(), body, "object keys are not alphabetical");
    value
}

fn trial<'a>(report: &'a Value, id: &str, index: u64) -> &'a Value {
    report["trials"]
        .as_array()
        .expect("trials")
        .iter()
        .find(|item| item["case_id"] == id && item["index"].as_u64() == Some(index))
        .unwrap_or_else(|| panic!("missing {id} {index}"))
}

fn assert_quiet(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    for banned in [
        SECRET,
        CANARY,
        "CANARY-CELL-other",
        "A231-MATCH",
        "dispatch.ticket",
        "password_file",
        "full",
        "prefix",
    ] {
        assert!(!text.contains(banned), "leaked {banned}");
    }
}

#[test]
fn help_and_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_sql-runner"))
        .arg("--help")
        .output()
        .expect("help");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, HELP.as_bytes());
    assert!(output.stderr.is_empty());

    let short = Command::new(env!("CARGO_BIN_EXE_sql-runner"))
        .arg("-h")
        .output()
        .expect("short help");
    assert_eq!(short.status.code(), Some(0));
    assert_eq!(short.stdout, HELP.as_bytes());

    let usage = Command::new(env!("CARGO_BIN_EXE_sql-runner"))
        .args(["--run-dir", "/tmp/nope", "--suite", "/tmp/nope.json"])
        .output()
        .expect("usage");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    assert_eq!(usage.stderr, b"sql-runner: usage\n");
}

#[test]
fn config_rejects_mode_tuple_and_paths() {
    let dir = scratch("config");
    let base = json!({
        "version": "oo.benchmark.sql-runner/v1",
        "mode": "synthetic",
        "metadata": "declared",
        "tuple": {},
        "executables": {},
        "cases": []
    });
    let mut production = base.clone();
    production["mode"] = json!("production");
    assert_config(&dir, &production, "sql-runner: mode /mode\n");
    let mut unknown = base.clone();
    unknown["mode"] = json!("unknown");
    assert_config(&dir, &unknown, "sql-runner: mode /mode\n");
    let mut metadata = base.clone();
    metadata["metadata"] = json!("certified");
    assert_config(&dir, &metadata, "sql-runner: schema /metadata\n");

    let mut bad_model = base.clone();
    bad_model["tuple"] = tuple();
    bad_model["tuple"]["model"] = json!("gpt");
    assert_config(&dir, &bad_model, "sql-runner: tuple /tuple/model\n");
    let mut bad_snapshot = base.clone();
    bad_snapshot["tuple"] = tuple();
    bad_snapshot["tuple"]["snapshot"] = json!("unknown");
    assert_config(&dir, &bad_snapshot, "sql-runner: tuple /tuple/snapshot\n");

    let tools = tools();
    let mut relative = suite("synthetic", vec![]);
    relative["executables"]["query_pg"]["path"] = json!("query-pg");
    relative["cases"] = json!([case(
        "only",
        1,
        "score",
        1000,
        &dir.join("none"),
        "gold-full.json"
    )]);
    // An empty case list is budget, and it is checked after executables.
    // Give the relative path a case count that would otherwise be valid once the path is accepted.
    assert_config(
        &dir,
        &relative,
        "sql-runner: path /executables/query_pg/path\n",
    );

    let mut root = suite("synthetic", vec![]);
    root["executables"]["query_pg"]["path"] = json!("/");
    assert_config(&dir, &root, "sql-runner: path /executables/query_pg/path\n");
    let mut unknown_part = suite("synthetic", vec![]);
    unknown_part["executables"]["query_pg"]["path"] = json!("/tmp/unknown/query-pg");
    assert_config(
        &dir,
        &unknown_part,
        "sql-runner: path /executables/query_pg/path\n",
    );

    let mut symlink = suite("synthetic", vec![]);
    let link = dir.join("query-link");
    std::os::unix::fs::symlink(&tools.query, &link).expect("symlink");
    symlink["executables"]["query_pg"]["path"] = json!(link.display().to_string());
    assert_config(
        &dir,
        &symlink,
        "sql-runner: path /executables/query_pg/path\n",
    );

    let mut bad_hash = suite("synthetic", vec![]);
    bad_hash["executables"]["query_pg"]["sha256"] =
        json!("0000000000000000000000000000000000000000000000000000000000000000");
    assert_config(
        &dir,
        &bad_hash,
        "sql-runner: hash /executables/query_pg/sha256\n",
    );

    secrets(&dir);
    let matched = request(&dir, "SELECT note FROM dispatch.ticket /* A231-MATCH */");
    let mut zero_k = suite(
        "synthetic",
        vec![case("only", 1, "score", 1000, &matched, "gold-full.json")],
    );
    zero_k["cases"][0]["k"] = json!(0);
    assert_config(&dir, &zero_k, "sql-runner: budget /cases/0/k\n");
    let mut nine = suite(
        "synthetic",
        vec![case("only", 1, "score", 1000, &matched, "gold-full.json")],
    );
    nine["cases"][0]["k"] = json!(9);
    assert_config(&dir, &nine, "sql-runner: budget /cases/0/k\n");

    let mut many = Vec::new();
    for index in 0..33 {
        let mut item = case("only", 1, "score", 1000, &matched, "gold-full.json");
        item["id"] = json!(format!("c{index}"));
        many.push(item);
    }
    assert_config(
        &dir,
        &suite("synthetic", many),
        "sql-runner: budget /cases\n",
    );

    let mut duplicate = suite(
        "synthetic",
        vec![
            case("same", 1, "score", 1000, &matched, "gold-full.json"),
            case("same", 1, "score", 1000, &matched, "gold-full.json"),
        ],
    );
    assert_config(&dir, &duplicate, "sql-runner: schema /cases/1/id\n");
    duplicate["cases"][1]["id"] = json!("other");
    duplicate["cases"][1]["role"] = json!("peek");
    assert_config(&dir, &duplicate, "sql-runner: role /cases/1/role\n");

    let gold = fixture("gold-full.json");
    let poisoned = request(
        &dir,
        &format!("SELECT '{path}' /* A231-MATCH */", path = gold.display()),
    );
    assert_config(
        &dir,
        &suite(
            "synthetic",
            vec![case("only", 1, "score", 1000, &poisoned, "gold-full.json")],
        ),
        "sql-runner: gold /cases/0/request_file\n",
    );

    let leaked = request(&dir, &format!("SELECT '{SECRET}' /* A231-MATCH */"));
    let stderr = Command::new(env!("CARGO_BIN_EXE_sql-runner"))
        .args([
            "--suite",
            write_suite(
                &dir,
                &suite(
                    "synthetic",
                    vec![case("only", 1, "score", 1000, &leaked, "gold-full.json")],
                ),
            )
            .to_str()
            .expect("suite"),
            "--run-dir",
            dir.join("secret-run").to_str().expect("run"),
        ])
        .output()
        .expect("password config");
    assert_eq!(stderr.status.code(), Some(2));
    assert!(stderr.stdout.is_empty());
    assert_eq!(
        stderr.stderr,
        b"sql-runner: password /cases/0/request_file\n"
    );
    assert!(!String::from_utf8_lossy(&stderr.stderr).contains(SECRET));
    assert!(!dir.join("secret-run").exists());
}

#[test]
fn one_match_passes_and_a_second_run_is_refused() {
    let dir = scratch("pass");
    secrets(&dir);
    let request = request(&dir, "SELECT note FROM dispatch.ticket /* A231-MATCH */");
    let suite_path = write_suite(
        &dir,
        &suite(
            "frozen_eval",
            vec![case("exact", 1, "score", 5000, &request, "gold-full.json")],
        ),
    );
    let run = dir.join("run");
    let started = Instant::now();
    let (code, stdout, err) = run_at(&suite_path, &run);
    assert!(started.elapsed().as_secs() < 10, "trial did not finish");
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    assert!(err.is_empty());
    let report = parse_report(&stdout);
    assert_eq!(
        fs::read(run.join("report.json")).expect("report file"),
        stdout
    );
    assert_eq!(report["passed"], true);
    assert_eq!(report["lane"], "trusted_sql");
    assert_eq!(report["metadata"], "declared");
    assert_eq!(report["mode"], "frozen_eval");
    assert_eq!(report["question_count"], 1);
    assert_eq!(report["trial_count"], 1);
    assert_eq!(report["pass_count"], 1);
    assert_eq!(report["tuple"]["model"], "none");
    assert_eq!(report["tuple"]["harness"], "sql-runner");
    let trial = trial(&report, "exact", 0);
    assert_eq!(trial["status"], "pass");
    assert_eq!(trial["reason"], "match");
    assert_eq!(trial["query_exit"], 0);
    assert_eq!(trial["adapter_exit"], 0);
    assert_eq!(trial["comparator_exit"], 0);
    assert_eq!(trial["truncated"], false);
    assert_eq!(trial["end_audit_ok"], true);
    assert_eq!(trial["partial"], false);
    assert_eq!(trial["backend_cleanup"], "unknown");
    assert_eq!(trial["comparator_verdict"], "match");
    assert_quiet(&stdout);
    let again = run_at(&suite_path, &run);
    assert_eq!(again.0, 2);
    assert!(again.1.is_empty());
    assert_eq!(again.2, b"sql-runner: run_dir /run_dir\n");
    assert_eq!(fs::read(run.join("report.json")).expect("kept"), stdout);
}

#[test]
fn mixed_suite_keeps_every_planned_trial() {
    let dir = scratch("mixed");
    secrets(&dir);
    let exact = request(&dir, "SELECT note FROM dispatch.ticket /* A231-MATCH */");
    let changed = request(
        &dir,
        "SELECT note FROM dispatch.ticket /* A231-MATCH */ /* changed */",
    );
    let hidden = request(&dir, "SELECT seal FROM dispatch.ticket /* A231-HIDE */");
    let noend = request(&dir, "SELECT note FROM dispatch.ticket /* A231-NOEND */");
    let trunc = request(&dir, "SELECT note FROM dispatch.ticket /* A231-TRUNC */");
    let refuse = request(
        &dir,
        "SELECT note FROM dispatch.ticket /* A231-MATCH */ /* set */",
    );
    let slow = request(&dir, "SELECT note FROM dispatch.ticket /* A231-SLOW */");
    let reann = request(
        &dir,
        "SELECT note FROM dispatch.ticket /* A231-MATCH */ /* reann */",
    );
    let suite_path = write_suite(
        &dir,
        &suite(
            "synthetic",
            vec![
                case("exact", 2, "score", 5000, &exact, "gold-full.json"),
                case("changed", 1, "score", 5000, &changed, "gold-changed.json"),
                case("hidden", 1, "score", 5000, &hidden, "gold-full.json"),
                case("noend", 1, "score", 5000, &noend, "gold-full.json"),
                case("trunc", 1, "score", 5000, &trunc, "gold-prefix.json"),
                case("refuse", 1, "score", 5000, &refuse, "gold-set.json"),
                case("slow", 1, "score", 300, &slow, "gold-full.json"),
                case(
                    "reann",
                    1,
                    "needs_reannotation",
                    5000,
                    &reann,
                    "gold-full.json",
                ),
            ],
        ),
    );
    let run = dir.join("run");
    let started = Instant::now();
    let (code, stdout, err) = run_at(&suite_path, &run);
    assert!(
        started.elapsed().as_secs() < 10,
        "deadline did not kill the slow child"
    );
    assert_eq!(code, 1, "{}", String::from_utf8_lossy(&err));
    assert!(err.is_empty());
    let report = parse_report(&stdout);
    assert_eq!(fs::read(run.join("report.json")).expect("report"), stdout);
    assert_eq!(report["passed"], false);
    assert_eq!(report["question_count"], 8);
    assert_eq!(report["trial_count"], 9);
    assert_eq!(report["pass_count"], 2);
    assert_eq!(report["fail_count"], 5);
    assert_eq!(report["timeout_count"], 1);
    assert_eq!(report["reannotation_count"], 1);
    assert_eq!(report["not_run_count"], 0);
    assert_eq!(report["trials"].as_array().expect("trials").len(), 9);

    for index in [0, 1] {
        let trial = trial(&report, "exact", index);
        assert_eq!(trial["status"], "pass");
        assert_eq!(trial["reason"], "match");
        assert_eq!(trial["backend_cleanup"], "unknown");
    }
    let changed_trial = trial(&report, "changed", 0);
    assert_eq!(changed_trial["status"], "fail");
    assert_eq!(changed_trial["reason"], "mismatch");
    assert_eq!(changed_trial["comparator_exit"], 1);
    assert_eq!(changed_trial["comparator_verdict"], "mismatch");
    let hidden_trial = trial(&report, "hidden", 0);
    assert_eq!(hidden_trial["status"], "fail");
    assert_eq!(hidden_trial["reason"], "refused");
    assert_eq!(hidden_trial["query_exit"], 6);
    assert!(hidden_trial["adapter_exit"].is_null());
    assert_eq!(hidden_trial["comparator_verdict"], "absent");
    let noend_trial = trial(&report, "noend", 0);
    assert_eq!(noend_trial["reason"], "no_end");
    assert_eq!(noend_trial["query_exit"], 8);
    assert!(noend_trial["end_audit_ok"].is_null());
    assert!(noend_trial["adapter_exit"].is_null());
    let trunc_trial = trial(&report, "trunc", 0);
    assert_eq!(trunc_trial["status"], "fail");
    assert_eq!(trunc_trial["reason"], "truncated");
    assert_eq!(trunc_trial["truncated"], true);
    assert_eq!(trunc_trial["query_exit"], 0);
    assert_eq!(trunc_trial["adapter_exit"], 0);
    assert_eq!(trunc_trial["comparator_exit"], 0);
    assert_eq!(trunc_trial["comparator_verdict"], "match");
    let refuse_trial = trial(&report, "refuse", 0);
    assert_eq!(refuse_trial["reason"], "comparator_refused");
    assert_eq!(refuse_trial["comparator_exit"], 2);
    assert_eq!(refuse_trial["comparator_verdict"], "absent");
    let slow_trial = trial(&report, "slow", 0);
    assert_eq!(slow_trial["status"], "timeout");
    assert_eq!(slow_trial["reason"], "timeout");
    assert!(slow_trial["query_exit"].is_null());
    assert!(slow_trial["adapter_exit"].is_null());
    assert_eq!(slow_trial["backend_cleanup"], "unknown");
    let reann_trial = trial(&report, "reann", 0);
    assert_eq!(reann_trial["status"], "needs_reannotation");
    assert_eq!(reann_trial["reason"], "needs_reannotation");
    assert_eq!(reann_trial["query_exit"], 0);
    assert_eq!(reann_trial["comparator_exit"], 0);
    assert_eq!(reann_trial["comparator_verdict"], "match");
    assert_quiet(&stdout);

    let left = fs::read_to_string(run.join("case-exact/trial-0/request.json")).expect("request");
    let right = fs::read_to_string(run.join("case-exact/trial-1/request.json")).expect("request");
    assert_ne!(left, right);
    assert!(left.contains("trial-0/audit.jsonl"));
    assert!(right.contains("trial-1/audit.jsonl"));
    assert!(!left.contains(CANARY));
    assert!(!left.contains(SECRET));
    assert!(run.join("case-exact/trial-0/audit.jsonl").is_file());
    assert!(run.join("case-exact/trial-1/audit.jsonl").is_file());
    assert!(!run.join("case-hidden/trial-0/audit.jsonl").exists());
    assert!(!run.join("case-hidden/trial-0/adapter.stdin").exists());
    let trunc_stdout = fs::read_to_string(run.join("case-trunc/trial-0/comparator.stdout"))
        .expect("trunc compare");
    assert!(trunc_stdout.contains("\"verdict\":\"match\""));
    let changed_stdout = fs::read_to_string(run.join("case-changed/trial-0/comparator.stdout"))
        .expect("changed compare");
    assert!(changed_stdout.contains(CANARY));
    let adapter_in = fs::read(run.join("case-exact/trial-0/adapter.stdin")).expect("adapter stdin");
    assert!(
        !adapter_in
            .windows(CANARY.len())
            .any(|window| window == CANARY.as_bytes())
    );
}

#[test]
fn flood_partial_and_missing_completion_are_failures() {
    let dir = scratch("edges");
    secrets(&dir);
    let flood = request(&dir, "SELECT note FROM dispatch.ticket /* A231-FLOOD */");
    let partial = request(&dir, "SELECT note FROM dispatch.ticket /* A231-PARTIAL */");
    let audit = request(
        &dir,
        "SELECT note FROM dispatch.ticket /* A231-AUDITFALSE */",
    );
    let suite_path = write_suite(
        &dir,
        &suite(
            "synthetic",
            vec![
                case("flood", 1, "score", 5000, &flood, "gold-full.json"),
                case("partial", 1, "score", 5000, &partial, "gold-full.json"),
                case("audit", 1, "score", 5000, &audit, "gold-full.json"),
            ],
        ),
    );
    let run = dir.join("run");
    let (code, stdout, err) = run_at(&suite_path, &run);
    assert_eq!(code, 1, "{}", String::from_utf8_lossy(&err));
    assert!(err.is_empty());
    let report = parse_report(&stdout);
    assert_eq!(report["trial_count"], 3);
    assert_eq!(report["pass_count"], 0);
    assert_eq!(report["fail_count"], 3);
    let flood_trial = trial(&report, "flood", 0);
    assert_eq!(flood_trial["status"], "fail");
    assert_eq!(flood_trial["reason"], "budget");
    assert!(flood_trial["adapter_exit"].is_null());
    let partial_trial = trial(&report, "partial", 0);
    assert_eq!(partial_trial["reason"], "partial");
    assert_eq!(partial_trial["partial"], true);
    assert_eq!(partial_trial["query_exit"], 8);
    assert!(partial_trial["adapter_exit"].is_null());
    let audit_trial = trial(&report, "audit", 0);
    assert_eq!(audit_trial["reason"], "no_end");
    assert_eq!(audit_trial["end_audit_ok"], false);
    assert_eq!(audit_trial["query_exit"], 7);
    assert!(audit_trial["comparator_exit"].is_null());
    assert_quiet(&stdout);
}
