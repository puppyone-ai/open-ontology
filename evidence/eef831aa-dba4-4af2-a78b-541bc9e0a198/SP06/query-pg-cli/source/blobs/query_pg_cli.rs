//! `query-pg` against local files, and one private TLS fixture when `OO_A198_LIVE=1`.
//!
//! The fixture prepares the database. The command under test does not. Passwords
//! stay in files. Oracles are Rust comparisons and SHA-256 digests.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use oo_gateway::policy::catalog::{CatalogLimits, UnknownRows, load_policy_catalog};
use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, InstallEffect, LogicalTable,
    PiiClass, PolicyCatalog, PolicyIdentity, TlsEndpoint, compile_column_acl, connect_require_tls,
    guard_select, install_column_acl,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;
use tokio_postgres::types::ToSql;

const HOST: &str = "oo-a198-pg";
const DATABASE: &str = "oo_a198";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a198_login";
const SECRET_DIR: &str = "/run/oo-a198/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";
const OWNER: &str = "a1980000-0000-4000-8000-000000000198";
const SHARD: &str = "shard_a198";
const SOURCE: &str = "tickets";
const CONFIG: &str = "a198a198a198a198a198a198a198a198a198a198a198a198a198a198a198a198";
const TYPE_DIGEST: &str = "b198b198b198b198b198b198b198b198b198b198b198b198b198b198b198b198";
const INPUT_DIGEST: &str = "c198c198c198c198c198c198c198c198c198c198c198c198c198c198c198c198";
const SCHEMA_DIGEST: &str = "d198d198d198d198d198d198d198d198d198d198d198d198d198d198d198d198";
const NAMESPACE: &str = "dispatch";
const TABLE: &str = "ticket";
const GENERATION: i64 = 7;
const ADVISORY_LOCK: i64 = 64_042_026_198;
const NOTE: &str = "雪の票\u{0301}";
const SEALS: [&str; 3] = ["sealed-left", "sealed-mid", "sealed-right"];
const ENV_PASSWORD: &str = "env-secret-not-used";

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(1);

#[test]
fn help_documents_the_request_and_exits_zero() {
    let output = run_raw(&["--help"], None);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).expect("help utf8");
    for needle in [
        "query-pg --request <path>",
        "password_file",
        "oo.gateway.query-pg/v1",
        "oo.gateway.query-pg-turn/v1",
        "Exit 6",
        "does not install",
    ] {
        assert!(text.contains(needle), "help missing {needle}");
    }
}

#[test]
fn usage_rejects_missing_unknown_and_mixed_arguments() {
    for args in [
        &[][..],
        &["--bogus"],
        &["--request"],
        &["--help", "--request"],
        &["--request", "x", "y"],
        &["--request", ""],
    ] {
        let output = run_raw(args, None);
        assert_eq!(output.status.code(), Some(2), "args {args:?}");
        assert_eq!(output.stderr, b"query-pg: usage\n");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn request_rejects_malformed_documents_without_leaking_the_password() {
    let dir = scratch();
    let password = b"pw-a198-request-secret-value\n";
    let password_path = dir.join("password");
    fs::write(&password_path, password).unwrap();
    let ca_path = dir.join("ca.pem");
    fs::write(&ca_path, b"-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n").unwrap();
    let audit = dir.join("audit.jsonl");

    let cases: Vec<(&str, &[u8])> = vec![
        ("bad-json", b"{"),
        (
            "duplicate",
            br#"{"version":"oo.gateway.query-pg/v1","version":"oo.gateway.query-pg/v1"}"#,
        ),
        ("trailing", b"{}\n[]"),
        ("empty-object", b"{}"),
    ];
    for (name, body) in cases {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        let output = run_raw(&["--request", path.to_str().unwrap()], None);
        assert_eq!(output.status.code(), Some(3), "{name}");
        assert_eq!(output.stderr, b"query-pg: request\n");
        assert!(!bytes_have(&output.stderr, b"pw-a198"), "{name} stderr");
        assert!(!bytes_have(&output.stdout, b"pw-a198"), "{name} stdout");
    }

    let mut with_password_field = base_value(&password_path, &ca_path, &audit, "desk", &["note"], "request");
    with_password_field
        .as_object_mut()
        .unwrap()
        .insert("password".to_string(), Value::String("pw-a198-request-secret-value".to_string()));
    let leaked = dir.join("password-field.json");
    fs::write(&leaked, serde_json::to_vec(&with_password_field).unwrap()).unwrap();
    let output = run_raw(&["--request", leaked.to_str().unwrap()], None);
    assert_eq!(output.status.code(), Some(3));
    assert!(!bytes_have(&output.stderr, b"pw-a198-request-secret-value"));
    assert!(!bytes_have(&output.stdout, b"pw-a198-request-secret-value"));

    let mut literal = base_value(&password_path, &ca_path, &audit, "desk", &["note"], "request");
    literal["statement"]["sql"] = Value::String("pw-a198-request-secret-value".to_string());
    let literal_path = dir.join("literal.json");
    fs::write(&literal_path, serde_json::to_vec(&literal).unwrap()).unwrap();
    let output = run_raw(&["--request", literal_path.to_str().unwrap()], None);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(output.stderr, b"query-pg: request\n");
    assert!(!bytes_have(&output.stdout, b"pw-a198-request-secret-value"));
    assert!(!bytes_have(&output.stderr, b"pw-a198-request-secret-value"));
    assert!(!audit.exists());

    let mut stdin_doc = base_value(&password_path, &ca_path, &dir.join("stdin-audit.jsonl"), "desk", &["note"], "stdin");
    stdin_doc["statement"].as_object_mut().unwrap().remove("sql");
    let stdin_path = dir.join("stdin.json");
    fs::write(&stdin_path, serde_json::to_vec(&stdin_doc).unwrap()).unwrap();
    let mut lines = String::new();
    for index in 0..9 {
        lines.push_str(&format!(
            "{{\"version\":\"oo.gateway.query-pg-turn/v1\",\"sql\":\"SELECT {index}\"}}\n"
        ));
    }
    let output = run_raw(&["--request", stdin_path.to_str().unwrap()], Some(lines.as_bytes()));
    assert_eq!(output.status.code(), Some(3));
    assert!(!dir.join("stdin-audit.jsonl").exists());
}

#[test]
fn policy_and_audit_and_ca_fail_closed() {
    let dir = scratch();
    let password_path = dir.join("password");
    fs::write(&password_path, b"local-password-value\n").unwrap();
    let ca_path = dir.join("ca.pem");
    fs::write(&ca_path, b"not-a-certificate\n").unwrap();
    let audit = dir.join("audit.jsonl");

    let row_filter = dir.join("row-filter.json");
    let mut doc = base_value(&password_path, &ca_path, &audit, "desk", &["note"], "request");
    doc["policies"][0]["row_filter"] = json!({
        "kind": "claims",
        "column": "note",
        "claim": "sub"
    });
    fs::write(&row_filter, serde_json::to_vec(&doc).unwrap()).unwrap();
    let output = run_raw(&["--request", row_filter.to_str().unwrap()], None);
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(output.stderr, b"query-pg: policy\n");
    assert!(!audit.exists());
    assert!(!bytes_have(&output.stderr, b"local-password-value"));

    let mismatch = dir.join("mismatch.json");
    let mut doc = base_value(&password_path, &ca_path, &audit, "desk", &["note"], "request");
    doc["identity"]["role"] = Value::String("desk".to_string());
    fs::write(&mismatch, serde_json::to_vec(&doc).unwrap()).unwrap();
    let output = run_raw(&["--request", mismatch.to_str().unwrap()], None);
    assert_eq!(output.status.code(), Some(5), "identity slug is not the database role");
    assert!(!audit.exists());

    let mixed = dir.join("mixed.json");
    let mut doc = base_value(&password_path, &ca_path, &audit, "desk", &["note"], "request");
    let peek = policy_value("peek", &["note"]);
    doc["policies"].as_array_mut().unwrap().push(peek);
    fs::write(&mixed, serde_json::to_vec(&doc).unwrap()).unwrap();
    let output = run_raw(&["--request", mixed.to_str().unwrap()], None);
    assert_eq!(output.status.code(), Some(5));
    assert!(!audit.exists());

    let garbage = dir.join("garbage.json");
    let doc = base_value(&password_path, &ca_path, &audit, "desk", &["note"], "request");
    fs::write(&garbage, serde_json::to_vec(&doc).unwrap()).unwrap();
    let output = run_raw(&["--request", garbage.to_str().unwrap()], None);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(output.stderr, b"query-pg: tls\n");
    assert!(!audit.exists());
    assert!(!bytes_have(&output.stderr, b"local-password-value"));
    assert!(!bytes_have(&output.stdout, b"not-a-certificate"));

    let key_ca = dir.join("key.pem");
    fs::write(&key_ca, b"-----BEGIN PRIVATE KEY-----\nMIIB\n").unwrap();
    let key_doc = base_value(&password_path, &key_ca, &dir.join("key-audit.jsonl"), "desk", &["note"], "request");
    let key_path = dir.join("key.json");
    fs::write(&key_path, serde_json::to_vec(&key_doc).unwrap()).unwrap();
    let output = run_raw(&["--request", key_path.to_str().unwrap()], None);
    assert_eq!(output.status.code(), Some(4));
    assert!(!dir.join("key-audit.jsonl").exists());
    assert!(!bytes_have(&output.stderr, b"PRIVATE KEY"));

    fs::write(&audit, b"keep\n").unwrap();
    let existing = dir.join("existing.json");
    let doc = base_value(&password_path, &ca_path, &audit, "desk", &["note"], "request");
    fs::write(&existing, serde_json::to_vec(&doc).unwrap()).unwrap();
    let output = run_raw(&["--request", existing.to_str().unwrap()], None);
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stderr, b"query-pg: audit\n");
    assert_eq!(fs::read(&audit).unwrap(), b"keep\n");

    let parent = dir.join("not-a-directory");
    fs::write(&parent, b"x").unwrap();
    let blocked = parent.join("audit.jsonl");
    let blocked_doc = base_value(&password_path, &ca_path, &blocked, "desk", &["note"], "request");
    let blocked_path = dir.join("blocked.json");
    fs::write(&blocked_path, serde_json::to_vec(&blocked_doc).unwrap()).unwrap();
    let output = run_raw(&["--request", blocked_path.to_str().unwrap()], None);
    assert_eq!(output.status.code(), Some(7));
    assert!(!bytes_have(&output.stderr, b"local-password-value"));

    let link = dir.join("password-link");
    std::os::unix::fs::symlink(&password_path, &link).unwrap();
    let link_doc = base_value(&link, &ca_path, &dir.join("link-audit.jsonl"), "desk", &["note"], "request");
    let link_path = dir.join("link.json");
    fs::write(&link_path, serde_json::to_vec(&link_doc).unwrap()).unwrap();
    let output = run_raw(&["--request", link_path.to_str().unwrap()], None);
    assert_eq!(output.status.code(), Some(3));
    assert!(!dir.join("link-audit.jsonl").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_query_pg_against_private_postgres() {
    if env::var("OO_A198_LIVE").ok().as_deref() != Some("1") {
        eprintln!("OO_A198_LIVE is unset; the private postgres comparison was not run");
        return;
    }
    let admin_pass = read_secret("admin.pass");
    let login_pass = read_secret("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca");
    let bans = vec![
        admin_pass.clone(),
        login_pass.clone(),
        ENV_PASSWORD.to_string(),
        SEALS[0].to_string(),
        SEALS[1].to_string(),
        SEALS[2].to_string(),
    ];
    let mut oracle = Oracle::new(&bans);
    reset_database(&admin_pass, &ca).await;
    let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    apply_frozen_schema(&mut admin).await;
    create_login(&admin, &login_pass).await;
    grant_mapping_read(&admin).await;
    let shadow = prepare_shadow(&mut admin).await;
    analyze_shadow(&admin, &shadow.table).await;
    let desk_policy = acl("desk", &["lane", "rank", "stamp", "note", "open"]);
    let peek_policy = acl("peek", &["note"]);
    let desk_installed = install_column_acl(&mut admin, &desk_policy)
        .await
        .expect("install desk");
    let peek_installed = install_column_acl(&mut admin, &peek_policy)
        .await
        .expect("install peek");
    assert_eq!(desk_installed.effect, InstallEffect::Installed);
    assert_eq!(peek_installed.effect, InstallEffect::Installed);
    assert_eq!(desk_installed.role, "oo_r_desk");
    assert_eq!(peek_installed.role, "oo_r_peek");
    let desk = compile_column_acl(&desk_policy).expect("compile desk");
    let peek = compile_column_acl(&peek_policy).expect("compile peek");
    let desk_identity = identity("oo_r_desk");
    let peek_identity = identity("oo_r_peek");
    let mut login = connect_db(DATABASE, LOGIN, &login_pass, &ca).await;
    let limits = limits();
    let desk_catalog = load_policy_catalog(&mut login, &desk_identity, &[desk], &limits)
        .await
        .expect("desk catalog");
    let peek_catalog = load_policy_catalog(&mut login, &peek_identity, &[peek], &limits)
        .await
        .expect("peek catalog");
    assert!(desk_catalog.relations[0].estimated_rows >= 1);
    assert!(!desk_catalog.relations[0]
        .columns
        .iter()
        .any(|column| column.name == "seal"));
    assert_eq!(
        peek_catalog.relations[0]
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["note"]
    );
    oracle.fact(
        "estimated_rows",
        &desk_catalog.relations[0].estimated_rows.to_string(),
    );
    oracle.fact(
        "desk_pending",
        &desk_catalog.relations[0].pending_unclassified.to_string(),
    );
    let before_rows = fetch_business(&admin, &shadow).await;
    let before_digest = row_digest(&before_rows);
    let before_tuples = tuples_settled(&admin, &shadow.table).await;
    assert_eq!(before_tuples.1, 0, "fixture updates");
    assert_eq!(before_tuples.2, 0, "fixture deletes");
    oracle.fact("row_digest_before", &before_digest);
    oracle.fact("tuples_before", &format!("{before_tuples:?}"));

    let work = scratch();
    let password_file = Path::new(SECRET_DIR).join("login.pass");
    let ca_file = Path::new(SECRET_DIR).join("ca.crt");

    let explicit_sql = "SELECT lane, rank, stamp, note, open FROM dispatch.ticket ORDER BY lane";
    let explicit_admin = fetch_typed(
        &admin,
        &select_sql(
            &shadow.table,
            &[
                shadow.lane.clone(),
                shadow.rank.clone(),
                shadow.stamp.clone(),
                shadow.note.clone(),
                shadow.open.clone(),
            ],
            &shadow.lane,
        ),
        &[Kind::I16, Kind::I32, Kind::I64, Kind::Text, Kind::Bool],
    )
    .await;
    let explicit = run_query(
        &work,
        &password_file,
        &ca_file,
        "desk",
        &["lane", "rank", "stamp", "note", "open"],
        "request",
        Some(explicit_sql),
        None,
        20,
        4096,
        &[],
        &bans,
    );
    assert_eq!(explicit.code, 0);
    assert!(explicit.stderr.is_empty());
    let explicit_turn = one_turn(&explicit.lines);
    assert_cells(&explicit_turn.rows, &explicit_admin);
    assert_summary_hashes(
        &explicit_turn.summary,
        explicit_sql,
        &desk_identity,
        &desk_catalog,
    );
    assert_eq!(explicit_turn.summary["truncated"], false);
    assert_eq!(explicit_turn.summary["status"], "succeeded");
    assert_eq!(explicit_turn.summary["rollback_confirmed"], true);
    assert_eq!(explicit_turn.summary["end_audit_ok"], true);
    assert_eq!(explicit_turn.summary["returned_rows"], 3);
    assert_audit(
        &explicit.audit,
        explicit_sql,
        &desk_identity,
        &desk_catalog,
        2,
        &bans,
    );
    oracle.fact("desk_explicit_rows", "3");
    oracle.fact(
        "desk_explicit_original",
        explicit_turn.summary["original_sha256"].as_str().unwrap(),
    );
    oracle.fact(
        "desk_explicit_executed",
        explicit_turn.summary["executed_sha256"].as_str().unwrap(),
    );
    save_clean("desk-explicit.stdout", &explicit.stdout, &bans);
    save_clean("desk-explicit.audit.jsonl", &fs::read(&explicit.audit).unwrap(), &bans);

    let star_names = desk_catalog.relations[0]
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect::<Vec<_>>();
    let star_physical: Vec<String> = desk_catalog.relations[0]
        .columns
        .iter()
        .map(|column| column.physical_name.clone())
        .collect();
    let star_kinds: Vec<Kind> = star_names.iter().copied().map(kind_of).collect();
    let star_sql = "SELECT * FROM dispatch.ticket ORDER BY lane";
    let star_admin = fetch_typed(
        &admin,
        &select_sql(&shadow.table, &star_physical, &shadow.lane),
        &star_kinds,
    )
    .await;
    let star = run_query(
        &work,
        &password_file,
        &ca_file,
        "desk",
        &["lane", "rank", "stamp", "note", "open"],
        "request",
        Some(star_sql),
        None,
        20,
        4096,
        &[("PGPASSWORD", ENV_PASSWORD)],
        &bans,
    );
    assert_eq!(star.code, 0, "env password must not replace the password file");
    assert!(star.stderr.is_empty());
    let star_turn = one_turn(&star.lines);
    assert_cells(&star_turn.rows, &star_admin);
    assert_summary_hashes(&star_turn.summary, star_sql, &desk_identity, &desk_catalog);
    assert!(!star.stdout.windows(ENV_PASSWORD.len()).any(|window| window == ENV_PASSWORD.as_bytes()));
    oracle.fact("star_columns", &star_names.join(","));
    oracle.fact("star_rows", &star_turn.rows.len().to_string());

    let count_sql = "SELECT count(*) FROM dispatch.ticket";
    let count_admin = fetch_typed(
        &admin,
        &format!(
            "SELECT count(*)::int8 FROM \"_oo\".{}",
            quote_physical(&shadow.table, "pt_")
        ),
        &[Kind::I64],
    )
    .await;
    let count = run_query(
        &work,
        &password_file,
        &ca_file,
        "desk",
        &["lane", "rank", "stamp", "note", "open"],
        "request",
        Some(count_sql),
        None,
        20,
        4096,
        &[],
        &bans,
    );
    assert_eq!(count.code, 0);
    let count_turn = one_turn(&count.lines);
    assert_cells(&count_turn.rows, &count_admin);
    assert_eq!(count_turn.rows[0][0]["value"], "3");
    assert!(count_turn.rows[0][0]["value"].is_string());
    assert_summary_hashes(&count_turn.summary, count_sql, &desk_identity, &desk_catalog);
    oracle.fact("count_star", "3");

    let peek_sql = "SELECT note FROM dispatch.ticket ORDER BY note";
    let peek_admin = fetch_typed(
        &admin,
        &select_sql(&shadow.table, std::slice::from_ref(&shadow.note), &shadow.note),
        &[Kind::Text],
    )
    .await;
    let peek = run_query(
        &work,
        &password_file,
        &ca_file,
        "peek",
        &["note"],
        "request",
        Some(peek_sql),
        None,
        20,
        4096,
        &[],
        &bans,
    );
    assert_eq!(peek.code, 0);
    let peek_turn = one_turn(&peek.lines);
    assert_cells(&peek_turn.rows, &peek_admin);
    assert_summary_hashes(&peek_turn.summary, peek_sql, &peek_identity, &peek_catalog);
    assert!(!peek.stdout.windows(b"-9223372036854775808".len()).any(|w| w == b"-9223372036854775808"));
    oracle.fact("peek_rows", &peek_turn.rows.len().to_string());

    let peek_star_sql = "SELECT * FROM dispatch.ticket ORDER BY note";
    let peek_star_admin = fetch_typed(
        &admin,
        &select_sql(
            &shadow.table,
            std::slice::from_ref(&peek_catalog.relations[0].columns[0].physical_name),
            &shadow.note,
        ),
        &[Kind::Text],
    )
    .await;
    let peek_star = run_query(
        &work,
        &password_file,
        &ca_file,
        "peek",
        &["note"],
        "request",
        Some(peek_star_sql),
        None,
        20,
        4096,
        &[],
        &bans,
    );
    assert_eq!(peek_star.code, 0);
    assert_cells(&one_turn(&peek_star.lines).rows, &peek_star_admin);

    let first_bytes = row_canonical_bytes(&explicit_turn.rows[0]);
    let truncated = run_query(
        &work,
        &password_file,
        &ca_file,
        "desk",
        &["lane", "rank", "stamp", "note", "open"],
        "request",
        Some(explicit_sql),
        None,
        20,
        first_bytes,
        &[],
        &bans,
    );
    assert_eq!(truncated.code, 0);
    let truncated_turn = one_turn(&truncated.lines);
    assert_eq!(truncated_turn.rows.len(), 1);
    assert_eq!(truncated_turn.summary["truncated"], true);
    assert_eq!(truncated_turn.summary["status"], "succeeded");
    assert_eq!(truncated_turn.summary["returned_rows"], 1);
    assert_eq!(truncated_turn.summary["end_audit_ok"], true);
    assert_cells(&truncated_turn.rows, &explicit_admin[..1]);
    oracle.fact("truncate_bytes", &first_bytes.to_string());
    oracle.fact("truncate_rows", "1");

    let turns = format!(
        "{}\n{}\n",
        json!({"version":"oo.gateway.query-pg-turn/v1","sql": count_sql}),
        json!({"version":"oo.gateway.query-pg-turn/v1","sql": peek_sql})
    );
    let stdin_run = run_query(
        &work,
        &password_file,
        &ca_file,
        "peek",
        &["note"],
        "stdin",
        None,
        Some(turns.as_bytes()),
        20,
        4096,
        &[],
        &bans,
    );
    assert_eq!(stdin_run.code, 0);
    assert!(stdin_run.stderr.is_empty());
    let grouped = group_turns(&stdin_run.lines);
    assert_eq!(grouped.len(), 2);
    assert_cells(&grouped[0].rows, &count_admin);
    assert_cells(&grouped[1].rows, &peek_admin);
    assert_summary_hashes(&grouped[0].summary, count_sql, &peek_identity, &peek_catalog);
    assert_summary_hashes(&grouped[1].summary, peek_sql, &peek_identity, &peek_catalog);
    let audit_lines = audit_values(&stdin_run.audit);
    assert_eq!(audit_lines.len(), 4);
    assert_eq!(audit_lines[0]["record"], "admit");
    assert_eq!(audit_lines[1]["record"], "complete");
    assert_eq!(audit_lines[2]["record"], "admit");
    assert_eq!(audit_lines[3]["record"], "complete");
    assert_eq!(audit_lines[0]["backend_pid"], audit_lines[2]["backend_pid"]);
    assert_eq!(audit_lines[0]["role"], "oo_r_peek");
    assert_quiet(&fs::read(&stdin_run.audit).unwrap(), &bans, "stdin audit");
    oracle.fact("stdin_audit_lines", "4");
    save_clean("stdin.audit.jsonl", &fs::read(&stdin_run.audit).unwrap(), &bans);

    for (name, role, columns, sql) in [
        (
            "hidden",
            "peek",
            &["note"][..],
            "SELECT lane FROM dispatch.ticket",
        ),
        (
            "write",
            "desk",
            &["lane", "rank", "stamp", "note", "open"][..],
            "UPDATE dispatch.ticket SET note = note",
        ),
        (
            "sleep",
            "desk",
            &["lane", "rank", "stamp", "note", "open"][..],
            "SELECT pg_sleep(1)",
        ),
    ] {
        let run = run_query(
            &work,
            &password_file,
            &ca_file,
            role,
            columns,
            "request",
            Some(sql),
            None,
            20,
            4096,
            &[],
            &bans,
        );
        assert_eq!(run.code, 6, "{name}");
        assert_eq!(run.stderr, b"query-pg: guard\n");
        assert!(!run.audit.exists(), "{name} audit");
        assert!(run.stdout.is_empty(), "{name} stdout");
        oracle.fact(&format!("{name}_exit"), "6");
        oracle.fact(&format!("{name}_audit"), "absent");
    }

    let sessions_before = sessions(&admin).await;
    let existing = work.join("existing-audit.jsonl");
    fs::write(&existing, b"keep\n").unwrap();
    let existing_run = run_query_at(
        &work,
        &password_file,
        &ca_file,
        "desk",
        &["note"],
        "request",
        Some("SELECT note FROM dispatch.ticket"),
        None,
        20,
        4096,
        &[],
        &bans,
        &existing,
    );
    assert_eq!(existing_run.code, 7);
    assert_eq!(fs::read(&existing).unwrap(), b"keep\n");
    assert_eq!(sessions(&admin).await, sessions_before);
    oracle.fact("existing_audit_sessions", "unchanged");

    let parent = work.join("parent-file");
    fs::write(&parent, b"x").unwrap();
    let nested = parent.join("audit.jsonl");
    let nested_run = run_query_at(
        &work,
        &password_file,
        &ca_file,
        "desk",
        &["note"],
        "request",
        Some("SELECT note FROM dispatch.ticket"),
        None,
        20,
        4096,
        &[],
        &bans,
        &nested,
    );
    assert_eq!(nested_run.code, 7);
    assert_eq!(sessions(&admin).await, sessions_before);
    oracle.fact("unwritable_parent_sessions", "unchanged");

    let garbage_ca = work.join("garbage.pem");
    fs::write(&garbage_ca, b"not-a-certificate\n").unwrap();
    let garbage_audit = work.join("garbage-audit.jsonl");
    let garbage = run_query_at(
        &work,
        &password_file,
        &garbage_ca,
        "desk",
        &["note"],
        "request",
        Some("SELECT note FROM dispatch.ticket"),
        None,
        20,
        4096,
        &[],
        &bans,
        &garbage_audit,
    );
    assert_eq!(garbage.code, 4);
    assert!(!garbage_audit.exists());
    assert_eq!(sessions(&admin).await, sessions_before);
    oracle.fact("garbage_ca_sessions", "unchanged");

    let other = Path::new(SECRET_DIR).join("other.crt");
    let other_audit = work.join("other-audit.jsonl");
    let other_run = run_query_at(
        &work,
        &password_file,
        &other,
        "desk",
        &["note"],
        "request",
        Some("SELECT note FROM dispatch.ticket"),
        None,
        20,
        4096,
        &[],
        &bans,
        &other_audit,
    );
    assert_eq!(other_run.code, 4);
    assert!(!other_audit.exists());
    oracle.fact("invalid_ca_exit", "4");
    oracle.fact("invalid_ca_audit", "absent");

    let after_rows = fetch_business(&admin, &shadow).await;
    if before_rows != after_rows {
        panic!("user rows changed");
    }
    let after_digest = row_digest(&after_rows);
    let after_tuples = tuples(&admin, &shadow.table).await;
    assert_eq!(before_digest, after_digest);
    assert_eq!(before_tuples, after_tuples);
    oracle.fact("row_digest_after", &after_digest);
    oracle.fact("tuples_after", &format!("{after_tuples:?}"));
    oracle.fact("done", "ok");
}

struct Run {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    lines: Vec<Value>,
    audit: PathBuf,
}

#[allow(clippy::too_many_arguments)]
fn run_query(
    dir: &Path,
    password_file: &Path,
    ca_file: &Path,
    role: &str,
    columns: &[&str],
    source: &str,
    sql: Option<&str>,
    stdin: Option<&[u8]>,
    max_rows: u64,
    max_bytes: u64,
    env_extra: &[(&str, &str)],
    bans: &[String],
) -> Run {
    let audit = dir.join(format!("audit-{}-{}.jsonl", role, SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)));
    run_query_at(
        dir,
        password_file,
        ca_file,
        role,
        columns,
        source,
        sql,
        stdin,
        max_rows,
        max_bytes,
        env_extra,
        bans,
        &audit,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_query_at(
    dir: &Path,
    password_file: &Path,
    ca_file: &Path,
    role: &str,
    columns: &[&str],
    source: &str,
    sql: Option<&str>,
    stdin: Option<&[u8]>,
    max_rows: u64,
    max_bytes: u64,
    env_extra: &[(&str, &str)],
    bans: &[String],
    audit: &Path,
) -> Run {
    let request = dir.join(format!("request-{}.json", SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)));
    let mut doc = base_value(password_file, ca_file, audit, role, columns, source);
    doc["endpoint"]["host"] = Value::String(HOST.to_string());
    doc["endpoint"]["port"] = json!(5432);
    doc["statement"]["max_rows"] = json!(max_rows);
    doc["statement"]["max_result_bytes"] = json!(max_bytes);
    if let Some(sql) = sql {
        doc["statement"]["sql"] = Value::String(sql.to_string());
    } else {
        doc["statement"].as_object_mut().unwrap().remove("sql");
    }
    let bytes = serde_json::to_vec(&doc).unwrap();
    assert_quiet(&bytes, bans, "request");
    fs::write(&request, &bytes).unwrap();
    let output = run_raw_env(
        &["--request", request.to_str().unwrap()],
        stdin,
        env_extra,
    );
    assert_quiet(&output.stdout, bans, "stdout");
    assert_quiet(&output.stderr, bans, "stderr");
    if output.status.code() == Some(0) {
        assert!(output.stderr.is_empty());
    }
    let lines = if output.stdout.is_empty() {
        Vec::new()
    } else {
        json_lines(&output.stdout)
    };
    Run {
        code: output.status.code().expect("exit"),
        stdout: output.stdout,
        stderr: output.stderr,
        lines,
        audit: audit.to_path_buf(),
    }
}

struct Turn {
    rows: Vec<Vec<Value>>,
    summary: Value,
}

fn one_turn(lines: &[Value]) -> Turn {
    let grouped = group_turns(lines);
    assert_eq!(grouped.len(), 1);
    grouped.into_iter().next().unwrap()
}

fn group_turns(lines: &[Value]) -> Vec<Turn> {
    let mut turns = Vec::new();
    let mut rows = Vec::new();
    for line in lines {
        match line["record"].as_str() {
            Some("row") => rows.push(line["cells"].as_array().expect("cells").clone()),
            Some("summary") => {
                turns.push(Turn {
                    rows: std::mem::take(&mut rows),
                    summary: line.clone(),
                });
            }
            other => panic!("unexpected record {other:?}"),
        }
    }
    assert!(rows.is_empty(), "rows without a summary");
    turns
}

fn assert_cells(got: &[Vec<Value>], expect: &[Vec<CellKind>]) {
    assert_eq!(got.len(), expect.len(), "row count");
    for (got_row, expect_row) in got.iter().zip(expect) {
        assert_eq!(got_row.len(), expect_row.len(), "cell count");
        for (cell, kind) in got_row.iter().zip(expect_row) {
            match kind {
                CellKind::Null => {
                    assert_eq!(cell["kind"], "null");
                    assert!(cell.get("value").is_none());
                }
                CellKind::Bool(value) => {
                    assert_eq!(cell["kind"], "bool");
                    assert_eq!(cell["value"], Value::Bool(*value));
                }
                CellKind::Int(value) => {
                    assert_eq!(cell["kind"], "int");
                    assert_eq!(cell["value"], Value::String(value.clone()));
                }
                CellKind::Text(value) => {
                    assert_eq!(cell["kind"], "text");
                    assert_eq!(cell["value"], Value::String(value.clone()));
                }
            }
        }
    }
}

enum CellKind {
    Null,
    Bool(bool),
    Int(String),
    Text(String),
}

fn assert_summary_hashes(
    summary: &Value,
    sql: &str,
    identity: &PolicyIdentity,
    catalog: &PolicyCatalog,
) {
    let plan = guard_select(identity, catalog, sql).unwrap_or_else(|err| {
        let text = err.to_string();
        assert!(!text.contains("pt_"), "guard display named a physical table");
        panic!("guard failed");
    });
    assert_eq!(
        summary["original_sha256"].as_str().unwrap(),
        sha256_hex(sql.as_bytes())
    );
    assert_eq!(
        summary["executed_sha256"].as_str().unwrap(),
        plan.executed_sha256
    );
    assert_eq!(summary["original_sha256"].as_str().unwrap().len(), 64);
}

fn assert_audit(
    path: &Path,
    sql: &str,
    identity: &PolicyIdentity,
    catalog: &PolicyCatalog,
    lines: usize,
    bans: &[String],
) {
    let raw = fs::read(path).expect("audit");
    assert_quiet(&raw, bans, "audit");
    let values = audit_values(path);
    assert_eq!(values.len(), lines);
    assert_eq!(values[0]["record"], "admit");
    assert_eq!(values[1]["record"], "complete");
    assert_eq!(values[0]["protocol"], "oo.gateway.query-pg-audit/v1");
    let plan = guard_select(identity, catalog, sql).expect("audit guard");
    assert_eq!(
        values[0]["original_sha256"].as_str().unwrap(),
        sha256_hex(sql.as_bytes())
    );
    assert_eq!(
        values[0]["executed_sha256"].as_str().unwrap(),
        plan.executed_sha256
    );
    assert_eq!(values[1]["original_sha256"], values[0]["original_sha256"]);
    assert_eq!(values[1]["executed_sha256"], values[0]["executed_sha256"]);
    assert_eq!(
        values[0]["fingerprint_hex"].as_str().unwrap(),
        plan.fingerprint_hex
    );
    assert_eq!(values[0]["role"], identity.role);
    assert_eq!(values[0]["project"], identity.project);
    assert_eq!(values[0]["policy_id"], identity.policy_id);
    assert!(values[0]["backend_pid"].as_i64().unwrap() > 0);
    let start = values[0]["backend_start"].as_str().unwrap();
    assert!(!start.is_empty() && start.bytes().all(|byte| byte.is_ascii_digit()));
    assert_eq!(values[1]["backend_pid"], values[0]["backend_pid"]);
    assert_eq!(values[1]["status"], "succeeded");
    assert!(!raw.windows(5).any(|window| window == b"SELECT"));
}

fn audit_values(path: &Path) -> Vec<Value> {
    json_lines(&fs::read(path).expect("audit bytes"))
}

fn json_lines(bytes: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(bytes).expect("utf8");
    text.split('\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("json line"))
        .collect()
}

fn row_canonical_bytes(row: &[Value]) -> u64 {
    row.iter()
        .map(|cell| match cell["kind"].as_str().unwrap() {
            "null" => 0,
            "bool" if cell["value"] == true => 4,
            "bool" => 5,
            "int" | "text" => u64::try_from(cell["value"].as_str().unwrap().len()).unwrap(),
            other => panic!("kind {other}"),
        })
        .sum()
}

fn assert_quiet(bytes: &[u8], bans: &[String], label: &str) {
    let text = String::from_utf8_lossy(bytes);
    for ban in bans {
        assert!(!text.contains(ban), "{label} contained a banned value");
    }
    assert!(!text.contains("pt_"), "{label} physical table");
    assert!(!text.contains("pc_"), "{label} physical column");
}

fn save_clean(name: &str, bytes: &[u8], bans: &[String]) {
    assert_quiet(bytes, bans, name);
    let path = Path::new("/evidence/live").join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
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
        assert!(!key.contains("pass"));
        assert_quiet(value.as_bytes(), &self.bans, key);
        self.lines.push(format!("{key}={value}"));
        let path = Path::new(ORACLE_PATH);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("oracle dir");
        }
        fs::write(path, self.lines.join("\n") + "\n").expect("oracle");
    }
}

fn base_value(
    password_file: &Path,
    ca_file: &Path,
    audit: &Path,
    role: &str,
    columns: &[&str],
    source: &str,
) -> Value {
    json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": {
            "host": "127.0.0.1",
            "port": 1,
            "database": DATABASE,
            "user": LOGIN
        },
        "password_file": password_file.display().to_string(),
        "ca_file": ca_file.display().to_string(),
        "audit_file": audit.display().to_string(),
        "identity": {
            "project": SOURCE,
            "role": format!("oo_r_{role}"),
            "policy_id": CONFIG
        },
        "policies": [policy_value(role, columns)],
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1_000_000,
            "unknown_rows": "refuse"
        },
        "statement": {
            "source": source,
            "sql": "SELECT note FROM dispatch.ticket",
            "max_rows": 20,
            "max_result_bytes": 4096,
            "wall_clock_ms": 30000
        }
    })
}

fn policy_value(role: &str, columns: &[&str]) -> Value {
    json!({
        "project": SOURCE,
        "config_digest": CONFIG,
        "generation": GENERATION,
        "role": role,
        "login": LOGIN,
        "table": {
            "namespace": NAMESPACE,
            "name": TABLE,
            "columns": columns.iter().copied().map(|name| json!({
                "name": name,
                "status": "confirmed",
                "semantic_type": "value",
                "pii": {"class": "none"}
            })).collect::<Vec<_>>()
        }
    })
}

fn bytes_have(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn query_pg_exe() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_query-pg") {
        return PathBuf::from(path);
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_query_pg") {
        return PathBuf::from(path);
    }
    // `cargo check --tests` does not define the binary path. The test binary
    // lives in target/debug/deps, next to target/debug/query-pg.
    let mut path = env::current_exe().expect("current_exe");
    path.pop();
    path.pop();
    path.push("query-pg");
    path
}

fn run_raw(args: &[&str], stdin: Option<&[u8]>) -> Output {
    run_raw_env(args, stdin, &[])
}

fn run_raw_env(args: &[&str], stdin: Option<&[u8]>, env_extra: &[(&str, &str)]) -> Output {
    let mut command = Command::new(query_pg_exe());
    command.args(args);
    command.env_remove("PGPASSWORD");
    for (key, value) in env_extra {
        command.env(key, value);
    }
    if let Some(bytes) = stdin {
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(bytes)
            .expect("stdin");
        child.wait_with_output().expect("wait")
    } else {
        command.output().expect("run")
    }
}

fn scratch() -> PathBuf {
    let path = env::temp_dir().join(format!(
        "oo-a198-query-pg-{}-{}",
        std::process::id(),
        SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[derive(Clone, Copy)]
enum Kind {
    I16,
    I32,
    I64,
    Text,
    Bool,
}

fn kind_of(name: &str) -> Kind {
    match name {
        "lane" => Kind::I16,
        "rank" => Kind::I32,
        "stamp" => Kind::I64,
        "note" | "seal" => Kind::Text,
        "open" => Kind::Bool,
        _ => panic!("unknown logical column"),
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Business {
    lane: i16,
    rank: i32,
    stamp: i64,
    note: Option<String>,
    open: bool,
    seal: String,
}

struct Shadow {
    table: String,
    lane: String,
    rank: String,
    stamp: String,
    note: String,
    open: String,
    seal: String,
}

fn limits() -> CatalogLimits {
    CatalogLimits {
        max_tables: 4,
        max_columns: 64,
        max_functions: 1,
        max_operators: 64,
        large_table_row_threshold: 1_000_000,
        unknown_rows: UnknownRows::Refuse,
    }
}

fn identity(role: &str) -> PolicyIdentity {
    PolicyIdentity {
        project: SOURCE.to_string(),
        role: role.to_string(),
        policy_id: CONFIG.to_string(),
    }
}

fn acl(role: &str, columns: &[&str]) -> ColumnAclPolicy {
    ColumnAclPolicy {
        project: SOURCE.to_string(),
        config_digest: CONFIG.to_string(),
        generation: GENERATION,
        role: role.to_string(),
        login: LOGIN.to_string(),
        table: LogicalTable {
            namespace: NAMESPACE.to_string(),
            name: TABLE.to_string(),
            columns: columns.iter().copied().map(column_spec).collect(),
        },
        row_filter: None,
    }
}

fn column_spec(name: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        status: ClassStatus::Confirmed,
        semantic_type: "value".to_string(),
        pii: PiiClass::None,
    }
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

fn row_digest(rows: &[Business]) -> String {
    let mut raw = Vec::new();
    for row in rows {
        raw.extend_from_slice(&row.lane.to_be_bytes());
        raw.extend_from_slice(&row.rank.to_be_bytes());
        raw.extend_from_slice(&row.stamp.to_be_bytes());
        match &row.note {
            Some(note) => {
                raw.push(1);
                raw.extend_from_slice(&(note.len() as u32).to_be_bytes());
                raw.extend_from_slice(note.as_bytes());
            }
            None => raw.push(0),
        }
        raw.push(u8::from(row.open));
        raw.extend_from_slice(&(row.seal.len() as u32).to_be_bytes());
        raw.extend_from_slice(row.seal.as_bytes());
    }
    sha256_hex(&raw)
}

fn read_secret(name: &str) -> String {
    let raw = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret");
    let value = raw.trim();
    assert!(
        (16..=128).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{name} is not lowercase hex"
    );
    value.to_string()
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
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    assert!(!last.contains(password), "connect error contained a password");
    panic!("tls connect failed");
}

async fn reset_database(admin_pass: &str, ca: &[u8]) {
    let maintenance = connect_db("postgres", ADMIN, admin_pass, ca).await;
    exec_hidden(
        &maintenance,
        "terminate",
        &format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid()"
        ),
    )
    .await;
    exec_hidden(
        &maintenance,
        "drop-database",
        &format!("DROP DATABASE IF EXISTS \"{DATABASE}\""),
    )
    .await;
    exec_hidden(
        &maintenance,
        "drop-roles",
        "DO $$\n\
         DECLARE role_name text;\n\
         BEGIN\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a198_login') THEN\n\
             DROP ROLE oo_a198_login;\n\
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
    exec_hidden(
        &maintenance,
        "create-database",
        &format!("CREATE DATABASE \"{DATABASE}\" TEMPLATE template0 ENCODING 'UTF8'"),
    )
    .await;
}

async fn exec_hidden(client: &Client, name: &str, sql: &str) {
    client
        .batch_execute(sql)
        .await
        .unwrap_or_else(|err| fail_db(name, &err));
}

async fn exec_tx(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql)
        .await
        .unwrap_or_else(|err| fail_db(name, &err));
}

fn fail_db(name: &str, err: &tokio_postgres::Error) -> ! {
    let state = err.code().map(|code| code.code().to_string()).unwrap_or_default();
    panic!("{name} sqlstate={state}");
}

async fn apply_frozen_schema(client: &mut Client) {
    let bootstrap = migration("0000_bootstrap_roles.sql");
    let fence = migration("0001_target_fence_expand.sql");
    let preflight = migration("0003_target_segment_preflight_expand.sql");
    let mapping = migration("0004_physical_mapping.sql");
    let full_import = migration("0005_full_import_expand.sql");
    let tx = begin_locked(client).await;
    exec_tx(&tx, "0000_bootstrap_roles.sql", &bootstrap.0).await;
    exec_tx(&tx, "set-owner", "SET LOCAL ROLE oo_target_owner").await;
    exec_tx(&tx, "0001_target_fence_expand.sql", &fence.0).await;
    exec_tx(
        &tx,
        "row-lock",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_tx(&tx, "0003_target_segment_preflight_expand.sql", &preflight.0).await;
    exec_tx(&tx, "0004_physical_mapping.sql", &mapping.0).await;
    exec_tx(
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
    exec_tx(
        &tx,
        "row-lock-import",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_tx(
        &tx,
        "stats-grant",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
    )
    .await;
    exec_tx(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    exec_tx(&tx, "0005_full_import_expand.sql", &full_import.0).await;
    exec_tx(
        &tx,
        "marker-import",
        &marker_sql(&[("full-import/v1", &full_import.1, "0005_full_import_expand.sql")]),
    )
    .await;
    tx.commit().await.expect("commit full import");
}

fn migration(name: &str) -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oo-pipeline/migrations")
        .join(name);
    let bytes = fs::read(&path).unwrap_or_else(|_| panic!("read {name}"));
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{name} utf8"));
    (text, sha256_hex(&bytes))
}

fn marker_sql(rows: &[(&str, &str, &str)]) -> String {
    let mut values = String::new();
    for (index, (version, checksum, filename)) in rows.iter().enumerate() {
        assert_eq!(checksum.len(), 64);
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!(
            "  ('{version}', '{checksum}', 'expand', '{filename}')"
        ));
    }
    format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}"
    )
}

async fn begin_locked(client: &mut Client) -> tokio_postgres::Transaction<'_> {
    let tx = client.transaction().await.expect("begin");
    exec_tx(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    exec_tx(&tx, "statement-timeout", "SET LOCAL statement_timeout = '60s'").await;
    exec_tx(
        &tx,
        "advisory-lock",
        &format!("SELECT pg_catalog.pg_advisory_xact_lock({ADVISORY_LOCK})"),
    )
    .await;
    tx
}

async fn create_login(client: &Client, password: &str) {
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    exec_hidden(
        client,
        "create-login",
        &format!(
            "CREATE ROLE \"{LOGIN}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{LOGIN}\""
        ),
    )
    .await;
}

async fn grant_mapping_read(client: &Client) {
    exec_hidden(
        client,
        "grant-mapping",
        &format!(
            "GRANT USAGE ON SCHEMA _oo TO \"{LOGIN}\";\n\
             GRANT SELECT ON TABLE _oo.physical_mappings TO \"{LOGIN}\";\n\
             GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO \"{LOGIN}\""
        ),
    )
    .await;
}

async fn prepare_shadow(client: &mut Client) -> Shadow {
    let target: i64 = client
        .query_one(
            "SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)",
            &[&SHARD, &OWNER],
        )
        .await
        .unwrap_or_else(|err| fail_db("adopt", &err))
        .get(0);
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)",
            &[&SHARD, &OWNER, &target],
        )
        .await
        .unwrap_or_else(|err| fail_db("bind", &err));
    let columns = ["lane", "rank", "stamp", "note", "open", "seal"];
    register(client, "namespace", "n", NAMESPACE, "", "").await;
    register(client, "table", "t", NAMESPACE, TABLE, "").await;
    for column in columns {
        register(client, "column", "c", NAMESPACE, TABLE, column).await;
    }
    let table = lookup(client, "table", NAMESPACE, TABLE, "").await;
    let lane = lookup(client, "column", NAMESPACE, TABLE, "lane").await;
    let rank = lookup(client, "column", NAMESPACE, TABLE, "rank").await;
    let stamp = lookup(client, "column", NAMESPACE, TABLE, "stamp").await;
    let note = lookup(client, "column", NAMESPACE, TABLE, "note").await;
    let open = lookup(client, "column", NAMESPACE, TABLE, "open").await;
    let seal = lookup(client, "column", NAMESPACE, TABLE, "seal").await;
    let oids = vec![
        type_oid(client, "int2").await,
        type_oid(client, "int4").await,
        type_oid(client, "int8").await,
        type_oid(client, "text").await,
        type_oid(client, "bool").await,
        type_oid(client, "text").await,
    ];
    assert!(oids.iter().all(|oid| *oid > 0));
    assert_ne!(oids[0], oids[1]);
    assert_ne!(oids[1], oids[2]);
    let schemas = vec![NAMESPACE.to_string()];
    let tables = vec![TABLE.to_string()];
    let physical = vec![table.clone()];
    let open_params: [&(dyn ToSql + Sync); 12] = [
        &SHARD,
        &OWNER,
        &target,
        &SOURCE,
        &GENERATION,
        &CONFIG,
        &TYPE_DIGEST,
        &INPUT_DIGEST,
        &SCHEMA_DIGEST,
        &schemas,
        &tables,
        &physical,
    ];
    let run_id: String = client
        .query_one(
            "SELECT _oo.full_import_open_run($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &open_params,
        )
        .await
        .unwrap_or_else(|err| fail_db("open-run", &err))
        .get(0);
    let idents: Vec<String> = columns.into_iter().map(str::to_string).collect();
    let typmods = vec![-1_i32; 6];
    let required = vec![true, true, true, false, true, true];
    let index = 0_i32;
    let prepare_params: [&(dyn ToSql + Sync); 14] = [
        &SHARD,
        &OWNER,
        &target,
        &run_id,
        &index,
        &SOURCE,
        &CONFIG,
        &GENERATION,
        &NAMESPACE,
        &TABLE,
        &idents,
        &oids,
        &typmods,
        &required,
    ];
    let prepared: String = client
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, ($2::text)::uuid, $3, ($4::text)::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &prepare_params,
        )
        .await
        .unwrap_or_else(|err| fail_db("prepare", &err))
        .get(0);
    assert_eq!(prepared, table);
    insert_rows(
        client,
        &Shadow {
            table: table.clone(),
            lane: lane.clone(),
            rank: rank.clone(),
            stamp: stamp.clone(),
            note: note.clone(),
            open: open.clone(),
            seal: seal.clone(),
        },
    )
    .await;
    Shadow {
        table,
        lane,
        rank,
        stamp,
        note,
        open,
        seal,
    }
}

async fn type_oid(client: &Client, name: &str) -> i64 {
    let oid: i32 = client
        .query_one(
            "SELECT type_row.oid::int4 \
             FROM pg_catalog.pg_type AS type_row \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace \
             WHERE namespace.nspname = 'pg_catalog' AND type_row.typname = $1 AND type_row.typtype = 'b'",
            &[&name],
        )
        .await
        .unwrap_or_else(|err| fail_db("type-oid", &err))
        .get(0);
    i64::from(oid)
}

async fn register(
    client: &Client,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let payload: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &GENERATION,
        &letter,
        &namespace,
        &table,
        &column,
    ];
    let digest: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .unwrap_or_else(|err| fail_db("digest", &err))
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [
        &SOURCE,
        &CONFIG,
        &GENERATION,
        &level,
        &namespace,
        &table,
        &column,
        &digest,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .unwrap_or_else(|err| fail_db("register", &err));
}

async fn lookup(
    client: &Client,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &GENERATION,
        &level,
        &namespace,
        &table,
        &column,
    ];
    client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &params,
        )
        .await
        .unwrap_or_else(|err| fail_db("lookup", &err))
        .get(0)
}

async fn insert_rows(client: &mut Client, shadow: &Shadow) {
    let sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {lane}, {rank}, {stamp}, {note}, {open}, {seal}) \
         VALUES (($1::text)::numeric, $2, $3, $4, $5, $6, $7)",
        table = quote_physical(&shadow.table, "pt_"),
        lane = quote_physical(&shadow.lane, "pc_"),
        rank = quote_physical(&shadow.rank, "pc_"),
        stamp = quote_physical(&shadow.stamp, "pc_"),
        note = quote_physical(&shadow.note, "pc_"),
        open = quote_physical(&shadow.open, "pc_"),
        seal = quote_physical(&shadow.seal, "pc_"),
    );
    let rows = [
        (-2_i16, -20_i32, i64::MIN, Some(NOTE), true, SEALS[0]),
        (1, 10, i64::MAX, None, false, SEALS[1]),
        (7, 0, 0, Some("beta"), true, SEALS[2]),
    ];
    let tx = client.transaction().await.expect("insert begin");
    for (lane, rank, stamp, note, open, seal) in rows {
        let pos = lane_pos(lane);
        tx.execute(&sql, &[&pos, &lane, &rank, &stamp, &note, &open, &seal])
            .await
            .unwrap_or_else(|err| fail_db("insert", &err));
    }
    tx.commit().await.expect("insert commit");
}

fn lane_pos(lane: i16) -> String {
    match lane {
        -2 => "1".to_string(),
        1 => "2".to_string(),
        7 => "3".to_string(),
        _ => panic!("position"),
    }
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical length");
    assert_eq!(rest.len(), 60, "physical body");
    assert!(
        rest.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "physical hex"
    );
    format!("\"{name}\"")
}

async fn analyze_shadow(client: &Client, table: &str) {
    exec_hidden(
        client,
        "analyze",
        &format!("ANALYZE \"_oo\".{}", quote_physical(table, "pt_")),
    )
    .await;
}

fn select_sql(table: &str, columns: &[String], order: &str) -> String {
    let list = columns
        .iter()
        .map(|column| quote_physical(column, "pc_"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {list} FROM \"_oo\".{table} ORDER BY {order}",
        table = quote_physical(table, "pt_"),
        order = quote_physical(order, "pc_")
    )
}

async fn fetch_typed(client: &Client, sql: &str, kinds: &[Kind]) -> Vec<Vec<CellKind>> {
    let rows = client
        .query(sql, &[])
        .await
        .unwrap_or_else(|err| fail_db("admin-read", &err));
    rows.iter().map(|row| decode(row, kinds)).collect()
}

fn decode(row: &tokio_postgres::Row, kinds: &[Kind]) -> Vec<CellKind> {
    kinds
        .iter()
        .enumerate()
        .map(|(index, kind)| match kind {
            Kind::I16 => CellKind::Int(row.get::<_, i16>(index).to_string()),
            Kind::I32 => CellKind::Int(row.get::<_, i32>(index).to_string()),
            Kind::I64 => CellKind::Int(row.get::<_, i64>(index).to_string()),
            Kind::Text => match row.get::<_, Option<String>>(index) {
                Some(text) => CellKind::Text(text),
                None => CellKind::Null,
            },
            Kind::Bool => CellKind::Bool(row.get(index)),
        })
        .collect()
}

async fn fetch_business(client: &Client, shadow: &Shadow) -> Vec<Business> {
    let sql = select_sql(
        &shadow.table,
        &[
            shadow.lane.clone(),
            shadow.rank.clone(),
            shadow.stamp.clone(),
            shadow.note.clone(),
            shadow.open.clone(),
            shadow.seal.clone(),
        ],
        &shadow.lane,
    );
    let rows = client
        .query(&sql, &[])
        .await
        .unwrap_or_else(|err| fail_db("business", &err));
    rows.iter()
        .map(|row| Business {
            lane: row.get(0),
            rank: row.get(1),
            stamp: row.get(2),
            note: row.get(3),
            open: row.get(4),
            seal: row.get(5),
        })
        .collect()
}

async fn tuples_settled(client: &Client, table: &str) -> (i64, i64, i64) {
    let mut last = (0, 0, 0);
    for _ in 0..50 {
        last = tuples(client, table).await;
        if last.0 >= 3 {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    last
}

async fn tuples(client: &Client, table: &str) -> (i64, i64, i64) {
    client
        .execute("SELECT pg_stat_clear_snapshot()", &[])
        .await
        .unwrap_or_else(|err| fail_db("tuples", &err));
    let row = client
        .query_one(
            "SELECT n_tup_ins, n_tup_upd, n_tup_del \
             FROM pg_stat_user_tables \
             WHERE schemaname = '_oo' AND relname = $1",
            &[&table],
        )
        .await
        .unwrap_or_else(|err| fail_db("tuples", &err));
    (row.get(0), row.get(1), row.get(2))
}

async fn sessions(client: &Client) -> i64 {
    client
        .query_one(
            "SELECT sessions FROM pg_stat_database WHERE datname = $1",
            &[&DATABASE],
        )
        .await
        .unwrap_or_else(|err| fail_db("sessions", &err))
        .get(0)
}
