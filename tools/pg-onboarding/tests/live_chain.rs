#![forbid(unsafe_code)]

//! Private TLS proof. The chain script sets `OO_A261_*` and passes `--ignored`.
//! This file does not open PostgreSQL itself. Gold checks read the CLI step files.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use pg_onboarding::{config_identity_hex, sha256_hex};
use serde_json::{json, Value};

const DIGEST: &str = "a4a889fba1436935fb1fe1af9c30a8a79109794d94600fa4ce9d2e1b21f7b618";
const MARKER: &str = "甲注评";

const DECISIONS: &[(&str, &str, &str, &str)] = &[
    ("shop.customer", "customer_id", "none", "identifier"),
    ("shop.customer", "display_name", "personal", "person_name"),
    ("shop.customer", "active", "none", "boolean_flag"),
    ("shop.order_doc", "order_id", "none", "identifier"),
    ("shop.order_doc", "customer_id", "none", "foreign_key"),
    ("shop.order_doc", "memo", "none", "free_text"),
    ("shop.order_doc", "posted", "none", "boolean_flag"),
    ("shop.order_doc", "amount", "none", "quantity"),
    ("shop.order_doc", "clerk_note", "sensitive", "free_text"),
];

#[test]
#[ignore = "one private PostgreSQL; linux/chain/inside.sh sets OO_A261_*"]
fn live_chain() {
    let bin = env_path("OO_A261_ONBOARDING");
    let bins = env_path("OO_A261_BINS");
    let secrets = env_path("OO_A261_SECRETS");
    let run = env_path("OO_A261_RUN");
    let work = env_path("OO_A261_WORK");
    let ready = env_path("OO_A261_READY");
    assert!(ready.is_absolute());
    assert!(!run.exists(), "run directory must be created by inspect");
    fs::create_dir_all(&work).unwrap();

    let pins = pin_map(&bins);
    let inspect_cfg = work.join("inspect.json");
    write_json(&inspect_cfg, &inspect_config(&pins, &secrets, &ready));
    let inspect = run_cli(&bin, &["inspect", "--config", inspect_cfg.to_str().unwrap(), "--run", run.to_str().unwrap()]);
    assert!(
        inspect.status.success(),
        "inspect failed\n{}",
        explain(&run, &inspect.stderr)
    );

    let manifest = read_json(&run.join("inspect-manifest.json"));
    let outcome = read_json(&run.join("outcome.json"));
    assert_eq!(outcome["status"], "awaiting_review");
    assert_eq!(manifest["mapping_run_generation"], 48);
    assert_eq!(manifest["source_cut"], 48);
    assert_ne!(manifest["target_fence"], 48);
    assert_eq!(manifest["config_identity_sha256"], DIGEST);
    let recomputed = config_identity_hex(
        "a261src",
        "oo-a261-pg",
        5432,
        "oo_a261_src",
        "oo_a261_reader",
        "secret://pg/a261/reader",
        &[("shop", "customer"), ("shop", "order_doc")],
    );
    assert_eq!(recomputed, DIGEST);
    assert_absent(&run.join("catalog.json"), MARKER);
    assert_absent(&run.join("captured-draft.json"), MARKER);
    assert_eq!(outcome["non_claims"]["mapping_rows_read"], false);
    assert_eq!(outcome["non_claims"]["config_identity"], "recomputed_from_non_secret_request_fields");

    let captured = read_json(&run.join("captured-draft.json"));
    write_operator(&work, &captured);
    let apply_cfg = work.join("apply.json");
    write_json(&apply_cfg, &apply_config(&pins, &secrets, &work));
    let apply = run_cli(
        &bin,
        &["apply-reviewed", "--config", apply_cfg.to_str().unwrap(), "--run", run.to_str().unwrap()],
    );
    assert!(
        apply.status.success(),
        "apply-reviewed failed\n{}",
        explain(&run, &apply.stderr)
    );

    let applied = read_json(&run.join("apply-outcome.json"));
    assert_eq!(applied["status"], "ready_for_trusted_query");
    assert_eq!(applied["mapping_run_generation"], 48);
    assert_ne!(applied["target_fence"], 48);
    assert_eq!(applied["analyze_published"], false);
    assert_eq!(applied["estimate_is_exact"], false);
    assert_eq!(applied["database_rollback"], false);
    assert_eq!(applied["database_release"], false);
    assert_non_claims(&applied["non_claims"]);
    assert_eq!(applied["provision"][0]["effect"], "installed");
    assert_eq!(applied["provision"][1]["effect"], "replay");
    let hidden = applied["hidden"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == "shop.customer" && item["column"] == "private_note")
        .unwrap();
    assert_eq!(hidden["reason"], "classification.absent");
    assert_install(&run);
    assert_query_gold(&run, &secrets);
    assert!(run.join("captured-draft.json").is_file());

    let again = run_cli(
        &bin,
        &["apply-reviewed", "--config", apply_cfg.to_str().unwrap(), "--run", run.to_str().unwrap()],
    );
    assert!(!again.status.success());
    let stderr = String::from_utf8_lossy(&again.stderr);
    assert!(stderr.contains("terminal"), "{stderr}");
    fs::write(work.join("oracle-ok.txt"), b"ready_for_trusted_query\n").unwrap();
}

fn env_path(key: &str) -> PathBuf {
    PathBuf::from(env::var(key).unwrap_or_else(|_| panic!("missing {key}")))
}

fn pin_map(dir: &Path) -> Vec<(&'static str, String, String)> {
    [
        "init-pg",
        "oo-pipeline",
        "oo-builder",
        "reviewed-effective-oos",
        "reviewed-policy",
        "install-column-acl",
        "provision-mapping-reader",
        "analyze-pg",
        "role-pack-bundle",
        "query-pg",
    ]
    .into_iter()
    .map(|name| {
        let path = dir.join(name);
        let bytes = fs::read(&path).unwrap_or_else(|_| panic!("missing bin {}", path.display()));
        (name, path.display().to_string(), sha256_hex(&bytes))
    })
    .collect()
}

fn pin(pins: &[(&str, String, String)], name: &str) -> Value {
    let found = pins.iter().find(|item| item.0 == name).unwrap();
    json!({"path": found.1, "sha256": found.2})
}

fn inspect_config(pins: &[( &'static str, String, String)], secrets: &Path, ready: &Path) -> Value {
    json!({
        "protocol": "oo.onboarding.inspect/v1",
        "caller": "trusted_local_admin",
        "binaries": {
            "init_pg": pin(pins, "init-pg"),
            "import_pg": pin(pins, "oo-pipeline"),
            "catalog_pg": pin(pins, "oo-pipeline"),
            "captured_pg_draft": pin(pins, "oo-builder")
        },
        "source": {
            "source_id": "a261src",
            "host": "oo-a261-pg",
            "port": 5432,
            "database": "oo_a261_src",
            "username": "oo_a261_reader",
            "secret_ref": "secret://pg/a261/reader",
            "password_file": secrets.join("reader.pass"),
            "ca_file": secrets.join("ca.crt"),
            "tables": [
                {"schema": "shop", "table": "customer"},
                {"schema": "shop", "table": "order_doc"}
            ]
        },
        "target": {
            "host": "oo-a261-pg",
            "port": 5432,
            "database": "oo_a261_dst",
            "administrator": "oo_fixture_admin",
            "runtime_principal": "oo_a261_runtime",
            "admin_password_file": secrets.join("admin.pass"),
            "runtime_password_file": secrets.join("runtime.pass"),
            "ca_file": secrets.join("ca.crt")
        },
        "shard": "a261main",
        "owner": "eef831aa-dba4-4af2-a78b-541bc9e0a261",
        "source_generation": 48,
        "connect_ms": 30000,
        "runtime_membership_ready_file": ready,
        "membership_wait_ms": 180000,
        "comment_markers": [MARKER]
    })
}

fn apply_config(pins: &[( &'static str, String, String)], secrets: &Path, work: &Path) -> Value {
    json!({
        "protocol": "oo.onboarding.apply-reviewed/v1",
        "caller": "trusted_local_admin",
        "binaries": {
            "review_pg_draft": pin(pins, "oo-builder"),
            "reviewed_effective_oos": pin(pins, "reviewed-effective-oos"),
            "reviewed_policy": pin(pins, "reviewed-policy"),
            "install_column_acl": pin(pins, "install-column-acl"),
            "provision_mapping_reader": pin(pins, "provision-mapping-reader"),
            "analyze_pg": pin(pins, "analyze-pg"),
            "role_pack": pin(pins, "oo-builder"),
            "role_pack_bundle": pin(pins, "role-pack-bundle"),
            "query_pg": pin(pins, "query-pg")
        },
        "reviews_file": work.join("reviews.json"),
        "decisions_file": work.join("decisions.json"),
        "settings": settings(),
        "schema": "shop",
        "query_login": {
            "user": "oo_a261_login",
            "password_file": secrets.join("login.pass")
        },
        "roles": [
            role("clerk", clerk_allow(), work.join("vis-clerk.json"), clerk_queries()),
            role("peek", peek_allow(), work.join("vis-peek.json"), peek_queries())
        ],
        "analyze": {
            "statement_timeout_ms": 60000,
            "lock_timeout_ms": 30000,
            "connect_ms": 30000
        }
    })
}

fn settings() -> Value {
    json!({
        "project": "a261src",
        "ontology_version": "ont-a261",
        "catalog_version": "cat-a261",
        "policy": {"id": "pol-a261", "version": "1"},
        "knowledge": {"id": "know-a261", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "shop",
            "locale": "en-US",
            "include_schemas": ["shop"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

fn write_operator(work: &Path, captured: &Value) {
    let schema_hash = captured["wrapper"]["schema_hash"].as_str().unwrap();
    assert_eq!(schema_hash.len(), 64);
    let queue = captured["draft"]["review_queue"].as_array().unwrap();
    let mut reviews = Vec::new();
    for item in queue {
        let kind = item["kind"].as_str().unwrap_or("");
        let item_id = item["item_id"].as_str().unwrap_or("");
        let structural = matches!(kind, "structural_column" | "structural_primary_key" | "candidate_relationship");
        let skipped = item_id.starts_with("pii/")
            || item_id.starts_with("semantic/")
            || item_id.starts_with("type/")
            || item_id.starts_with("classification/");
        if !structural || skipped {
            continue;
        }
        reviews.push(json!({
            "item_id": item_id,
            "action": "accept",
            "schema_hash": schema_hash,
            "fact_fingerprint": item["fact_fingerprint"].as_str().unwrap()
        }));
    }
    assert!(!reviews.is_empty());
    write_json(&work.join("reviews.json"), &Value::Array(reviews));
    let mut decisions = Vec::new();
    for (dataset, column, pii, semantic) in DECISIONS {
        let item_id = format!("column/{dataset}/{column}");
        let fingerprint = queue
            .iter()
            .find(|item| item["item_id"] == item_id && item["kind"] == "structural_column")
            .and_then(|item| item["fact_fingerprint"].as_str())
            .unwrap_or_else(|| panic!("missing fingerprint {item_id}"));
        decisions.push(json!({
            "dataset": dataset,
            "column": column,
            "schema_hash": schema_hash,
            "fact_fingerprint": fingerprint,
            "pii": pii,
            "semantic_type": semantic,
            "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
        }));
    }
    write_json(&work.join("decisions.json"), &Value::Array(decisions));
    write_json(&work.join("vis-clerk.json"), &visibility("oo_r_clerk", &["customer_id", "display_name", "active"], &["order_id", "customer_id", "memo", "posted", "amount", "clerk_note"]));
    write_json(&work.join("vis-peek.json"), &visibility("oo_r_peek", &["customer_id", "display_name"], &["order_id", "customer_id", "posted"]));
}

fn visibility(role: &str, customer: &[&str], order: &[&str]) -> Value {
    json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "a261src",
        "principal": "fixture_operator",
        "role": role,
        "ontology_version": "ont-a261",
        "catalog_version": "cat-a261",
        "policy": {"id": "pol-a261", "version": "1"},
        "knowledge": {"id": "know-a261", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [
            {"dataset": "shop.customer", "row_security": "none", "global_stats": "denied", "columns": customer},
            {"dataset": "shop.order_doc", "row_security": "none", "global_stats": "denied", "columns": order}
        ],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    })
}

fn role(slug: &str, allow: Value, visibility_file: PathBuf, queries: Value) -> Value {
    json!({
        "slug": slug,
        "allow": allow,
        "visibility_file": visibility_file,
        "queries": queries
    })
}

fn clerk_allow() -> Value {
    allow(&[
        ("shop.customer", "customer_id"),
        ("shop.customer", "display_name"),
        ("shop.customer", "active"),
        ("shop.order_doc", "order_id"),
        ("shop.order_doc", "customer_id"),
        ("shop.order_doc", "memo"),
        ("shop.order_doc", "posted"),
        ("shop.order_doc", "amount"),
        ("shop.order_doc", "clerk_note"),
    ])
}

fn peek_allow() -> Value {
    allow(&[
        ("shop.customer", "customer_id"),
        ("shop.customer", "display_name"),
        ("shop.order_doc", "order_id"),
        ("shop.order_doc", "customer_id"),
        ("shop.order_doc", "posted"),
    ])
}

fn allow(columns: &[(&str, &str)]) -> Value {
    Value::Array(
        columns
            .iter()
            .map(|pair| {
                let (dataset, column) = *pair;
                let pii = DECISIONS
                    .iter()
                    .find(|item| item.0 == dataset && item.1 == column)
                    .map(|item| item.2)
                    .unwrap();
                json!({
                    "dataset": dataset,
                    "column": column,
                    "explicit_allow": matches!(pii, "personal" | "sensitive")
                })
            })
            .collect(),
    )
}

fn clerk_queries() -> Value {
    json!([
        query("clerk-before", "before_analyze", 5, Some("query-pg: policy"), "SELECT customer_id FROM shop.customer ORDER BY customer_id"),
        query("clerk-hidden", "after_bundle", 6, Some("query-pg: guard"), "SELECT private_note FROM shop.customer"),
        query("clerk-insert", "after_bundle", 6, Some("query-pg: guard"), "INSERT INTO shop.customer (customer_id, display_name, active) VALUES (99, 'x', true)"),
        query("clerk-pos", "after_bundle", 6, Some("query-pg: guard"), "SELECT customer_id FROM shop.customer ORDER BY _oo_pos"),
        query("clerk-customer", "after_bundle", 0, None, "SELECT customer_id, display_name, active FROM shop.customer ORDER BY customer_id"),
        query("clerk-orders", "after_bundle", 0, None, "SELECT order_id, customer_id, memo, posted, amount, clerk_note FROM shop.order_doc ORDER BY order_id"),
        query("clerk-count", "after_bundle", 0, None, "SELECT count(*) FROM shop.order_doc")
    ])
}

fn peek_queries() -> Value {
    json!([
        query("peek-customer", "after_bundle", 0, None, "SELECT customer_id, display_name FROM shop.customer ORDER BY customer_id"),
        query("peek-orders", "after_bundle", 0, None, "SELECT order_id, customer_id, posted FROM shop.order_doc ORDER BY order_id")
    ])
}

fn query(name: &str, when: &str, expected_exit: i32, expected_stderr: Option<&str>, sql: &str) -> Value {
    json!({
        "name": name,
        "when": when,
        "expected_exit": expected_exit,
        "expected_stderr": expected_stderr,
        "statement": {
            "source": "request",
            "sql": sql,
            "max_rows": 20,
            "max_result_bytes": 8192,
            "wall_clock_ms": 30000
        }
    })
}

fn assert_install(run: &Path) {
    for slug in ["clerk", "peek"] {
        let value = read_json(&run.join("steps").join(format!("install-column-acl-{slug}.stdout")));
        assert_eq!(value["effect"], "installed");
        assert_eq!(value["grantor"], "oo_fixture_admin");
        assert_ne!(value["grantor"], "oo_target_owner");
        assert_eq!(value["role"], format!("oo_r_{slug}"));
    }
}

fn assert_query_gold(run: &Path, secrets: &Path) {
    let passwords = ["admin.pass", "reader.pass", "runtime.pass", "login.pass"]
        .into_iter()
        .map(|name| password(&secrets.join(name)))
        .collect::<Vec<_>>();
    assert_rows(
        run,
        "clerk-customer",
        &[
            vec![cell_int("41"), cell_text("丙号"), cell_bool(true)],
            vec![cell_int("42"), Cell::Null, cell_bool(false)],
            vec![cell_int("43"), cell_text("丁铺"), cell_bool(true)],
        ],
    );
    assert_rows(
        run,
        "clerk-orders",
        &[
            vec![cell_int("701"), cell_int("41"), cell_text("首单"), cell_bool(true), cell_int("-9223372036854775808"), cell_text("柜员甲")],
            vec![cell_int("702"), cell_int("41"), Cell::Null, cell_bool(false), cell_int("9223372036854775807"), Cell::Null],
            vec![cell_int("703"), cell_int("43"), cell_text("零单"), cell_bool(true), cell_int("0"), cell_text("柜员乙")],
        ],
    );
    assert_rows(run, "clerk-count", &[vec![cell_int("3")]]);
    assert_rows(
        run,
        "peek-customer",
        &[
            vec![cell_int("41"), cell_text("丙号")],
            vec![cell_int("42"), Cell::Null],
            vec![cell_int("43"), cell_text("丁铺")],
        ],
    );
    assert_rows(
        run,
        "peek-orders",
        &[
            vec![cell_int("701"), cell_int("41"), cell_bool(true)],
            vec![cell_int("702"), cell_int("41"), cell_bool(false)],
            vec![cell_int("703"), cell_int("43"), cell_bool(true)],
        ],
    );
    let clerk_orders = fs::read(run.join("steps").join("query-clerk-orders.stdout")).unwrap();
    assert!(clerk_orders.windows("柜员甲".len()).any(|window| window == "柜员甲".as_bytes()));
    for name in ["peek-customer", "peek-orders", "clerk-customer", "clerk-count"] {
        let bytes = fs::read(run.join("steps").join(format!("query-{name}.stdout"))).unwrap();
        assert!(!bytes.windows("柜员甲".len()).any(|window| window == "柜员甲".as_bytes()), "{name}");
        assert!(!bytes.windows("内部甲".len()).any(|window| window == "内部甲".as_bytes()), "{name}");
    }
    for name in ["clerk-before", "clerk-hidden", "clerk-insert", "clerk-pos"] {
        let stdout = fs::read(run.join("steps").join(format!("query-{name}.stdout"))).unwrap();
        assert!(stdout.is_empty(), "{name} wrote stdout");
        assert!(!run.join("audit").join(format!("{name}.jsonl")).exists(), "{name} wrote an audit");
    }
    let digest = read_json(&run.join("apply-outcome.json"))["config_identity_sha256"]
        .as_str()
        .unwrap()
        .to_string();
    for (name, role, sql) in [
        ("clerk-customer", "oo_r_clerk", "SELECT customer_id, display_name, active FROM shop.customer ORDER BY customer_id"),
        ("clerk-orders", "oo_r_clerk", "SELECT order_id, customer_id, memo, posted, amount, clerk_note FROM shop.order_doc ORDER BY order_id"),
        ("clerk-count", "oo_r_clerk", "SELECT count(*) FROM shop.order_doc"),
        ("peek-customer", "oo_r_peek", "SELECT customer_id, display_name FROM shop.customer ORDER BY customer_id"),
        ("peek-orders", "oo_r_peek", "SELECT order_id, customer_id, posted FROM shop.order_doc ORDER BY order_id"),
    ] {
        assert_audit(run, name, role, &digest, sql, &passwords);
    }
    for secret in &passwords {
        assert_secret_absent(run, secret);
    }
}

fn assert_audit(run: &Path, name: &str, role: &str, digest: &str, sql: &str, passwords: &[Vec<u8>]) {
    let path = run.join("audit").join(format!("{name}.jsonl"));
    let raw = fs::read(&path).unwrap_or_else(|_| panic!("missing audit {name}"));
    assert!(!raw.windows(b"SELECT".len()).any(|window| window == b"SELECT"), "{name} audit stored SQL");
    for secret in passwords {
        assert!(!raw.windows(secret.len()).any(|window| window == secret.as_slice()), "{name} audit stored a password");
    }
    let lines = json_lines(&raw);
    assert_eq!(lines.len(), 2, "{name} audit records");
    assert_eq!(lines[0]["record"], "admit");
    assert_eq!(lines[1]["record"], "complete");
    assert_eq!(lines[0]["protocol"], "oo.gateway.query-pg-audit/v1");
    assert_eq!(lines[0]["role"], role);
    assert_eq!(lines[0]["project"], "a261src");
    assert_eq!(lines[0]["policy_id"], digest);
    let hash = sha256_hex(sql.as_bytes());
    assert_eq!(lines[0]["original_sha256"], hash);
    assert_eq!(lines[1]["original_sha256"], hash);
    assert_eq!(lines[1]["status"], "succeeded");
}

fn assert_rows(run: &Path, name: &str, expect: &[Vec<Cell>]) {
    let stdout = fs::read(run.join("steps").join(format!("query-{name}.stdout"))).unwrap();
    let lines = json_lines(&stdout);
    let rows: Vec<&Value> = lines.iter().filter(|line| line["record"] == "row" && line["turn"] == 0).collect();
    assert_eq!(rows.len(), expect.len(), "{name} row count");
    for (index, wanted) in expect.iter().enumerate() {
        let cells = rows[index]["cells"].as_array().unwrap();
        assert_eq!(cells.len(), wanted.len(), "{name} width {index}");
        for (got, cell) in cells.iter().zip(wanted.iter()) {
            assert!(cell.matches(got), "{name} row {index} got {got}");
        }
    }
    let summary = lines.iter().find(|line| line["record"] == "summary" && line["turn"] == 0).unwrap();
    assert_eq!(summary["status"], "succeeded");
    assert_eq!(summary["returned_rows"].as_u64(), Some(u64::try_from(expect.len()).unwrap()));
    assert_eq!(summary["truncated"], false);
}

fn assert_non_claims(value: &Value) {
    assert_eq!(value["parent_acceptance"], false);
    assert_eq!(value["development_unit_approval"], false);
    assert_eq!(value["component_approval"], false);
    assert_eq!(value["m1_i01"], "not_accepted");
    assert_eq!(value["m1_i02"], "not_accepted");
    assert_eq!(value["semantic_publication"], "missing");
    assert_eq!(value["authentication"], "not_claimed");
    assert_eq!(value["authorization"], "not_claimed");
    assert_eq!(value["natural_language"], "missing");
    assert_eq!(value["harness_mount"], "not_performed");
    assert_eq!(value["public_auth"], false);
    assert_eq!(value["mapping_rows_read"], false);
    assert_eq!(value["analyze_published"], false);
}

fn assert_absent(path: &Path, needle: &str) {
    let bytes = fs::read(path).unwrap();
    assert!(
        !bytes.windows(needle.len()).any(|window| window == needle.as_bytes()),
        "{} contains the comment marker",
        path.display()
    );
}

fn assert_secret_absent(run: &Path, secret: &[u8]) {
    for name in ["inspect-manifest.json", "outcome.json", "apply-outcome.json"] {
        let bytes = fs::read(run.join(name)).unwrap();
        assert!(!bytes.windows(secret.len()).any(|window| window == secret), "{name} contains a password");
    }
}

fn password(path: &Path) -> Vec<u8> {
    let mut bytes = fs::read(path).unwrap();
    if bytes.ends_with(b"\n") {
        bytes.pop();
        if bytes.ends_with(b"\r") {
            bytes.pop();
        }
    }
    assert!(!bytes.is_empty());
    bytes
}

enum Cell {
    Null,
    Bool(bool),
    Int(&'static str),
    Text(&'static str),
}

impl Cell {
    fn matches(&self, got: &Value) -> bool {
        match self {
            Self::Null => got["kind"] == "null" && (got.get("value").is_none() || got["value"].is_null()),
            Self::Bool(value) => got["kind"] == "bool" && got["value"] == *value,
            Self::Int(value) => got["kind"] == "int" && got["value"] == *value,
            Self::Text(value) => got["kind"] == "text" && got["value"] == *value,
        }
    }
}

fn cell_int(value: &'static str) -> Cell {
    Cell::Int(value)
}

fn cell_text(value: &'static str) -> Cell {
    Cell::Text(value)
}

fn cell_bool(value: bool) -> Cell {
    Cell::Bool(value)
}

fn json_lines(bytes: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(bytes).unwrap();
    text.lines().filter(|line| !line.is_empty()).map(|line| serde_json::from_str(line).unwrap()).collect()
}

fn read_json(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap_or_else(|_| panic!("missing {}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("json {}", path.display()))
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn run_cli(bin: &Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(bin);
    command.args(args).env_clear().env("HOME", "/tmp").env("LANG", "C.UTF-8");
    if let Ok(path) = env::var("PATH") {
        command.env("PATH", path);
    }
    command.output().unwrap()
}

fn explain(run: &Path, stderr: &[u8]) -> String {
    let outcome = fs::read_to_string(run.join("outcome.json")).unwrap_or_default();
    let apply = fs::read_to_string(run.join("apply-outcome.json")).unwrap_or_default();
    format!("stderr {}\noutcome {outcome}\napply {apply}\n", String::from_utf8_lossy(stderr))
}
