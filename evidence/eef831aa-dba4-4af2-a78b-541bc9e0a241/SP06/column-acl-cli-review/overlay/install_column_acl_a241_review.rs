//! Independent review of install-column-acl at the fixed commit.
//!
//! The live path is gated by OO_A241_LIVE. It drives the compiled
//! install-column-acl binary, then grants mapping read and runs ANALYZE
//! itself, then drives the compiled query-pg binary. Those two steps are
//! not performed by the installer. A second compiled chain pastes the
//! reviewed-policy producer DTO into an install request.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use oo_gateway::{connect_require_tls, TlsEndpoint};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;

const PROTOCOL: &str = "oo.gateway.install-column-acl-cli/v1";
const HOST: &str = "oo-a241-pg";
const DATABASE: &str = "oo_a241";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a241_login";
const SECRET_DIR: &str = "/run/oo-a241/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";
const OWNER: &str = "a2410000-0000-4000-8000-000000000241";
const SHARD: &str = "shard_a241";
const SOURCE: &str = "trade";
const CONFIG: &str = "a241a241a241a241a241a241a241a241a241a241a241a241a241a241a241a241";
const TYPE_DIGEST: &str = "b241b241b241b241b241b241b241b241b241b241b241b241b241b241b241b241";
const INPUT_DIGEST: &str = "c241c241c241c241c241c241c241c241c241c241c241c241c241c241c241c241";
const SCHEMA_DIGEST: &str = "d241d241d241d241d241d241d241d241d241d241d241d241d241d241d241d241";
const NAMESPACE: &str = "trade";
const PARTY: &str = "party";
const LEDGER: &str = "ledger";
const STALE_GENERATION: i64 = 3;
const CURRENT_GENERATION: i64 = 7;
const PRODUCER_PROTOCOL: &str = "oo.gateway.reviewed-policy-cli/v1";
const PRODUCER_DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PRODUCER_OWNER: &str = "owner_role_do_not_leak";
const PRODUCER_CAPTURED: &str = "2026-10-04T12:00:00.000000Z";

#[derive(Clone, Copy)]
struct PartyGold {
    party_id: i32,
    display_name: &'static str,
    note: &'static str,
}

const PARTY_GOLD: [PartyGold; 2] = [
    PartyGold {
        party_id: 41,
        display_name: "ada",
        note: "kept-local",
    },
    PartyGold {
        party_id: 42,
        display_name: "bea",
        note: "other-local",
    },
];

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct InstallOutcome {
    protocol: String,
    effect: String,
    role: String,
    grantor: String,
    tables: Vec<PhysicalTable>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PhysicalTable {
    physical_table: String,
    physical_columns: Vec<String>,
}

struct PartyMap {
    table: String,
    party_id: String,
    display_name: String,
    note: String,
    ratio: String,
}

struct LedgerMap {
    table: String,
    entry_id: String,
    party_id: String,
    memo: String,
}

struct Shadow {
    party: PartyMap,
    ledger: LedgerMap,
}

impl PartyMap {
    fn column(&self, name: &str) -> String {
        match name {
            "party_id" => self.party_id.clone(),
            "display_name" => self.display_name.clone(),
            "note" => self.note.clone(),
            "ratio" => self.ratio.clone(),
            _ => panic!("unknown party column {name}"),
        }
    }
}

impl LedgerMap {
    fn column(&self, name: &str) -> String {
        match name {
            "entry_id" => self.entry_id.clone(),
            "party_id" => self.party_id.clone(),
            "memo" => self.memo.clone(),
            _ => panic!("unknown ledger column {name}"),
        }
    }
}

struct Oracle {
    lines: Vec<String>,
    bans: Vec<String>,
}

impl Oracle {
    fn new(bans: &[String]) -> Self {
        Self {
            lines: Vec::new(),
            bans: bans.to_vec(),
        }
    }

    fn fact(&mut self, key: &str, value: &str) {
        assert!(!key.contains("pass"), "oracle key");
        assert_quiet(value.as_bytes(), &self.bans, key);
        self.lines.push(format!("{key}={value}"));
        let path = Path::new(ORACLE_PATH);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("oracle dir");
        }
        fs::write(path, self.lines.join("\n") + "\n").expect("oracle");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn review_private_tls_install_and_query() {
    assert_installer_separate();
    if env::var("OO_A241_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    assert_eq!(CONFIG.len(), 64);
    let admin_pass = read_secret("admin.pass");
    let login_pass = read_secret("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
    let bans = vec![admin_pass.clone(), login_pass.clone()];
    let mut oracle = Oracle::new(&bans);
    reset_database(&admin_pass, &ca).await;
    let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    apply_frozen_schema(&mut admin).await;
    create_login(&admin, LOGIN, &login_pass).await;
    let shadow = prepare_shadow(&mut admin).await;
    assert_eq!(max_generation(&admin, CONFIG).await, Some(CURRENT_GENERATION));

    refuse_stale_and_bad_second(&admin, &bans, &mut oracle).await;
    let clerk = install_two_roles(&admin, &shadow, &bans, &mut oracle).await;
    hidden_write(&admin, &shadow, &login_pass, &ca, &mut oracle).await;
    separate_query(&admin, &shadow, &bans, &mut oracle).await;
    paste_producer(&admin, &bans, &mut oracle).await;
    assert_eq!(column_grant_count(&admin, "oo_r_clerk").await, 6);
    let _ = clerk;
}

fn assert_installer_separate() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/install-column-acl.rs");
    let text = fs::read_to_string(path).expect("install-column-acl source");
    assert!(!text.contains("query-pg"), "installer names query-pg");
    assert!(!text.contains("ANALYZE"), "installer analyzes");
    assert!(!text.contains("GRANT"), "installer grants");
    assert!(
        !text.contains("provision_mapping_reader"),
        "installer provisions mapping read"
    );
}

async fn refuse_stale_and_bad_second(admin: &Client, bans: &[String], oracle: &mut Oracle) {
    eprintln!("phase=stale");
    let stale = run_cli(live_doc(
        "stale_gen",
        STALE_GENERATION,
        &[("party", &["party_id"]), ("ledger", &["entry_id"])],
    ))
    .await;
    assert_line(&stale, 7, "install-column-acl: policy stale");
    assert_quiet(&stale.stderr, bans, "stale stderr");
    assert!(!role_present(admin, "oo_r_stale_gen").await);
    oracle.fact("stale", "policy stale");
    oracle.fact("stale_role", "absent");

    eprintln!("phase=bad-second");
    let bad = run_cli(live_doc(
        "bad_map",
        CURRENT_GENERATION,
        &[
            ("party", &["party_id", "display_name", "note"]),
            ("missing_table", &["entry_id"]),
        ],
    ))
    .await;
    assert_line(&bad, 7, "install-column-acl: policy mapping");
    assert!(!role_present(admin, "oo_r_bad_map").await);
    assert_eq!(column_grant_count(admin, "oo_r_bad_map").await, 0);
    oracle.fact("bad_second", "policy mapping");
    oracle.fact("bad_second_role", "absent");
}

async fn install_two_roles(
    admin: &Client,
    shadow: &Shadow,
    bans: &[String],
    oracle: &mut Oracle,
) -> InstallOutcome {
    eprintln!("phase=install");
    let clerk_bytes = live_doc(
        "clerk",
        CURRENT_GENERATION,
        &[
            ("party", &["party_id", "display_name", "note"]),
            ("ledger", &["entry_id", "party_id", "memo"]),
        ],
    );
    let clerk = run_cli(clerk_bytes).await;
    let clerk_body = assert_ok(&clerk, bans);
    assert_outcome_prefix(&clerk.stdout, "installed", "oo_r_clerk");
    let clerk_gold = InstallOutcome {
        protocol: PROTOCOL.to_string(),
        effect: "installed".to_string(),
        role: "oo_r_clerk".to_string(),
        grantor: ADMIN.to_string(),
        tables: vec![
            PhysicalTable {
                physical_table: shadow.party.table.clone(),
                physical_columns: vec![
                    shadow.party.column("party_id"),
                    shadow.party.column("display_name"),
                    shadow.party.column("note"),
                ],
            },
            PhysicalTable {
                physical_table: shadow.ledger.table.clone(),
                physical_columns: vec![
                    shadow.ledger.column("entry_id"),
                    shadow.ledger.column("party_id"),
                    shadow.ledger.column("memo"),
                ],
            },
        ],
    };
    assert_eq!(clerk_body, clerk_gold);
    assert!(!clerk_body.tables[0]
        .physical_columns
        .contains(&shadow.party.ratio));
    oracle.fact("clerk_effect", "installed");
    oracle.fact("grantor", ADMIN);

    eprintln!("phase=replay");
    let replay = run_cli(live_doc(
        "clerk",
        CURRENT_GENERATION,
        &[
            ("party", &["party_id", "display_name", "note"]),
            ("ledger", &["entry_id", "party_id", "memo"]),
        ],
    ))
    .await;
    let replay_body = assert_ok(&replay, bans);
    assert_outcome_prefix(&replay.stdout, "replay", "oo_r_clerk");
    assert_eq!(replay_body.effect, "replay");
    assert_eq!(replay_body.tables, clerk_body.tables);
    assert_eq!(replay_body.role, clerk_body.role);
    assert_eq!(replay_body.grantor, clerk_body.grantor);
    oracle.fact("replay_effect", "replay");
    oracle.fact("replay_tables", "same");

    eprintln!("phase=peek");
    let peek = run_cli(live_doc(
        "peek",
        CURRENT_GENERATION,
        &[
            ("party", &["party_id", "display_name"]),
            ("ledger", &["entry_id", "party_id"]),
        ],
    ))
    .await;
    let peek_body = assert_ok(&peek, bans);
    assert_eq!(peek_body.effect, "installed");
    assert_eq!(peek_body.role, "oo_r_peek");
    assert_eq!(peek_body.grantor, ADMIN);
    assert_eq!(
        peek_body.tables[0].physical_columns,
        vec![
            shadow.party.column("party_id"),
            shadow.party.column("display_name")
        ]
    );
    assert_eq!(
        peek_body.tables[1].physical_columns,
        vec![
            shadow.ledger.column("entry_id"),
            shadow.ledger.column("party_id")
        ]
    );
    oracle.fact("peek_effect", "installed");
    assert_eq!(column_grant_count(admin, "oo_r_clerk").await, 6);
    assert_eq!(column_grant_count(admin, "oo_r_peek").await, 4);
    assert_eq!(table_privilege_count(admin, "oo_r_clerk").await, 0);
    assert_eq!(table_privilege_count(admin, "oo_r_peek").await, 0);
    oracle.fact("column_grants_clerk", "6");
    oracle.fact("column_grants_peek", "4");
    oracle.fact("table_privileges", "0");
    assert_role_shape(admin, "oo_r_clerk").await;
    assert_role_shape(admin, "oo_r_peek").await;
    oracle.fact("role_shape", "nologin");
    clerk_body
}

async fn hidden_write(
    admin: &Client,
    shadow: &Shadow,
    login_pass: &str,
    ca: &[u8],
    oracle: &mut Oracle,
) {
    eprintln!("phase=hidden");
    let mut reader = connect_db(DATABASE, LOGIN, login_pass, ca).await;
    let party = quote_physical(&shadow.party.table, "pt_");
    let ledger = quote_physical(&shadow.ledger.table, "pt_");
    let party_id = quote_physical(&shadow.party.party_id, "pc_");
    let display = quote_physical(&shadow.party.display_name, "pc_");
    let note = quote_physical(&shadow.party.note, "pc_");
    let memo = quote_physical(&shadow.ledger.memo, "pc_");
    let row = query_as_role(
        &mut reader,
        "oo_r_clerk",
        &format!(
            "SELECT {party_id}::int4, {display}::text, {note}::text \
             FROM \"_oo\".{party} ORDER BY {party_id}::int4 LIMIT 1"
        ),
    )
    .await;
    assert_eq!(row.get::<_, i32>(0), PARTY_GOLD[0].party_id);
    assert_eq!(row.get::<_, String>(1), PARTY_GOLD[0].display_name);
    assert_eq!(row.get::<_, String>(2), PARTY_GOLD[0].note);
    oracle.fact("allowed_party_id", &PARTY_GOLD[0].party_id.to_string());
    expect_42501(
        &mut reader,
        "oo_r_clerk",
        &format!("INSERT INTO \"_oo\".{party} (\"_oo_pos\") VALUES (9)"),
    )
    .await;
    expect_42501(
        &mut reader,
        "oo_r_clerk",
        &format!("UPDATE \"_oo\".{ledger} SET {memo} = 'rewritten'"),
    )
    .await;
    oracle.fact("hidden_write", "42501");
    assert_eq!(mapping_privilege_count(admin).await, 0);
    oracle.fact("mapping_before", "0");
    let _ = admin;
}

async fn separate_query(admin: &Client, shadow: &Shadow, bans: &[String], oracle: &mut Oracle) {
    eprintln!("phase=mapping-grant");
    grant_mapping_read(admin, LOGIN).await;
    let flags = mapping_flags(admin).await;
    assert_eq!(flags, [true, true, true], "explicit mapping grants");
    oracle.fact("mapping_grant", "explicit");
    assert!(reltuples_unknown(admin, &shadow.party.table).await);
    assert!(reltuples_unknown(admin, &shadow.ledger.table).await);
    oracle.fact("reltuples_before", "unknown");

    eprintln!("phase=query-before-analyze");
    let before = run_query_pg(bans, "before.jsonl").await;
    let before_stderr = String::from_utf8_lossy(&before.stderr).to_string();
    oracle.fact(
        "query_before_analyze",
        before_stderr.trim_end().strip_prefix("query-pg: ").unwrap_or("other"),
    );
    assert_eq!(before.status.code(), Some(5), "{before_stderr}");
    assert_eq!(before_stderr, "query-pg: policy\n");
    assert!(before.stdout.is_empty());
    assert!(!before.audit.exists(), "policy refusal created an audit file");

    eprintln!("phase=analyze");
    analyze_table(admin, &shadow.party.table).await;
    analyze_table(admin, &shadow.ledger.table).await;
    assert!(!reltuples_unknown(admin, &shadow.party.table).await);
    assert!(!reltuples_unknown(admin, &shadow.ledger.table).await);
    oracle.fact("reltuples_after", "known");

    eprintln!("phase=query-after-analyze");
    let after = run_query_pg(bans, "after.jsonl").await;
    assert_eq!(
        after.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&after.stderr)
    );
    assert!(after.stderr.is_empty());
    assert_quiet(&after.stdout, bans, "query stdout");
    let rows = query_rows(&after.stdout);
    assert_eq!(rows.len(), PARTY_GOLD.len());
    for (got, gold) in rows.iter().zip(PARTY_GOLD) {
        assert_eq!(got[0], ("int".to_string(), gold.party_id.to_string()));
        assert_eq!(got[1], ("text".to_string(), gold.display_name.to_string()));
        assert_eq!(got[2], ("text".to_string(), gold.note.to_string()));
    }
    let summary = query_summary(&after.stdout);
    assert_eq!(summary["status"], "succeeded");
    assert_eq!(summary["returned_rows"], json!(2));
    assert_eq!(summary["end_audit_ok"], true);
    assert_eq!(summary["turn"], json!(0));
    fs::write("/evidence/live/query-clerk.stdout", &after.stdout).expect("save query");
    let receipt = fs::read(&after.audit).expect("query receipt");
    assert_quiet(&receipt, bans, "query receipt");
    oracle.fact("query_after_analyze", "succeeded");
    oracle.fact("query_rows", "2");
    oracle.fact("query_receipt", "local");
    oracle.fact("audit_wal", "not_claimed");
    oracle.fact("authentication", "not_claimed");
    oracle.fact("publication", "not_claimed");
    oracle.fact("rls", "not_claimed");
}

async fn paste_producer(admin: &Client, bans: &[String], oracle: &mut Oracle) {
    eprintln!("phase=producer");
    let before_roles = role_count(admin, "oo_r_").await;
    let before_grants = column_grant_count(admin, "oo_r_clerk").await;
    let policies = producer_policies();
    let body = json!({
        "protocol": PROTOCOL,
        "policies": policies,
        "endpoint": {
            "host": HOST,
            "port": 5432,
            "database": DATABASE,
            "user": ADMIN
        },
        "password_file": Path::new(SECRET_DIR).join("admin.pass").display().to_string(),
        "ca_file": Path::new(SECRET_DIR).join("ca.crt").display().to_string()
    });
    let bytes = serde_json::to_vec(&body).expect("producer install json");
    assert_quiet(&bytes, bans, "producer request");
    let output = run_cli(bytes).await;
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let token = stderr
        .trim_end()
        .strip_prefix("install-column-acl: ")
        .unwrap_or("other");
    oracle.fact("producer_effect", token);
    assert_eq!(output.status.code(), Some(7), "{stderr}");
    assert!(output.stdout.is_empty());
    assert_eq!(stderr, "install-column-acl: policy stale\n");
    assert_eq!(role_count(admin, "oo_r_").await, before_roles);
    assert_eq!(column_grant_count(admin, "oo_r_clerk").await, before_grants);
    oracle.fact("producer_roles", "unchanged");
    oracle.fact("producer_grants", "unchanged");
}

struct QueryRun {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    audit: PathBuf,
}

async fn run_query_pg(bans: &[String], audit_name: &str) -> QueryRun {
    let work = env::temp_dir().join(format!("oo-a241-query-{}", std::process::id()));
    fs::create_dir_all(&work).expect("query work");
    let audit = work.join(audit_name);
    let _ = fs::remove_file(&audit);
    let request = query_request(&audit);
    let text = serde_json::to_vec(&request).expect("query json");
    assert_quiet(&text, bans, "query request");
    let path = work.join(audit_name.replace(".jsonl", ".json"));
    fs::write(&path, &text).expect("query request file");
    let bin = bin_path("query-pg");
    let output = tokio::task::spawn_blocking(move || {
        Command::new(bin)
            .arg("--request")
            .arg(path)
            .env_remove("PGPASSWORD")
            .env_remove("PGPASSFILE")
            .output()
            .expect("run query-pg")
    })
    .await
    .expect("query join");
    QueryRun {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
        audit,
    }
}

fn query_request(audit: &Path) -> Value {
    json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": {
            "host": HOST,
            "port": 5432,
            "database": DATABASE,
            "user": LOGIN
        },
        "password_file": Path::new(SECRET_DIR).join("login.pass").display().to_string(),
        "ca_file": Path::new(SECRET_DIR).join("ca.crt").display().to_string(),
        "audit_file": audit.display().to_string(),
        "identity": {
            "project": SOURCE,
            "role": "oo_r_clerk",
            "policy_id": CONFIG
        },
        "policies": [
            query_policy("clerk", CURRENT_GENERATION, "party", &["party_id", "display_name", "note"]),
            query_policy("clerk", CURRENT_GENERATION, "ledger", &["entry_id", "party_id", "memo"])
        ],
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1_000_000,
            "unknown_rows": "refuse"
        },
        "statement": {
            "source": "request",
            "sql": "SELECT party_id, display_name, note FROM trade.party ORDER BY party_id",
            "max_rows": 20,
            "max_result_bytes": 4096,
            "wall_clock_ms": 30000
        }
    })
}

fn query_rows(stdout: &[u8]) -> Vec<Vec<(String, String)>> {
    let text = std::str::from_utf8(stdout).expect("query utf-8");
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line).expect("query line");
        if value["record"] != "row" {
            continue;
        }
        let cells = value["cells"].as_array().expect("cells");
        rows.push(
            cells
                .iter()
                .map(|cell| {
                    (
                        cell["kind"].as_str().expect("kind").to_string(),
                        cell["value"].as_str().expect("value").to_string(),
                    )
                })
                .collect(),
        );
    }
    rows
}

fn query_summary(stdout: &[u8]) -> Value {
    let text = std::str::from_utf8(stdout).expect("query utf-8");
    for line in text.lines() {
        let value: Value = serde_json::from_str(line).expect("query line");
        if value["record"] == "summary" {
            return value;
        }
    }
    panic!("query summary missing");
}

fn bin_path(name: &str) -> PathBuf {
    let underscored = name.replace('-', "_");
    for key in [
        format!("CARGO_BIN_EXE_{name}"),
        format!("CARGO_BIN_EXE_{underscored}"),
    ] {
        if let Ok(path) = env::var(&key)
            && !path.is_empty()
        {
            return PathBuf::from(path);
        }
    }
    let mut path = env::current_exe().expect("current_exe");
    path.pop();
    path.pop();
    path.push(name);
    path
}

fn run_cli_blocking(bytes: &[u8]) -> Output {
    let mut child = Command::new(bin_path("install-column-acl"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("PGPASSWORD")
        .env_remove("PGPASSFILE")
        .spawn()
        .expect("spawn install-column-acl");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(bytes)
        .expect("write stdin");
    child.wait_with_output().expect("wait install-column-acl")
}

async fn run_cli(bytes: Vec<u8>) -> Output {
    tokio::task::spawn_blocking(move || run_cli_blocking(&bytes))
        .await
        .expect("cli join")
}

fn assert_line(output: &Output, code: i32, line: &str) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "stdout was not empty");
    assert_eq!(output.stderr, format!("{line}\n").as_bytes());
}

fn assert_ok(output: &Output, bans: &[String]) -> InstallOutcome {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.ends_with(b"\n"));
    assert_quiet(&output.stdout, bans, "stdout");
    serde_json::from_slice(&output.stdout).expect("outcome")
}

fn assert_quiet(bytes: &[u8], bans: &[String], label: &str) {
    let text = String::from_utf8_lossy(bytes);
    for ban in bans {
        assert!(!text.contains(ban.as_str()), "{label} contained a secret");
    }
}

fn assert_outcome_prefix(bytes: &[u8], effect: &str, role: &str) {
    let prefix = format!(
        "{{\"protocol\":\"{PROTOCOL}\",\"effect\":\"{effect}\",\"role\":\"{role}\",\"grantor\":\"{ADMIN}\",\"tables\":["
    );
    assert!(
        bytes.starts_with(prefix.as_bytes()),
        "outcome key order {}",
        String::from_utf8_lossy(bytes)
    );
}

fn policy_json(role: &str, generation: i64, table: &str, columns: &[&str]) -> Value {
    json!({
        "project": SOURCE,
        "config_digest": CONFIG,
        "generation": generation,
        "role": role,
        "login": LOGIN,
        "table": {
            "namespace": NAMESPACE,
            "name": table,
            "columns": columns.iter().map(|name| json!({
                "name": name,
                "status": "confirmed",
                "semantic_type": "identifier",
                "pii": {"class": "none"}
            })).collect::<Vec<_>>()
        },
        "row_filter": Value::Null
    })
}

fn query_policy(role: &str, generation: i64, table: &str, columns: &[&str]) -> Value {
    let mut value = policy_json(role, generation, table, columns);
    value
        .as_object_mut()
        .expect("policy object")
        .remove("row_filter");
    value
}

fn live_doc(role: &str, generation: i64, tables: &[(&str, &[&str])]) -> Vec<u8> {
    let policies: Vec<Value> = tables
        .iter()
        .map(|(table, columns)| policy_json(role, generation, table, columns))
        .collect();
    serde_json::to_vec(&json!({
        "protocol": PROTOCOL,
        "policies": policies,
        "endpoint": {
            "host": HOST,
            "port": 5432,
            "database": DATABASE,
            "user": ADMIN
        },
        "password_file": Path::new(SECRET_DIR).join("admin.pass").display().to_string(),
        "ca_file": Path::new(SECRET_DIR).join("ca.crt").display().to_string()
    }))
    .expect("live json")
}

fn read_secret(name: &str) -> String {
    let raw = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret file");
    let value = raw.trim();
    let hex = (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    assert!(hex, "{name} is not lowercase hex");
    value.to_string()
}

async fn reset_database(admin_pass: &str, ca: &[u8]) {
    let maintenance = connect_db("postgres", ADMIN, admin_pass, ca).await;
    expect_sql(
        &maintenance,
        "terminate database sessions",
        &format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid()"
        ),
    )
    .await;
    expect_sql(
        &maintenance,
        "drop database",
        &format!("DROP DATABASE IF EXISTS \"{DATABASE}\""),
    )
    .await;
    expect_sql(
        &maintenance,
        "drop fixture roles",
        "DO $$\n\
         DECLARE granted_name text;\n\
         DECLARE member_name text;\n\
         DECLARE role_name text;\n\
         BEGIN\n\
           FOR granted_name, member_name IN\n\
             SELECT granted.rolname, member_role.rolname\n\
             FROM pg_catalog.pg_auth_members AS membership\n\
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid\n\
             JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member\n\
             WHERE granted.rolname LIKE 'oo\\_r\\_%' ESCAPE '\\'\n\
                OR member_role.rolname = 'oo_a241_login'\n\
           LOOP\n\
             EXECUTE format('REVOKE %I FROM %I', granted_name, member_name);\n\
           END LOOP;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a241_login') THEN\n\
             DROP ROLE oo_a241_login;\n\
           END IF;\n\
           FOR role_name IN\n\
             SELECT rolname FROM pg_catalog.pg_roles\n\
             WHERE rolname LIKE 'oo\\_r\\_%' ESCAPE '\\'\n\
           LOOP\n\
             EXECUTE format('DROP ROLE %I', role_name);\n\
           END LOOP;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_pipeline_runtime') THEN\n\
             DROP ROLE oo_pipeline_runtime;\n\
           END IF;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner') THEN\n\
             DROP ROLE oo_target_owner;\n\
           END IF;\n\
         END\n\
         $$",
    )
    .await;
    expect_sql(
        &maintenance,
        "create database",
        &format!("CREATE DATABASE \"{DATABASE}\" TEMPLATE template0 ENCODING 'UTF8'"),
    )
    .await;
}

async fn apply_frozen_schema(client: &mut Client) {
    let bootstrap = migration("0000_bootstrap_roles.sql");
    let fence = migration("0001_target_fence_expand.sql");
    let preflight = migration("0003_target_segment_preflight_expand.sql");
    let mapping = migration("0004_physical_mapping.sql");
    let full_import = migration("0005_full_import_expand.sql");
    let tx = begin_locked(client).await;
    exec_named(&tx, "0000_bootstrap_roles.sql", &bootstrap.0).await;
    exec_named(&tx, "set-owner", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0001_target_fence_expand.sql", &fence.0).await;
    exec_named(
        &tx,
        "row-lock",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_named(&tx, "0003_target_segment_preflight_expand.sql", &preflight.0).await;
    exec_named(&tx, "0004_physical_mapping.sql", &mapping.0).await;
    exec_named(
        &tx,
        "markers-target",
        &marker_sql(&[
            ("target-bootstrap/1", &bootstrap.1, "0000_bootstrap_roles.sql"),
            ("target-fence-receipts/1", &fence.1, "0001_target_fence_expand.sql"),
            (
                "target-fence-receipts/3",
                &preflight.1,
                "0003_target_segment_preflight_expand.sql",
            ),
            ("physical-mapping/v1", &mapping.1, "0004_physical_mapping.sql"),
        ]),
    )
    .await;
    tx.commit().await.expect("commit target schema");
    let tx = begin_locked(client).await;
    exec_named(
        &tx,
        "row-lock-import",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_named(
        &tx,
        "stats-grant",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
    )
    .await;
    exec_named(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0005_full_import_expand.sql", &full_import.0).await;
    exec_named(
        &tx,
        "marker-import",
        &marker_sql(&[(
            "full-import/v1",
            &full_import.1,
            "0005_full_import_expand.sql",
        )]),
    )
    .await;
    tx.commit().await.expect("commit full import");
}

fn migration(name: &str) -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oo-pipeline/migrations")
        .join(name);
    let bytes = fs::read(&path).unwrap_or_else(|err| panic!("read {name}: {err}"));
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{name} is not utf-8"));
    (text, sha256_hex(&bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
}

fn marker_sql(rows: &[(&str, &str, &str)]) -> String {
    let mut values = String::new();
    for (index, (version, checksum, filename)) in rows.iter().enumerate() {
        assert!(closed_token(version), "version token");
        assert!(closed_token(filename), "filename token");
        assert_eq!(checksum.len(), 64, "checksum length");
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!(
            "  ('{version}', '{checksum}', 'expand', '{filename}')"
        ));
    }
    format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}")
}

fn closed_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '.' | '-'))
}

async fn begin_locked(client: &mut Client) -> tokio_postgres::Transaction<'_> {
    let tx = client.transaction().await.expect("begin");
    exec_named(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    exec_named(&tx, "statement-timeout", "SET LOCAL statement_timeout = '60s'").await;
    exec_named(
        &tx,
        "advisory-lock",
        "SELECT pg_catalog.pg_advisory_xact_lock(6404202641)",
    )
    .await;
    tx
}

async fn expect_sql(client: &Client, name: &str, sql: &str) {
    client
        .batch_execute(sql)
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {}", db_detail(&err)));
}

fn db_detail(err: &tokio_postgres::Error) -> String {
    err.as_db_error()
        .map(|db| db.to_string())
        .unwrap_or_else(|| err.to_string())
}

async fn exec_named(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql)
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {}", db_detail(&err)));
}

async fn create_login(client: &Client, name: &str, password: &str) {
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(name.bytes().all(|byte| {
        byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit()
    }));
    client
        .batch_execute(&format!(
            "CREATE ROLE \"{name}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{name}\""
        ))
        .await
        .expect("create login");
}

async fn prepare_shadow(client: &mut Client) -> Shadow {
    let target: i64 = client
        .query_one(
            "SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)",
            &[&SHARD, &OWNER],
        )
        .await
        .expect("adopt")
        .get(0);
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)",
            &[&SHARD, &OWNER, &target],
        )
        .await
        .expect("bind");
    for generation in [STALE_GENERATION, CURRENT_GENERATION] {
        register(client, &map_key(CONFIG, generation, "namespace", "n", "", "")).await;
        register(client, &map_key(CONFIG, generation, "table", "t", PARTY, "")).await;
        register(client, &map_key(CONFIG, generation, "table", "t", LEDGER, "")).await;
        for column in ["party_id", "display_name", "note", "ratio"] {
            register(client, &map_key(CONFIG, generation, "column", "c", PARTY, column)).await;
        }
        for column in ["entry_id", "party_id", "memo"] {
            register(client, &map_key(CONFIG, generation, "column", "c", LEDGER, column)).await;
        }
    }
    let party_table = lookup(client, "table", PARTY, "").await;
    let ledger_table = lookup(client, "table", LEDGER, "").await;
    let party_id = lookup(client, "column", PARTY, "party_id").await;
    let display_name = lookup(client, "column", PARTY, "display_name").await;
    let note = lookup(client, "column", PARTY, "note").await;
    let ratio = lookup(client, "column", PARTY, "ratio").await;
    let entry_id = lookup(client, "column", LEDGER, "entry_id").await;
    let ledger_party = lookup(client, "column", LEDGER, "party_id").await;
    let memo = lookup(client, "column", LEDGER, "memo").await;
    let run_id = open_run(client, target, &party_table, &ledger_table).await;
    prepare_one(
        client,
        target,
        &run_id,
        0,
        PARTY,
        &["party_id", "display_name", "note", "ratio"],
        &[23, 25, 25, 700],
    )
    .await;
    prepare_one(
        client,
        target,
        &run_id,
        1,
        LEDGER,
        &["entry_id", "party_id", "memo"],
        &[23, 23, 25],
    )
    .await;
    let shadow = Shadow {
        party: PartyMap {
            table: party_table,
            party_id,
            display_name,
            note,
            ratio,
        },
        ledger: LedgerMap {
            table: ledger_table,
            entry_id,
            party_id: ledger_party,
            memo,
        },
    };
    insert_rows(client, &shadow).await;
    shadow
}

async fn open_run(client: &Client, target: i64, party: &str, ledger: &str) -> String {
    let schemas = vec![NAMESPACE.to_string(), NAMESPACE.to_string()];
    let tables = vec![PARTY.to_string(), LEDGER.to_string()];
    let physical = vec![party.to_string(), ledger.to_string()];
    let params: [&(dyn ToSql + Sync); 12] = [
        &SHARD,
        &OWNER,
        &target,
        &SOURCE,
        &CURRENT_GENERATION,
        &CONFIG,
        &TYPE_DIGEST,
        &INPUT_DIGEST,
        &SCHEMA_DIGEST,
        &schemas,
        &tables,
        &physical,
    ];
    client
        .query_one(
            "SELECT _oo.full_import_open_run($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &params,
        )
        .await
        .expect("open run")
        .get(0)
}

async fn prepare_one(
    client: &Client,
    target: i64,
    run_id: &str,
    index: i32,
    table: &str,
    idents: &[&str],
    oids: &[i64],
) {
    let run_owned = run_id.to_string();
    let names: Vec<String> = idents.iter().map(|name| (*name).to_string()).collect();
    let oid_values = oids.to_vec();
    let typmods = vec![-1_i32; idents.len()];
    let required = vec![true; idents.len()];
    let namespace = NAMESPACE.to_string();
    let table_name = table.to_string();
    let params: [&(dyn ToSql + Sync); 14] = [
        &SHARD,
        &OWNER,
        &target,
        &run_owned,
        &index,
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &namespace,
        &table_name,
        &names,
        &oid_values,
        &typmods,
        &required,
    ];
    let prepared: String = client
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, ($2::text)::uuid, $3, ($4::text)::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &params,
        )
        .await
        .expect("prepare shadow")
        .get(0);
    assert!(prepared.starts_with("pt_"), "{prepared}");
}

async fn insert_rows(client: &mut Client, shadow: &Shadow) {
    let party_sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {party_id}, {display_name}, {note}, {ratio}) \
         VALUES (($1::text)::numeric, $2, $3, $4, ($5::text)::real)",
        table = quote_physical(&shadow.party.table, "pt_"),
        party_id = quote_physical(&shadow.party.party_id, "pc_"),
        display_name = quote_physical(&shadow.party.display_name, "pc_"),
        note = quote_physical(&shadow.party.note, "pc_"),
        ratio = quote_physical(&shadow.party.ratio, "pc_"),
    );
    let ledger_sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {entry_id}, {party_id}, {memo}) \
         VALUES (($1::text)::numeric, $2, $3, $4)",
        table = quote_physical(&shadow.ledger.table, "pt_"),
        entry_id = quote_physical(&shadow.ledger.entry_id, "pc_"),
        party_id = quote_physical(&shadow.ledger.party_id, "pc_"),
        memo = quote_physical(&shadow.ledger.memo, "pc_"),
    );
    let tx = client.transaction().await.expect("insert begin");
    let first: [&(dyn ToSql + Sync); 5] = [
        &"1",
        &PARTY_GOLD[0].party_id,
        &PARTY_GOLD[0].display_name,
        &PARTY_GOLD[0].note,
        &"1.25",
    ];
    let second: [&(dyn ToSql + Sync); 5] = [
        &"2",
        &PARTY_GOLD[1].party_id,
        &PARTY_GOLD[1].display_name,
        &PARTY_GOLD[1].note,
        &"2.25",
    ];
    tx.execute(&party_sql, &first).await.expect("insert party 41");
    tx.execute(&party_sql, &second).await.expect("insert party 42");
    let entry_a: [&(dyn ToSql + Sync); 4] = [&"10", &70_i32, &PARTY_GOLD[0].party_id, &"memo-seven"];
    let entry_b: [&(dyn ToSql + Sync); 4] = [&"11", &71_i32, &PARTY_GOLD[1].party_id, &"memo-eight"];
    tx.execute(&ledger_sql, &entry_a).await.expect("insert ledger 70");
    tx.execute(&ledger_sql, &entry_b).await.expect("insert ledger 71");
    tx.commit().await.expect("insert commit");
}

struct MapKey<'a> {
    digest: &'a str,
    generation: i64,
    level: &'a str,
    letter: &'a str,
    table: &'a str,
    column: &'a str,
}

fn map_key<'a>(
    digest: &'a str,
    generation: i64,
    level: &'a str,
    letter: &'a str,
    table: &'a str,
    column: &'a str,
) -> MapKey<'a> {
    MapKey {
        digest,
        generation,
        level,
        letter,
        table,
        column,
    }
}

async fn register(client: &Client, key: &MapKey<'_>) {
    let namespace = NAMESPACE;
    let payload: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &key.digest,
        &key.generation,
        &key.letter,
        &namespace,
        &key.table,
        &key.column,
    ];
    let digest_bytes: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .expect("digest")
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [
        &SOURCE,
        &key.digest,
        &key.generation,
        &key.level,
        &namespace,
        &key.table,
        &key.column,
        &digest_bytes,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .expect("register");
}

async fn lookup(client: &Client, level: &str, table: &str, column: &str) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &level,
        &NAMESPACE,
        &table,
        &column,
    ];
    let name: String = client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &params,
        )
        .await
        .expect("lookup")
        .get(0);
    let prefix = if level == "table" { "pt_" } else { "pc_" };
    let _ = quote_physical(&name, prefix);
    name
}

async fn max_generation(client: &Client, digest: &str) -> Option<i64> {
    client
        .query_one(
            "SELECT max(run_generation) FROM _oo.physical_mappings WHERE source = $1 AND config_digest = $2",
            &[&SOURCE, &digest],
        )
        .await
        .expect("max generation")
        .get(0)
}

async fn role_count(client: &Client, prefix: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM pg_catalog.pg_roles WHERE left(rolname, length($1)) = $1",
            &[&prefix],
        )
        .await
        .expect("role count")
        .get(0)
}

async fn role_present(client: &Client, name: &str) -> bool {
    client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)",
            &[&name],
        )
        .await
        .expect("role present")
        .get(0)
}

async fn column_grant_count(client: &Client, role: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.column_privileges WHERE grantee = $1",
            &[&role],
        )
        .await
        .expect("column grants")
        .get(0)
}

async fn table_privilege_count(client: &Client, role: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.table_privileges WHERE grantee = $1",
            &[&role],
        )
        .await
        .expect("table privileges")
        .get(0)
}

async fn mapping_privilege_count(client: &Client) -> i64 {
    client
        .query_one(
            "SELECT \
               (SELECT count(*) FROM information_schema.column_privileges \
                 WHERE grantee IN ('oo_r_clerk', 'oo_r_peek', 'oo_a241_login') \
                   AND table_schema = '_oo' AND table_name = 'physical_mappings') \
             + (SELECT count(*) FROM information_schema.table_privileges \
                 WHERE grantee IN ('oo_r_clerk', 'oo_r_peek', 'oo_a241_login') \
                   AND table_schema = '_oo' AND table_name = 'physical_mappings')",
            &[],
        )
        .await
        .expect("mapping privileges")
        .get(0)
}

async fn reltuples_unknown(client: &Client, table: &str) -> bool {
    client
        .query_one(
            "SELECT relation.reltuples < 0 \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $1",
            &[&table],
        )
        .await
        .expect("reltuples")
        .get(0)
}

async fn assert_role_shape(client: &Client, role: &str) {
    let row = client
        .query_one(
            "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, \
                    rolbypassrls, rolreplication \
             FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&role],
        )
        .await
        .expect("role flags");
    let flags = [
        row.get::<_, bool>(0),
        row.get::<_, bool>(1),
        row.get::<_, bool>(2),
        row.get::<_, bool>(3),
        row.get::<_, bool>(4),
        row.get::<_, bool>(5),
        row.get::<_, bool>(6),
    ];
    assert_eq!(flags, [false; 7], "{role}");
}

async fn grant_mapping_read(client: &Client, login: &str) {
    let quoted = quote_role(login);
    let sql = format!(
        "GRANT USAGE ON SCHEMA \"_oo\" TO {quoted};\n\
         GRANT SELECT ON TABLE \"_oo\".physical_mappings TO {quoted};\n\
         GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO {quoted}"
    );
    expect_sql(client, "explicit mapping grant", &sql).await;
}

async fn mapping_flags(client: &Client) -> [bool; 3] {
    let row = client
        .query_one(
            "SELECT \
               has_schema_privilege($1, '_oo', 'USAGE'), \
               has_table_privilege($1, '_oo.physical_mappings', 'SELECT'), \
               has_function_privilege($1, '_oo.lookup_physical_mapping(text, text, bigint, text, text, text, text)', 'EXECUTE')",
            &[&LOGIN],
        )
        .await
        .expect("mapping flags");
    [row.get(0), row.get(1), row.get(2)]
}

async fn analyze_table(client: &Client, table: &str) {
    let quoted = quote_physical(table, "pt_");
    expect_sql(client, "analyze", &format!("ANALYZE \"_oo\".{quoted}")).await;
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical length {name}");
    assert_eq!(rest.len(), 60, "physical body");
    assert!(
        rest.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "physical hex"
    );
    format!("\"{name}\"")
}

async fn connect_db(database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    let mut last = String::new();
    for _ in 0..20 {
        match connect_require_tls(&endpoint, password, ca).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                return client;
            }
            Err(err) => {
                last = err.to_string();
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect to {database} as {user} failed: {last}");
}

async fn query_as_role(client: &mut Client, role: &str, sql: &str) -> tokio_postgres::Row {
    let tx = client.transaction().await.expect("begin read");
    let quoted = quote_role(role);
    tx.batch_execute(&format!("SET LOCAL ROLE {quoted}"))
        .await
        .expect("set role");
    let row = tx.query_one(sql, &[]).await.expect("selected read");
    tx.rollback().await.expect("rollback read");
    row
}

async fn expect_42501(client: &mut Client, role: &str, sql: &str) {
    let tx = client.transaction().await.expect("begin denial");
    let quoted = quote_role(role);
    tx.batch_execute(&format!("SET LOCAL ROLE {quoted}"))
        .await
        .expect("set role");
    let err = tx.query_one(sql, &[]).await.expect_err("expected denial");
    let state = err.code().map(|code| code.code().to_string()).unwrap_or_default();
    assert_eq!(state, "42501", "{err}");
    tx.rollback().await.expect("rollback denial");
}

fn quote_role(role: &str) -> String {
    assert!(
        role.bytes().all(|byte| {
            byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit()
        }) && !role.is_empty()
    );
    format!("\"{role}\"")
}

fn producer_policies() -> Vec<Value> {
    let catalog = producer_catalog();
    let catalog_text = serde_json::to_string(&catalog).expect("catalog");
    let artifact_json = run_bin(
        &env::var("OO_A241_OO_BUILDER").expect("oo-builder"),
        &["captured-pg-draft"],
        &catalog_text,
    );
    assert!(!artifact_json.ends_with('\n'), "artifact newline");
    let artifact: Value = serde_json::from_str(&artifact_json).expect("artifact");
    let draft_text = artifact["draft"].to_string();
    assert!(
        !draft_text.contains(PRODUCER_OWNER),
        "draft leaked owner"
    );
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .expect("schema hash")
        .to_string();
    assert_eq!(schema_hash, catalog["schema_hash"].as_str().unwrap());
    let prints = fingerprints_of(&artifact);
    let decisions = shop_decisions(&prints, &schema_hash);
    let decisions_json = serde_json::to_string(&decisions).expect("decisions");
    let command_in = serde_json::to_string(&json!({
        "protocol": "oo.builder.reviewed-effective-oos-command/v1",
        "artifact": artifact,
        "settings": producer_settings(),
        "decisions": decisions
    }))
    .expect("command in");
    let command_json = run_bin(
        &env::var("OO_A241_REVIEWED_EFFECTIVE").expect("reviewed-effective"),
        &[],
        &command_in,
    );
    assert!(!command_json.ends_with('\n'), "command newline");
    assert!(!command_json.contains(PRODUCER_OWNER), "command leaked owner");
    let command: Value = serde_json::from_str(&command_json).expect("command");
    let artifact_sha = command["provenance"]["sealed_source_sha256"]
        .as_str()
        .expect("sealed hash")
        .to_string();
    let operator = serde_json::to_string(&json!({
        "protocol": PRODUCER_PROTOCOL,
        "command": command_json,
        "artifact": artifact_json,
        "decisions": decisions_json,
        "binding": producer_binding(&schema_hash, &artifact_sha)
    }))
    .expect("operator");
    let stdout = run_bin(&bin_path("reviewed-policy").display().to_string(), &[], &operator);
    assert!(!stdout.ends_with('\n'), "policy newline");
    assert!(!stdout.contains(PRODUCER_OWNER), "policy leaked owner");
    fs::write("/evidence/live/producer-policies.json", stdout.as_bytes()).expect("save policies");
    let doc: Value = serde_json::from_str(&stdout).expect("policy set");
    assert_eq!(doc["protocol"], PRODUCER_PROTOCOL);
    doc["policies"].as_array().expect("policies").clone()
}

fn run_bin(program: &str, args: &[&str], stdin_text: &str) -> String {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {program}: {err}"));
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin_text.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    assert!(
        output.status.success(),
        "{program} {args:?} exited {:?}\n{stderr}\n{stdout}",
        output.status.code()
    );
    stdout
}

struct Col {
    attnum: i32,
    name: &'static str,
    oid: i64,
    type_name: &'static str,
}

struct TableSpec {
    name: &'static str,
    columns: Vec<Col>,
    pk: Vec<(i32, &'static str)>,
}

fn column_json(column: &Col) -> Value {
    json!({
        "attnum": column.attnum,
        "name": column.name,
        "name_ident": column.name,
        "type_oid": column.oid,
        "type_schema": "pg_catalog",
        "type_name": column.type_name,
        "typtype": "b",
        "typmod": -1,
        "base_type_oid": Value::Null,
        "numeric_precision": Value::Null,
        "numeric_scale": Value::Null,
        "not_null": true,
        "collation_oid": 0,
        "collation_name": Value::Null,
        "identity": "",
        "generated": "",
        "default_expr": Value::Null,
        "comment": Value::Null,
        "enum_labels": [],
        "domain_checks": [],
        "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
        "current_privileges": {"select": true, "insert": false, "update": false}
    })
}

fn table_json(table: &TableSpec) -> Value {
    json!({
        "schema": "public",
        "schema_ident": "public",
        "name": table.name,
        "name_ident": table.name,
        "relkind": "r",
        "replica_identity": "d",
        "rls_enabled": false,
        "rls_forced": false,
        "owner": PRODUCER_OWNER,
        "comment": Value::Null,
        "columns": table.columns.iter().map(column_json).collect::<Vec<_>>(),
        "primary_key": table.pk.iter().map(key_json).collect::<Vec<_>>(),
        "primary_key_nulls_not_distinct": false,
        "unique_keys": [],
        "foreign_keys": [],
        "policies": [],
        "privileges": [{"grantee": PRODUCER_OWNER, "privilege": "SELECT", "grantable": false}]
    })
}

fn key_json(key: &(i32, &str)) -> Value {
    json!({"attnum": key.0, "name": key.1})
}

fn producer_catalog() -> Value {
    let tables = shop_tables();
    let hash = sha256_hex(schema_preimage(&tables).as_bytes());
    json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "shop_src",
        "database": "shop",
        "captured_at": PRODUCER_CAPTURED,
        "schema_hash": hash,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": tables.iter().map(table_json).collect::<Vec<_>>(),
        "type_conversion": "unsupported"
    })
}

fn shop_tables() -> Vec<TableSpec> {
    vec![
        TableSpec {
            name: "customer",
            columns: vec![
                Col { attnum: 1, name: "id", oid: 23, type_name: "int4" },
                Col { attnum: 2, name: "email", oid: 25, type_name: "text" },
                Col { attnum: 3, name: "active", oid: 16, type_name: "bool" },
            ],
            pk: vec![(1, "id")],
        },
        TableSpec {
            name: "purchase",
            columns: vec![
                Col { attnum: 1, name: "id", oid: 23, type_name: "int4" },
                Col { attnum: 2, name: "customer_id", oid: 23, type_name: "int4" },
                Col { attnum: 3, name: "note", oid: 25, type_name: "text" },
            ],
            pk: vec![(1, "id")],
        },
    ]
}

fn schema_preimage(tables: &[TableSpec]) -> String {
    let mut ordered: Vec<&TableSpec> = tables.iter().collect();
    ordered.sort_by(|left, right| left.name.cmp(right.name));
    let mut buf = String::from("{\"tables\":[");
    for (index, table) in ordered.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_pair(&mut buf, "schema", "public");
        buf.push(',');
        write_pair(&mut buf, "name", table.name);
        buf.push(',');
        write_pair(&mut buf, "relkind", "r");
        buf.push(',');
        write_pair(&mut buf, "replica_identity", "d");
        buf.push(',');
        write_bool(&mut buf, "rls", false);
        buf.push(',');
        write_bool(&mut buf, "force_rls", false);
        buf.push_str(",\"columns\":[");
        for (col_index, column) in table.columns.iter().enumerate() {
            if col_index > 0 {
                buf.push(',');
            }
            buf.push('{');
            write_i32(&mut buf, "attnum", column.attnum);
            buf.push(',');
            write_pair(&mut buf, "name", column.name);
            buf.push(',');
            write_i64(&mut buf, "type_oid", column.oid);
            buf.push_str(",\"typmod\":-1,\"not_null\":true,\"collation_oid\":0,\"typtype\":\"b\",\"base_type_oid\":null,\"identity\":\"\",\"generated\":\"\",\"default_sha256\":null,\"enum_labels\":[],\"domain_checks\":[]}");
        }
        buf.push_str("],\"primary_key\":[");
        for (key_index, (attnum, name)) in table.pk.iter().enumerate() {
            if key_index > 0 {
                buf.push(',');
            }
            buf.push_str("{\"attnum\":");
            buf.push_str(&attnum.to_string());
            buf.push_str(",\"name\":");
            push_string(&mut buf, name);
            buf.push('}');
        }
        buf.push_str("],\"primary_key_nulls_not_distinct\":false,\"unique_keys\":[],\"foreign_keys\":[],\"policies\":[]}");
    }
    buf.push_str("]}");
    buf
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn write_pair(buf: &mut String, name: &str, value: &str) {
    push_string(buf, name);
    buf.push(':');
    push_string(buf, value);
}

fn write_bool(buf: &mut String, name: &str, value: bool) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn write_i32(buf: &mut String, name: &str, value: i32) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_i64(buf: &mut String, name: &str, value: i64) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn fingerprints_of(artifact: &Value) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    let queue = artifact["draft"]["review_queue"].as_array().expect("review queue");
    for item in queue {
        if item["kind"] != "structural_column" {
            continue;
        }
        let item_id = item["item_id"].as_str().expect("item id");
        let Some(rest) = item_id.strip_prefix("column/") else {
            continue;
        };
        let Some((dataset, column)) = rest.rsplit_once('/') else {
            continue;
        };
        let fingerprint = item["fact_fingerprint"].as_str().expect("fingerprint").to_string();
        out.insert((dataset.to_string(), column.to_string()), fingerprint);
    }
    out
}

fn shop_decisions(prints: &BTreeMap<(String, String), String>, schema_hash: &str) -> Vec<Value> {
    vec![
        decision(prints, schema_hash, "public.customer", "id", "none", "identifier"),
        decision(prints, schema_hash, "public.customer", "email", "personal", "email"),
        decision(prints, schema_hash, "public.customer", "active", "none", "boolean_flag"),
        decision(prints, schema_hash, "public.purchase", "id", "none", "identifier"),
        decision(prints, schema_hash, "public.purchase", "customer_id", "none", "foreign_key"),
        decision(prints, schema_hash, "public.purchase", "note", "sensitive", "free_text"),
    ]
}

fn decision(
    prints: &BTreeMap<(String, String), String>,
    schema_hash: &str,
    dataset: &str,
    column: &str,
    pii: &str,
    semantic: &str,
) -> Value {
    let fingerprint = prints
        .get(&(dataset.to_string(), column.to_string()))
        .unwrap_or_else(|| panic!("missing fingerprint {dataset}.{column}"))
        .clone();
    json!({
        "dataset": dataset,
        "column": column,
        "schema_hash": schema_hash,
        "fact_fingerprint": fingerprint,
        "pii": pii,
        "semantic_type": semantic,
        "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
    })
}

fn producer_settings() -> Value {
    json!({
        "project": "shop",
        "ontology_version": "ont-1",
        "catalog_version": "cat-1",
        "policy": {"id": "pol-shop", "version": "1"},
        "knowledge": {"id": "know-shop", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "shop",
            "locale": "en-US",
            "include_schemas": ["public"],
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

fn producer_binding(schema_hash: &str, artifact_sha: &str) -> Value {
    json!({
        "identity": {
            "project": "shop",
            "ontology_version": "ont-1",
            "catalog_version": "cat-1",
            "policy_id": "pol-shop",
            "policy_version": "1",
            "knowledge_id": "know-shop",
            "knowledge_version": "1"
        },
        "seal": {
            "source_id": "shop_src",
            "schema": "public",
            "schema_hash": schema_hash,
            "artifact_sha256": artifact_sha
        },
        "config_digest": PRODUCER_DIGEST,
        "generation": 4,
        "role": "clerk",
        "login": "clerk_login",
        "allow": [
            {"dataset": "public.customer", "column": "id", "explicit_allow": false},
            {"dataset": "public.customer", "column": "email", "explicit_allow": true},
            {"dataset": "public.customer", "column": "active", "explicit_allow": false},
            {"dataset": "public.purchase", "column": "id", "explicit_allow": false},
            {"dataset": "public.purchase", "column": "customer_id", "explicit_allow": false},
            {"dataset": "public.purchase", "column": "note", "explicit_allow": true}
        ]
    })
}
