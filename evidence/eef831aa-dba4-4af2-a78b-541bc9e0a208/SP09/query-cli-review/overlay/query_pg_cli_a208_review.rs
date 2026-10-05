//! Independent review oracle for the shipping `query-pg` binary.
//!
//! This file is copied onto a private archive only. It is not product source.
//! The fixture is `studio.plate` on database `oo_a208`, roles `oo_r_keeper`
//! and `oo_r_reader`. It does not reuse the a198 desk/peek rows.

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use oo_gateway::policy::catalog::{CatalogError, CatalogLimits, UnknownRows, load_policy_catalog};
use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, InstallEffect, LogicalTable, PiiClass, PolicyIdentity,
    TlsEndpoint, compile_column_acl, connect_require_tls, install_column_acl,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;
use tokio_postgres::types::ToSql;

const HOST: &str = "oo-a208-pg";
const DATABASE: &str = "oo_a208";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a208_login";
const SECRET_DIR: &str = "/run/oo-a208/fixture";
const ORACLE_PATH: &str = "/evidence/live/cli-oracle.txt";
const OWNER: &str = "a2080000-0000-4000-8000-000000000208";
const SHARD: &str = "shard_a208";
const SOURCE: &str = "gallery";
const CONFIG: &str = "a208a208a208a208a208a208a208a208a208a208a208a208a208a208a208a208";
const OTHER_DIGEST: &str = "b208b208b208b208b208b208b208b208b208b208b208b208b208b208b208b208";
const TYPE_DIGEST: &str = "c208c208c208c208c208c208c208c208c208c208c208c208c208c208c208c208";
const INPUT_DIGEST: &str = "d208d208d208d208d208d208d208d208d208d208d208d208d208d208d208d208";
const SCHEMA_DIGEST: &str = "e208e208e208e208e208e208e208e208e208e208e208e208e208e208e208e208";
const NAMESPACE: &str = "studio";
const TABLE: &str = "plate";
const GENERATION: i64 = 6;
const OTHER_GENERATION: i64 = 21;
const ADVISORY_LOCK: i64 = 64_042_026_208;
const CAPTION: &str = "画廊\u{0301}の札";
const TOKENS: [&str; 3] = ["tok-north-208", "tok-south-208", "tok-east-208"];
const ENV_PASSWORD: &str = "env-not-used-a208";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn review_query_pg_on_a_fresh_fixture() {
    if env::var("OO_A208_LIVE").ok().as_deref() != Some("1") {
        eprintln!("OO_A208_LIVE is unset; the review comparison was not run");
        return;
    }
    let mut live = Live::setup().await;
    live.keeper_exact().await;
    live.reader_and_stdin().await;
    live.truncate_first_row().await;
    live.guard_refusals().await;
    live.limits_before_connect().await;
    live.audit_before_query().await;
    live.hidden_database_denial().await;
    live.generation_scope().await;
    live.rows_unchanged().await;
    live.oracle.fact("done", "ok");
}

struct Live {
    oracle: Oracle,
    admin: Client,
    login_pass: String,
    shadow: Shadow,
    work: PathBuf,
    before_rows: Vec<Business>,
    before_digest: String,
    before_tuples: (i64, i64, i64),
    bans: Vec<String>,
}

struct Shadow {
    table: String,
    hue: String,
    score: String,
    frame: String,
    caption: String,
    shown: String,
    token: String,
}

#[derive(Clone, PartialEq, Eq)]
struct Business {
    hue: i16,
    score: i32,
    frame: i64,
    caption: Option<String>,
    shown: bool,
    token: String,
}

struct Oracle {
    bans: Vec<String>,
}

impl Oracle {
    fn new(bans: &[String]) -> Self {
        Self { bans: bans.to_vec() }
    }

    fn fact(&mut self, key: &str, value: &str) {
        for ban in &self.bans {
            assert!(!key.contains(ban) && !value.contains(ban), "oracle fact refused");
        }
        assert!(!value.contains("pt_") && !value.contains("pc_"), "oracle physical name");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(ORACLE_PATH)
            .expect("oracle");
        writeln!(file, "{key}={value}").expect("oracle write");
        file.sync_all().expect("oracle sync");
    }
}

struct Run {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    lines: Vec<Value>,
    audit: PathBuf,
}

impl Live {
    async fn setup() -> Self {
        let admin_pass = read_secret("admin.pass");
        let login_pass = read_secret("login.pass");
        let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca");
        let mut bans = vec![admin_pass.clone(), login_pass.clone(), ENV_PASSWORD.to_string()];
        bans.extend(TOKENS.iter().map(|token| (*token).to_string()));
        let mut oracle = Oracle::new(&bans);
        reset_database(&admin_pass, &ca).await;
        let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
        apply_frozen_schema(&mut admin).await;
        create_login(&admin, &login_pass).await;
        grant_mapping_read(&admin).await;
        let shadow = prepare_shadow(&mut admin).await;
        analyze_shadow(&admin, &shadow.table).await;
        let keeper = install_column_acl(
            &mut admin,
            &acl("keeper", &["hue", "score", "frame", "caption", "shown"]),
        )
        .await
        .expect("install keeper");
        let reader = install_column_acl(&mut admin, &acl("reader", &["caption"]))
            .await
            .expect("install reader");
        assert_eq!(keeper.effect, InstallEffect::Installed);
        assert_eq!(reader.effect, InstallEffect::Installed);
        assert_eq!(keeper.role, "oo_r_keeper");
        assert_eq!(reader.role, "oo_r_reader");
        let mut login = connect_db(DATABASE, LOGIN, &login_pass, &ca).await;
        let keeper_policy = compile_column_acl(&acl("keeper", &["hue", "score", "frame", "caption", "shown"]))
            .expect("compile keeper");
        let catalog = load_policy_catalog(
            &mut login,
            &identity("oo_r_keeper"),
            &[keeper_policy],
            &limits(),
        )
        .await
        .expect("keeper catalog");
        let names: Vec<&str> = catalog.relations[0]
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect();
        assert!(!names.contains(&"token"), "token visible to keeper");
        assert!(!names.iter().any(|name| name.starts_with("_oo")));
        oracle.fact("keeper_visible", &names.join(","));
        oracle.fact(
            "keeper_pending",
            &catalog.relations[0].pending_unclassified.to_string(),
        );
        assert!(catalog.relations[0].pending_unclassified >= 2);
        let before_rows = fetch_business(&admin, &shadow).await;
        let before_digest = row_digest(&before_rows);
        let before_tuples = tuples_settled(&admin, &shadow.table).await;
        assert_eq!(before_tuples, (3, 0, 0));
        oracle.fact("row_digest_before", &before_digest);
        let work = scratch();
        Self {
            oracle,
            admin,
            login_pass,
            shadow,
            work,
            before_rows,
            before_digest,
            before_tuples,
            bans,
        }
    }

    async fn keeper_exact(&mut self) {
        let sql = "SELECT hue, score, frame, caption, shown FROM studio.plate ORDER BY hue";
        let admin_rows = fetch_typed(
            &self.admin,
            &select_sql(
                &self.shadow.table,
                &[
                    self.shadow.hue.clone(),
                    self.shadow.score.clone(),
                    self.shadow.frame.clone(),
                    self.shadow.caption.clone(),
                    self.shadow.shown.clone(),
                ],
                &self.shadow.hue,
            ),
            &[Kind::I16, Kind::I32, Kind::I64, Kind::Text, Kind::Bool],
        )
        .await;
        let expect = expected_keeper();
        assert_eq!(admin_rows, expect, "admin typed rows");
        let run = self.run_ok("keeper", &["hue", "score", "frame", "caption", "shown"], "request", Some(sql), None, 20, 8192, false);
        let turn = one_turn(&run.lines);
        assert_cells(&turn.rows, &expect);
        assert_eq!(turn.summary["status"], "succeeded");
        assert_eq!(turn.summary["truncated"], false);
        assert_eq!(turn.summary["returned_rows"], 3);
        assert_eq!(turn.summary["rollback_confirmed"], true);
        assert_eq!(turn.summary["end_audit_ok"], true);
        assert_eq!(turn.summary["sqlstate"], Value::Null);
        assert_summary_hashes(&turn.summary);
        assert_eq!(
            turn.summary["original_sha256"],
            Value::String(sha256_hex(sql.as_bytes()))
        );
        assert_audit(&run.audit, "oo_r_keeper", 2, &self.bans);
        let raw = String::from_utf8(run.stdout.clone()).expect("utf8");
        assert!(raw.contains("\"value\":\"-32768\""), "int16 min is a json string");
        assert!(raw.contains("\"value\":\"-2147483648\""));
        assert!(raw.contains("\"value\":\"-9223372036854775808\""));
        assert!(raw.contains("\"value\":\"9223372036854775807\""));
        assert!(raw.contains(CAPTION));
        assert!(!raw.contains("\"value\":-32768"), "int rendered as a json number");
        self.quiet(&run.stdout, "keeper stdout");
        self.quiet(&run.stderr, "keeper stderr");
        self.oracle.fact("keeper_rows", "3");
        self.oracle.fact("keeper_int16_min", "-32768");
        self.oracle.fact("keeper_int64_min", "-9223372036854775808");
        self.oracle.fact("keeper_int64_max", "9223372036854775807");

        let star_sql = "SELECT * FROM studio.plate ORDER BY hue";
        let star = self.run_ok(
            "keeper",
            &["hue", "score", "frame", "caption", "shown"],
            "request",
            Some(star_sql),
            None,
            20,
            8192,
            true,
        );
        let star_turn = one_turn(&star.lines);
        assert_cells(&star_turn.rows, &expect);
        assert_ne!(
            star_turn.summary["original_sha256"],
            star_turn.summary["executed_sha256"]
        );
        assert!(!star.stdout.windows(ENV_PASSWORD.len()).any(|window| window == ENV_PASSWORD.as_bytes()));
        self.oracle.fact("star_rows", "3");
        self.oracle.fact("star_hides_token", "1");
        self.oracle.fact("env_password_ignored", "1");

        let count_sql = "SELECT count(*) FROM studio.plate";
        let count_admin = fetch_typed(
            &self.admin,
            &format!(
                "SELECT count(*)::int8 FROM \"_oo\".{}",
                quote_physical(&self.shadow.table, "pt_")
            ),
            &[Kind::I64],
        )
        .await;
        let count = self.run_ok("keeper", &["hue", "score", "frame", "caption", "shown"], "request", Some(count_sql), None, 20, 8192, false);
        let count_turn = one_turn(&count.lines);
        assert_cells(&count_turn.rows, &count_admin);
        assert_eq!(count_turn.rows[0][0]["kind"], "int");
        assert_eq!(count_turn.rows[0][0]["value"], "3");
        assert!(count_turn.rows[0][0]["value"].is_string());
        self.oracle.fact("count_star", "3");
    }

    async fn reader_and_stdin(&mut self) {
        let sql = "SELECT caption FROM studio.plate ORDER BY caption";
        let admin_rows = fetch_typed(
            &self.admin,
            &select_sql(&self.shadow.table, std::slice::from_ref(&self.shadow.caption), &self.shadow.caption),
            &[Kind::Text],
        )
        .await;
        let run = self.run_ok("reader", &["caption"], "request", Some(sql), None, 20, 8192, false);
        assert_cells(&one_turn(&run.lines).rows, &admin_rows);
        let text = String::from_utf8(run.stdout.clone()).expect("utf8");
        assert!(!text.contains("-9223372036854775808"));
        self.oracle.fact("reader_rows", &admin_rows.len().to_string());

        let count_sql = "SELECT count(*) FROM studio.plate";
        let turns = format!(
            "{}\n{}\n",
            json!({"version":"oo.gateway.query-pg-turn/v1","sql": count_sql}),
            json!({"version":"oo.gateway.query-pg-turn/v1","sql": sql})
        );
        let stdin_run = self.run_ok("reader", &["caption"], "stdin", None, Some(turns.as_bytes()), 20, 8192, false);
        let grouped = group_turns(&stdin_run.lines);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].rows[0][0]["value"], "3");
        assert_cells(&grouped[1].rows, &admin_rows);
        let audit = audit_values(&stdin_run.audit);
        assert_eq!(audit.len(), 4);
        assert_eq!(audit[0]["record"], "admit");
        assert_eq!(audit[1]["record"], "complete");
        assert_eq!(audit[2]["record"], "admit");
        assert_eq!(audit[3]["record"], "complete");
        assert_eq!(audit[0]["backend_pid"], audit[2]["backend_pid"]);
        assert_eq!(audit[0]["role"], "oo_r_reader");
        assert_eq!(audit[1]["status"], "succeeded");
        assert_eq!(audit[3]["status"], "succeeded");
        self.quiet(&fs::read(&stdin_run.audit).expect("audit"), "stdin audit");
        self.oracle.fact("stdin_turns", "2");
        self.oracle.fact("stdin_audit_lines", "4");
        self.oracle.fact("stdin_same_pid", "1");
    }

    async fn truncate_first_row(&mut self) {
        let sql = "SELECT hue, score, frame, caption, shown FROM studio.plate ORDER BY hue";
        let bytes = first_row_bytes();
        let run = self.run_ok(
            "keeper",
            &["hue", "score", "frame", "caption", "shown"],
            "request",
            Some(sql),
            None,
            20,
            bytes,
            false,
        );
        let turn = one_turn(&run.lines);
        assert_eq!(turn.rows.len(), 1);
        assert_eq!(turn.summary["truncated"], true);
        assert_eq!(turn.summary["status"], "succeeded");
        assert_eq!(turn.summary["returned_rows"], 1);
        assert_eq!(turn.summary["end_audit_ok"], true);
        assert_eq!(turn.rows[0][0]["value"], "-32768");
        self.oracle.fact("truncate_rows", "1");
        self.oracle.fact("truncate_bytes", &bytes.to_string());
    }

    async fn guard_refusals(&mut self) {
        for (name, role, columns, sql) in [
            ("hidden", "reader", &["caption"][..], "SELECT token FROM studio.plate"),
            (
                "write",
                "keeper",
                &["hue", "score", "frame", "caption", "shown"][..],
                "UPDATE studio.plate SET caption = caption",
            ),
            (
                "sleep",
                "keeper",
                &["hue", "score", "frame", "caption", "shown"][..],
                "SELECT pg_sleep(1)",
            ),
        ] {
            let run = self.run("request", role, columns, Some(sql), None, 20, 8192, false, &self.work.join(format!("{name}.jsonl")));
            assert_eq!(run.code, 6, "{name}");
            assert_eq!(run.stderr, b"query-pg: guard\n");
            assert!(run.stdout.is_empty(), "{name} stdout");
            assert!(!run.audit.exists(), "{name} audit");
            self.quiet(&run.stderr, name);
            self.oracle.fact(&format!("{name}_exit"), "6");
            self.oracle.fact(&format!("{name}_audit"), "absent");
        }
    }

    async fn limits_before_connect(&mut self) {
        let before = self.sessions().await;
        let password_audit = self.work.join("password-audit.jsonl");
        let mut doc = self.base("keeper", &["caption"], "request", &password_audit);
        doc["statement"]["sql"] = Value::String(self.login_pass.clone());
        let password_run = self.run_doc(&doc, None, false);
        assert_eq!(password_run.code, 3);
        assert_eq!(password_run.stderr, b"query-pg: request\n");
        assert!(!password_audit.exists());
        assert_eq!(self.sessions().await, before);
        self.oracle.fact("password_in_request_exit", "3");
        self.oracle.fact("password_in_request_sessions", "unchanged");

        let filter_audit = self.work.join("filter-audit.jsonl");
        let mut filter = self.base("reader", &["caption"], "request", &filter_audit);
        filter["policies"][0]["row_filter"] = json!({"kind": "claims", "column": "caption", "claim": "sub"});
        let filter_run = self.run_doc(&filter, None, false);
        assert_eq!(filter_run.code, 5);
        assert_eq!(filter_run.stderr, b"query-pg: policy\n");
        assert!(!filter_audit.exists());
        assert_eq!(self.sessions().await, before);
        self.oracle.fact("row_filter_exit", "5");
        self.oracle.fact("row_filter_sessions", "unchanged");

        let mismatch_audit = self.work.join("mismatch-audit.jsonl");
        let mut mismatch = self.base("reader", &["caption"], "request", &mismatch_audit);
        mismatch["identity"]["role"] = json!("oo_r_keeper");
        let mismatch_run = self.run_doc(&mismatch, None, false);
        assert_eq!(mismatch_run.code, 5);
        assert_eq!(mismatch_run.stderr, b"query-pg: policy\n");
        assert!(!mismatch_audit.exists());
        assert_eq!(self.sessions().await, before);
        self.oracle.fact("role_mismatch_exit", "5");

        let narrow_audit = self.work.join("operators-audit.jsonl");
        let mut narrow = self.base("reader", &["caption"], "request", &narrow_audit);
        narrow["catalog"]["max_operators"] = json!(53);
        let narrow_run = self.run_doc(&narrow, None, false);
        assert_eq!(narrow_run.code, 3);
        assert_eq!(narrow_run.stderr, b"query-pg: request\n");
        assert!(!narrow_audit.exists());
        assert_eq!(self.sessions().await, before);
        self.oracle.fact("operators_limit_exit", "3");

        let nine = (0..9)
            .map(|index| {
                format!(
                    "{}\n",
                    json!({"version":"oo.gateway.query-pg-turn/v1","sql": format!("SELECT caption FROM studio.plate WHERE hue = {index}")})
                )
            })
            .collect::<String>();
        let nine_audit = self.work.join("nine-audit.jsonl");
        let nine_doc = self.base("reader", &["caption"], "stdin", &nine_audit);
        let nine_run = self.run_doc(&nine_doc, Some(nine.as_bytes()), false);
        assert_eq!(nine_run.code, 3);
        assert_eq!(nine_run.stderr, b"query-pg: request\n");
        assert!(!nine_audit.exists());
        assert_eq!(self.sessions().await, before);
        self.oracle.fact("stdin_ninth_exit", "3");

        let existing = self.work.join("existing-audit.jsonl");
        fs::write(&existing, b"keep\n").unwrap();
        let existing_doc = self.base("reader", &["caption"], "request", &existing);
        let existing_run = self.run_doc(&existing_doc, None, false);
        assert_eq!(existing_run.code, 7);
        assert_eq!(existing_run.stderr, b"query-pg: audit\n");
        assert_eq!(fs::read(&existing).unwrap(), b"keep\n");
        assert_eq!(self.sessions().await, before);
        self.oracle.fact("existing_audit_exit", "7");
        self.oracle.fact("existing_audit_sessions", "unchanged");

        let parent = self.work.join("parent-file");
        fs::write(&parent, b"x").unwrap();
        let nested = parent.join("audit.jsonl");
        let nested_doc = self.base("reader", &["caption"], "request", &nested);
        let nested_run = self.run_doc(&nested_doc, None, false);
        assert_eq!(nested_run.code, 7);
        assert_eq!(nested_run.stderr, b"query-pg: audit\n");
        assert_eq!(self.sessions().await, before);
        self.oracle.fact("unwritable_parent_exit", "7");

        let garbage = self.work.join("garbage.pem");
        fs::write(&garbage, b"not-a-certificate\n").unwrap();
        let garbage_audit = self.work.join("garbage-audit.jsonl");
        let garbage_run = self.run_at("reader", &["caption"], "request", Some("SELECT caption FROM studio.plate"), None, &garbage, &garbage_audit, false);
        assert_eq!(garbage_run.code, 4);
        assert_eq!(garbage_run.stderr, b"query-pg: tls\n");
        assert!(!garbage_audit.exists());
        assert_eq!(self.sessions().await, before);
        self.oracle.fact("garbage_ca_exit", "4");
        self.oracle.fact("garbage_ca_sessions", "unchanged");

        let other = Path::new(SECRET_DIR).join("other.crt");
        let other_audit = self.work.join("other-audit.jsonl");
        let other_run = self.run_at("reader", &["caption"], "request", Some("SELECT caption FROM studio.plate"), None, &other, &other_audit, false);
        assert_eq!(other_run.code, 4);
        assert_eq!(other_run.stderr, b"query-pg: tls\n");
        assert!(!other_audit.exists());
        self.oracle.fact("wrong_ca_exit", "4");

        let bare = run_raw(&[], None);
        assert_eq!(bare.status.code(), Some(2));
        assert_eq!(bare.stderr, b"query-pg: usage\n");
        self.oracle.fact("usage_exit", "2");
    }

    async fn audit_before_query(&mut self) {
        let audit = self.work.join("lock-audit.jsonl");
        let mut doc = self.base("reader", &["caption"], "request", &audit);
        doc["statement"]["wall_clock_ms"] = json!(30000);
        let tx = self.admin.transaction().await.expect("lock begin");
        exec_tx(
            &tx,
            "lock-table",
            &format!(
                "LOCK TABLE \"_oo\".{} IN ACCESS EXCLUSIVE MODE",
                quote_physical(&self.shadow.table, "pt_")
            ),
        )
        .await;
        let request = self.work.join("lock-request.json");
        fs::write(&request, serde_json::to_vec(&doc).unwrap()).unwrap();
        let mut child = Command::new(query_pg_exe());
        child.arg("--request").arg(&request);
        child.env_remove("PGPASSWORD");
        child.stdin(Stdio::null());
        child.stdout(Stdio::piped());
        child.stderr(Stdio::piped());
        let mut child = child.spawn().expect("spawn locked query");
        let mut admit_seen = false;
        let mut lock_seen = false;
        let mut gap = false;
        let mut activity = String::from("none");
        let mut early = None;
        for index in 0..160 {
            let admit = audit_has(&audit, "admit");
            let complete = audit_has(&audit, "complete");
            let waiting = count_lock_waits(&tx).await;
            admit_seen |= admit;
            lock_seen |= waiting >= 1;
            if admit && !complete && waiting >= 1 {
                gap = true;
                break;
            }
            if index == 40 || index == 159 {
                activity = activity_summary(&tx).await;
            }
            if let Some(status) = child.try_wait().expect("try_wait") {
                early = Some(status);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if !gap {
            activity = activity_summary(&tx).await;
        }
        tx.commit().await.expect("unlock");
        let output = finish_child(child, early);
        let code = output.status.code().unwrap_or(-1);
        self.oracle.fact("audit_gap", if gap { "1" } else { "0" });
        self.oracle.fact("audit_admit_seen", if admit_seen { "1" } else { "0" });
        self.oracle.fact("audit_lock_seen", if lock_seen { "1" } else { "0" });
        self.oracle.fact("audit_activity", &fact_token(&activity));
        self.oracle.fact("audit_exit", &code.to_string());
        if code == 0 {
            assert!(output.stderr.is_empty());
            let lines = json_lines(&output.stdout);
            let turn = one_turn(&lines);
            assert_eq!(turn.summary["status"], "succeeded");
            assert_eq!(turn.summary["end_audit_ok"], true);
            let records = audit_values(&audit);
            assert_eq!(records[0]["record"], "admit");
            assert_eq!(records[1]["record"], "complete");
            self.quiet(&output.stdout, "locked stdout");
            self.quiet(&fs::read(&audit).unwrap(), "locked audit");
            self.oracle.fact(
                "audit_before_query",
                if gap { "admit_while_locked" } else { "file_order_only" },
            );
            self.oracle.fact("audit_completion", "after_unlock");
        } else {
            self.oracle.fact("audit_before_query", "query_failed");
            self.oracle.fact("audit_completion", "not_checked");
        }
    }

    async fn hidden_database_denial(&mut self) {
        let state = direct_denial(
            &self.login_pass,
            &format!(
                "SELECT {} FROM \"_oo\".{}",
                quote_physical(&self.shadow.token, "pc_"),
                quote_physical(&self.shadow.table, "pt_")
            ),
        )
        .await;
        assert_eq!(state, "42501");
        self.oracle.fact("direct_token_42501", "1");

        let audit = self.work.join("revoke-audit.jsonl");
        let mut doc = self.base("keeper", &["hue", "score", "frame", "caption", "shown"], "request", &audit);
        doc["statement"]["sql"] = json!("SELECT score FROM studio.plate ORDER BY score");
        doc["statement"]["wall_clock_ms"] = json!(30000);
        let tx = self.admin.transaction().await.expect("revoke begin");
        exec_tx(
            &tx,
            "lock-for-revoke",
            &format!(
                "LOCK TABLE \"_oo\".{} IN ACCESS EXCLUSIVE MODE",
                quote_physical(&self.shadow.table, "pt_")
            ),
        )
        .await;
        let request = self.work.join("revoke-request.json");
        fs::write(&request, serde_json::to_vec(&doc).unwrap()).unwrap();
        let mut child = Command::new(query_pg_exe());
        child.arg("--request").arg(&request);
        child.env_remove("PGPASSWORD");
        child.stdin(Stdio::null());
        child.stdout(Stdio::piped());
        child.stderr(Stdio::piped());
        let mut child = child.spawn().expect("spawn revoke query");
        let mut saw = false;
        let mut revoke_activity = String::from("none");
        let mut early = None;
        for index in 0..160 {
            if audit_has(&audit, "admit") && !audit_has(&audit, "complete") && count_lock_waits(&tx).await >= 1 {
                saw = true;
                break;
            }
            if index == 159 {
                revoke_activity = activity_summary(&tx).await;
            }
            if let Some(status) = child.try_wait().expect("try_wait") {
                early = Some(status);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.oracle.fact("revoke_wait", if saw { "1" } else { "0" });
        self.oracle.fact("revoke_activity", &fact_token(&revoke_activity));
        if saw {
            exec_tx(
                &tx,
                "revoke-score",
                &format!(
                    "REVOKE SELECT ({column}) ON TABLE \"_oo\".{table} FROM oo_r_keeper",
                    column = quote_physical(&self.shadow.score, "pc_"),
                    table = quote_physical(&self.shadow.table, "pt_")
                ),
            )
            .await;
        }
        tx.commit().await.expect("revoke commit");
        let output = finish_child(child, early);
        let hidden_code = output.status.code().unwrap_or(-1);
        let lines = if output.stdout.is_empty() { Vec::new() } else { json_lines(&output.stdout) };
        let summary = lines.iter().find(|line| line["record"] == "summary");
        let state = summary
            .and_then(|line| line["sqlstate"].as_str())
            .unwrap_or("none");
        let status = summary
            .and_then(|line| line["status"].as_str())
            .unwrap_or("none");
        let audit_state = if audit.exists() {
            let records = audit_values(&audit);
            match records.last() {
                Some(line) => line["sqlstate"].as_str().unwrap_or("none").to_string(),
                None => "none".to_string(),
            }
        } else {
            "absent".to_string()
        };
        self.quiet(&output.stdout, "42501 stdout");
        self.quiet(&output.stderr, "42501 stderr");
        if audit.exists() {
            self.quiet(&fs::read(&audit).unwrap(), "42501 audit");
        }
        self.oracle.fact("hidden_db_exit", &hidden_code.to_string());
        self.oracle.fact("hidden_db_status", &fact_token(status));
        self.oracle.fact("hidden_db_sqlstate", &fact_token(state));
        self.oracle.fact("hidden_db_audit_sqlstate", &fact_token(&audit_state));
        self.oracle.fact(
            "hidden_db_42501",
            if hidden_code == 8 && state == "42501" && audit_state == "42501" { "1" } else { "0" },
        );
    }

    async fn generation_scope(&mut self) {
        register_level(&self.admin, OTHER_DIGEST, OTHER_GENERATION, "namespace", "n", NAMESPACE, "", "").await;
        register_level(&self.admin, OTHER_DIGEST, OTHER_GENERATION, "table", "t", NAMESPACE, TABLE, "").await;
        register_level(&self.admin, OTHER_DIGEST, OTHER_GENERATION, "column", "c", NAMESPACE, TABLE, "caption").await;
        let mut login = connect_db(
            DATABASE,
            LOGIN,
            &self.login_pass,
            &fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca"),
        )
        .await;
        let current = compile_column_acl(&acl("reader", &["caption"])).expect("current");
        load_policy_catalog(&mut login, &identity("oo_r_reader"), &[current], &limits())
            .await
            .expect("generation 6 still loads");
        let high = ColumnAclPolicy {
            generation: OTHER_GENERATION,
            ..acl("reader", &["caption"])
        };
        let high = compile_column_acl(&high).expect("high");
        let high_err = load_policy_catalog(&mut login, &identity("oo_r_reader"), &[high], &limits())
            .await
            .expect_err("higher generation on this digest");
        assert!(matches!(high_err, CatalogError::StaleGeneration));
        let other_low = ColumnAclPolicy {
            config_digest: OTHER_DIGEST.to_string(),
            generation: GENERATION,
            ..acl("reader", &["caption"])
        };
        let other_low = compile_column_acl(&other_low).expect("other low");
        let mut other_identity = identity("oo_r_reader");
        other_identity.policy_id = OTHER_DIGEST.to_string();
        let low_err = load_policy_catalog(&mut login, &other_identity, &[other_low], &limits())
            .await
            .expect_err("other digest low generation");
        assert!(matches!(low_err, CatalogError::StaleGeneration));
        let other_high = ColumnAclPolicy {
            config_digest: OTHER_DIGEST.to_string(),
            generation: OTHER_GENERATION,
            ..acl("reader", &["caption"])
        };
        let other_high = compile_column_acl(&other_high).expect("other high");
        let high_other = load_policy_catalog(&mut login, &other_identity, &[other_high], &limits())
            .await
            .expect_err("other digest has no shadow");
        let label = match &high_other {
            CatalogError::MappingAbsent => "mapping_absent",
            CatalogError::StaleGeneration => "stale",
            CatalogError::Database { sqlstate } if sqlstate == "42501" => "database_42501",
            CatalogError::Database { .. } => "database",
            _ => "other",
        };
        assert_ne!(label, "stale", "other digest at its own maximum was treated as stale");
        self.oracle.fact("generation_a208_stays", "6");
        self.oracle.fact("generation_a208_high", "stale");
        self.oracle.fact("generation_b208_low", "stale");
        self.oracle.fact("generation_b208_high", label);
        self.oracle.fact("max_generation_scope", "source_and_digest");
    }

    async fn rows_unchanged(&mut self) {
        let after = fetch_business(&self.admin, &self.shadow).await;
        assert_eq!(self.before_rows.len(), after.len());
        assert!(
            self.before_rows.iter().zip(&after).all(|(left, right)| left == right),
            "business rows changed"
        );
        let digest = row_digest(&after);
        assert_eq!(self.before_digest, digest);
        let tuples = tuples(&self.admin, &self.shadow.table).await;
        assert_eq!(self.before_tuples, tuples);
        self.oracle.fact("row_digest_after", &digest);
        self.oracle.fact("tuples_after", "3,0,0");
    }

    #[allow(clippy::too_many_arguments)]
    fn run_ok(
        &self,
        role: &str,
        columns: &[&str],
        source: &str,
        sql: Option<&str>,
        stdin: Option<&[u8]>,
        max_rows: u32,
        max_bytes: u64,
        env_password: bool,
    ) -> Run {
        let audit = self.work.join(format!("ok-{}-{}.jsonl", role, self.work.read_dir().map(|iter| iter.count()).unwrap_or(0)));
        let run = self.run(source, role, columns, sql, stdin, max_rows, max_bytes, env_password, &audit);
        assert_eq!(run.code, 0, "{role} {source}");
        assert!(run.stderr.is_empty());
        run
    }

    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        source: &str,
        role: &str,
        columns: &[&str],
        sql: Option<&str>,
        stdin: Option<&[u8]>,
        max_rows: u32,
        max_bytes: u64,
        env_password: bool,
        audit: &Path,
    ) -> Run {
        let mut doc = self.base(role, columns, source, audit);
        if let Some(sql) = sql {
            doc["statement"]["sql"] = Value::String(sql.to_string());
        } else {
            doc["statement"].as_object_mut().unwrap().remove("sql");
        }
        doc["statement"]["max_rows"] = json!(max_rows);
        doc["statement"]["max_result_bytes"] = json!(max_bytes);
        self.run_doc(&doc, stdin, env_password)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_at(
        &self,
        role: &str,
        columns: &[&str],
        source: &str,
        sql: Option<&str>,
        stdin: Option<&[u8]>,
        ca_file: &Path,
        audit: &Path,
        env_password: bool,
    ) -> Run {
        let mut doc = self.base(role, columns, source, audit);
        doc["ca_file"] = json!(ca_file.display().to_string());
        if let Some(sql) = sql {
            doc["statement"]["sql"] = Value::String(sql.to_string());
        }
        self.run_doc(&doc, stdin, env_password)
    }

    fn run_doc(&self, doc: &Value, stdin: Option<&[u8]>, env_password: bool) -> Run {
        let audit = PathBuf::from(doc["audit_file"].as_str().unwrap());
        let request = self.work.join(format!("request-{}.json", audit.file_name().unwrap().to_string_lossy()));
        fs::write(&request, serde_json::to_vec(doc).unwrap()).unwrap();
        let mut command = Command::new(query_pg_exe());
        command.arg("--request").arg(&request);
        command.env_remove("PGPASSWORD");
        if env_password {
            command.env("PGPASSWORD", ENV_PASSWORD);
        }
        let output = if let Some(bytes) = stdin {
            command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
            let mut child = command.spawn().expect("spawn");
            child.stdin.take().unwrap().write_all(bytes).expect("stdin");
            child.wait_with_output().expect("wait")
        } else {
            command.output().expect("run")
        };
        finish_run(output, audit)
    }

    fn base(&self, role: &str, columns: &[&str], source: &str, audit: &Path) -> Value {
        let password_file = Path::new(SECRET_DIR).join("login.pass");
        let ca_file = Path::new(SECRET_DIR).join("ca.crt");
        let mut doc = json!({
            "version": "oo.gateway.query-pg/v1",
            "endpoint": {"host": HOST, "port": 5432, "database": DATABASE, "user": LOGIN},
            "password_file": password_file.display().to_string(),
            "ca_file": ca_file.display().to_string(),
            "audit_file": audit.display().to_string(),
            "identity": {"project": SOURCE, "role": format!("oo_r_{role}"), "policy_id": CONFIG},
            "policies": [policy_value(role, columns)],
            "catalog": {
                "max_tables": 4,
                "max_columns": 64,
                "max_functions": 1,
                "max_operators": 64,
                "large_table_row_threshold": 1000000,
                "unknown_rows": "refuse"
            },
            "statement": {
                "source": source,
                "max_rows": 20,
                "max_result_bytes": 8192,
                "wall_clock_ms": 20000
            }
        });
        if source == "request" {
            doc["statement"]["sql"] = json!("SELECT caption FROM studio.plate");
        }
        doc
    }

    fn quiet(&self, bytes: &[u8], label: &str) {
        let text = String::from_utf8_lossy(bytes);
        for ban in &self.bans {
            assert!(!text.contains(ban), "{label} contained a banned value");
        }
        assert!(!text.contains("pt_"), "{label} physical table");
        assert!(!text.contains("pc_"), "{label} physical column");
        assert!(!text.contains("postgres://"), "{label} dsn");
    }

    async fn sessions(&self) -> i64 {
        self.admin
            .query_one("SELECT sessions FROM pg_stat_database WHERE datname = $1", &[&DATABASE])
            .await
            .unwrap_or_else(|err| fail_db("sessions", &err))
            .get(0)
    }
}

fn expected_keeper() -> Vec<Vec<Value>> {
    vec![
        row_values(&[
            cell_int("-32768"),
            cell_int("-2147483648"),
            cell_int("-9223372036854775808"),
            cell_text(CAPTION),
            cell_bool(true),
        ]),
        row_values(&[cell_int("0"), cell_int("1"), cell_int("0"), cell_null(), cell_bool(false)]),
        row_values(&[
            cell_int("12"),
            cell_int("40"),
            cell_int("9223372036854775807"),
            cell_text("plate-beta"),
            cell_bool(true),
        ]),
    ]
}

fn first_row_bytes() -> u64 {
    let caption_bytes = u64::try_from(CAPTION.len()).unwrap();
    6 + 11 + 20 + caption_bytes + 4
}

fn cell_null() -> Value {
    json!({"kind": "null"})
}

fn cell_bool(value: bool) -> Value {
    json!({"kind": "bool", "value": value})
}

fn cell_int(value: &str) -> Value {
    json!({"kind": "int", "value": value})
}

fn cell_text(value: &str) -> Value {
    json!({"kind": "text", "value": value})
}

fn row_values(cells: &[Value]) -> Vec<Value> {
    cells.to_vec()
}

struct Turn {
    rows: Vec<Vec<Value>>,
    summary: Value,
}

fn one_turn(lines: &[Value]) -> Turn {
    let rows = lines
        .iter()
        .filter(|line| line["record"] == "row" && line["turn"] == 0)
        .map(|line| line["cells"].as_array().unwrap().clone())
        .collect();
    let summary = lines
        .iter()
        .find(|line| line["record"] == "summary" && line["turn"] == 0)
        .expect("summary")
        .clone();
    Turn { rows, summary }
}

fn group_turns(lines: &[Value]) -> Vec<Turn> {
    let mut turns = Vec::new();
    for turn in 0..2 {
        let rows = lines
            .iter()
            .filter(|line| line["record"] == "row" && line["turn"] == turn)
            .map(|line| line["cells"].as_array().unwrap().clone())
            .collect();
        let summary = lines
            .iter()
            .find(|line| line["record"] == "summary" && line["turn"] == turn)
            .expect("summary")
            .clone();
        turns.push(Turn { rows, summary });
    }
    turns
}

fn assert_cells(got: &[Vec<Value>], expect: &[Vec<Value>]) {
    assert_eq!(got, expect);
}

fn assert_summary_hashes(summary: &Value) {
    for key in ["original_sha256", "executed_sha256"] {
        let value = summary[key].as_str().expect(key);
        assert_eq!(value.len(), 64);
        assert!(value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}

fn assert_audit(path: &Path, role: &str, lines: usize, bans: &[String]) {
    let raw = fs::read(path).expect("audit");
    let text = String::from_utf8(raw.clone()).expect("audit utf8");
    for ban in bans {
        assert!(!text.contains(ban), "audit contained a banned value");
    }
    assert!(!text.contains("SELECT"));
    let values = json_lines(&raw);
    assert_eq!(values.len(), lines);
    assert_eq!(values[0]["record"], "admit");
    assert_eq!(values[1]["record"], "complete");
    assert_eq!(values[0]["protocol"], "oo.gateway.query-pg-audit/v1");
    assert_eq!(values[1]["protocol"], "oo.gateway.query-pg-audit/v1");
    assert_eq!(values[0]["role"], role);
    assert_eq!(values[0]["project"], SOURCE);
    assert_eq!(values[0]["policy_id"], CONFIG);
    assert!(values[0]["backend_pid"].as_i64().unwrap() > 0);
    assert_eq!(values[1]["backend_pid"], values[0]["backend_pid"]);
    assert_eq!(values[1]["status"], "succeeded");
    let fingerprint = values[0]["fingerprint_hex"].as_str().expect("fingerprint");
    assert!((1..=128).contains(&fingerprint.len()));
    assert!(fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let started = values[0]["backend_start"].as_str().expect("backend_start");
    assert!(!started.is_empty() && started.bytes().all(|byte| byte.is_ascii_digit()));
    assert_eq!(values[0]["original_sha256"].as_str().expect("hash").len(), 64);
    assert_eq!(values[0]["executed_sha256"].as_str().expect("hash").len(), 64);
}

fn audit_values(path: &Path) -> Vec<Value> {
    json_lines(&fs::read(path).expect("audit bytes"))
}

fn audit_has(path: &Path, record: &str) -> bool {
    if !path.exists() {
        return false;
    }
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    json_lines(&bytes).iter().any(|line| line["record"] == record)
}

fn json_lines(bytes: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(bytes).expect("utf8");
    text.split('\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("json line"))
        .collect()
}

fn finish_run(output: Output, audit: PathBuf) -> Run {
    let lines = if output.stdout.is_empty() {
        Vec::new()
    } else {
        json_lines(&output.stdout)
    };
    Run {
        code: output.status.code().unwrap_or(-1),
        stdout: output.stdout,
        stderr: output.stderr,
        lines,
        audit,
    }
}

fn query_pg_exe() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_query-pg") {
        return PathBuf::from(path);
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_query_pg") {
        return PathBuf::from(path);
    }
    let mut path = env::current_exe().expect("current_exe");
    path.pop();
    path.pop();
    path.push("query-pg");
    path
}

fn run_raw(args: &[&str], stdin: Option<&[u8]>) -> Output {
    let mut command = Command::new(query_pg_exe());
    command.args(args);
    command.env_remove("PGPASSWORD");
    if let Some(bytes) = stdin {
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn");
        child.stdin.take().unwrap().write_all(bytes).expect("stdin");
        child.wait_with_output().expect("wait")
    } else {
        command.output().expect("run")
    }
}

fn scratch() -> PathBuf {
    let path = env::temp_dir().join(format!("oo-a208-review-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn read_secret(name: &str) -> String {
    let raw = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret");
    let value = raw.trim();
    assert!(
        (16..=128).contains(&value.len())
            && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{name} is not hex"
    );
    value.to_string()
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
        raw.extend_from_slice(&row.hue.to_be_bytes());
        raw.extend_from_slice(&row.score.to_be_bytes());
        raw.extend_from_slice(&row.frame.to_be_bytes());
        match &row.caption {
            Some(caption) => {
                raw.push(1);
                raw.extend_from_slice(&(caption.len() as u32).to_be_bytes());
                raw.extend_from_slice(caption.as_bytes());
            }
            None => raw.push(0),
        }
        raw.push(u8::from(row.shown));
        raw.extend_from_slice(&(row.token.len() as u32).to_be_bytes());
        raw.extend_from_slice(row.token.as_bytes());
    }
    sha256_hex(&raw)
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
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE datname IN ('oo_a208', 'oo_a190') AND pid <> pg_backend_pid()",
    )
    .await;
    exec_hidden(&maintenance, "drop-a190", "DROP DATABASE IF EXISTS \"oo_a190\" WITH (FORCE)").await;
    exec_hidden(
        &maintenance,
        "drop-database",
        &format!("DROP DATABASE IF EXISTS \"{DATABASE}\" WITH (FORCE)"),
    )
    .await;
    exec_hidden(
        &maintenance,
        "drop-roles",
        "DO $$\n\
         DECLARE role_name text;\n\
         BEGIN\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a208_login') THEN\n\
             DROP ROLE oo_a208_login;\n\
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
    client.batch_execute(sql).await.unwrap_or_else(|err| fail_db(name, &err));
}

async fn exec_tx(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql).await.unwrap_or_else(|err| fail_db(name, &err));
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
    exec_tx(&tx, "row-lock", "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    exec_tx(&tx, "0003_target_segment_preflight_expand.sql", &preflight.0).await;
    exec_tx(&tx, "0004_physical_mapping.sql", &mapping.0).await;
    exec_tx(
        &tx,
        "markers-target",
        &marker_sql(&[
            ("target-bootstrap/1", &bootstrap.1, "0000_bootstrap_roles.sql"),
            ("target-fence-receipts/1", &fence.1, "0001_target_fence_expand.sql"),
            ("target-fence-receipts/3", &preflight.1, "0003_target_segment_preflight_expand.sql"),
            ("physical-mapping/v1", &mapping.1, "0004_physical_mapping.sql"),
        ]),
    )
    .await;
    tx.commit().await.expect("commit target schema");
    let tx = begin_locked(client).await;
    exec_tx(&tx, "row-lock-import", "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    exec_tx(&tx, "stats-grant", "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE").await;
    exec_tx(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    exec_tx(&tx, "0005_full_import_expand.sql", &full_import.0).await;
    exec_tx(&tx, "marker-import", &marker_sql(&[("full-import/v1", &full_import.1, "0005_full_import_expand.sql")])).await;
    tx.commit().await.expect("commit full import");
}

fn migration(name: &str) -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../oo-pipeline/migrations").join(name);
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
        values.push_str(&format!("  ('{version}', '{checksum}', 'expand', '{filename}')"));
    }
    format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}")
}

async fn begin_locked(client: &mut Client) -> tokio_postgres::Transaction<'_> {
    let tx = client.transaction().await.expect("begin");
    exec_tx(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    exec_tx(&tx, "statement-timeout", "SET LOCAL statement_timeout = '60s'").await;
    exec_tx(&tx, "advisory-lock", &format!("SELECT pg_catalog.pg_advisory_xact_lock({ADVISORY_LOCK})")).await;
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
        .query_one("SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)", &[&SHARD, &OWNER])
        .await
        .unwrap_or_else(|err| fail_db("adopt", &err))
        .get(0);
    client
        .execute("SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)", &[&SHARD, &OWNER, &target])
        .await
        .unwrap_or_else(|err| fail_db("bind", &err));
    let columns = ["hue", "score", "frame", "caption", "shown", "token"];
    register(client, CONFIG, GENERATION, "namespace", "n", NAMESPACE, "", "").await;
    register(client, CONFIG, GENERATION, "table", "t", NAMESPACE, TABLE, "").await;
    for column in columns {
        register(client, CONFIG, GENERATION, "column", "c", NAMESPACE, TABLE, column).await;
    }
    let table = lookup(client, "table", NAMESPACE, TABLE, "").await;
    let hue = lookup(client, "column", NAMESPACE, TABLE, "hue").await;
    let score = lookup(client, "column", NAMESPACE, TABLE, "score").await;
    let frame = lookup(client, "column", NAMESPACE, TABLE, "frame").await;
    let caption = lookup(client, "column", NAMESPACE, TABLE, "caption").await;
    let shown = lookup(client, "column", NAMESPACE, TABLE, "shown").await;
    let token = lookup(client, "column", NAMESPACE, TABLE, "token").await;
    let oids = vec![
        type_oid(client, "int2").await,
        type_oid(client, "int4").await,
        type_oid(client, "int8").await,
        type_oid(client, "text").await,
        type_oid(client, "bool").await,
        type_oid(client, "text").await,
    ];
    let schemas = vec![NAMESPACE.to_string()];
    let tables = vec![TABLE.to_string()];
    let physical = vec![table.clone()];
    let open_params: [&(dyn ToSql + Sync); 12] = [
        &SHARD, &OWNER, &target, &SOURCE, &GENERATION, &CONFIG, &TYPE_DIGEST, &INPUT_DIGEST, &SCHEMA_DIGEST, &schemas, &tables, &physical,
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
        &SHARD, &OWNER, &target, &run_id, &index, &SOURCE, &CONFIG, &GENERATION, &NAMESPACE, &TABLE, &idents, &oids, &typmods, &required,
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
    let shadow = Shadow { table: table.clone(), hue, score, frame, caption, shown, token };
    insert_rows(client, &shadow).await;
    shadow
}

#[allow(clippy::too_many_arguments)]
async fn register_level(
    client: &Client,
    digest: &str,
    generation: i64,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    register(client, digest, generation, level, letter, namespace, table, column).await;
}

#[allow(clippy::too_many_arguments)]
async fn register(
    client: &Client,
    digest_text: &str,
    generation: i64,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let payload: [&(dyn ToSql + Sync); 7] = [&SOURCE, &digest_text, &generation, &letter, &namespace, &table, &column];
    let digest: Vec<u8> = client
        .query_one("SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))", &payload)
        .await
        .unwrap_or_else(|err| fail_db("digest", &err))
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [&SOURCE, &digest_text, &generation, &level, &namespace, &table, &column, &digest];
    client
        .execute("SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)", &bind)
        .await
        .unwrap_or_else(|err| fail_db("register", &err));
}

async fn lookup(client: &Client, level: &str, namespace: &str, table: &str, column: &str) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [&SOURCE, &CONFIG, &GENERATION, &level, &namespace, &table, &column];
    client
        .query_one("SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)", &params)
        .await
        .unwrap_or_else(|err| fail_db("lookup", &err))
        .get(0)
}

async fn type_oid(client: &Client, name: &str) -> i64 {
    let oid: i32 = client
        .query_one(
            "SELECT type_row.oid::int4 FROM pg_catalog.pg_type AS type_row \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace \
             WHERE namespace.nspname = 'pg_catalog' AND type_row.typname = $1 AND type_row.typtype = 'b'",
            &[&name],
        )
        .await
        .unwrap_or_else(|err| fail_db("type-oid", &err))
        .get(0);
    i64::from(oid)
}

async fn insert_rows(client: &mut Client, shadow: &Shadow) {
    let sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {hue}, {score}, {frame}, {caption}, {shown}, {token}) \
         VALUES (($1::text)::numeric, $2, $3, $4, $5, $6, $7)",
        table = quote_physical(&shadow.table, "pt_"),
        hue = quote_physical(&shadow.hue, "pc_"),
        score = quote_physical(&shadow.score, "pc_"),
        frame = quote_physical(&shadow.frame, "pc_"),
        caption = quote_physical(&shadow.caption, "pc_"),
        shown = quote_physical(&shadow.shown, "pc_"),
        token = quote_physical(&shadow.token, "pc_"),
    );
    let rows = [
        (i16::MIN, i32::MIN, i64::MIN, Some(CAPTION), true, TOKENS[0]),
        (0, 1, 0, None, false, TOKENS[1]),
        (12, 40, i64::MAX, Some("plate-beta"), true, TOKENS[2]),
    ];
    let tx = client.transaction().await.expect("insert begin");
    for (index, (hue, score, frame, caption, shown, token)) in rows.into_iter().enumerate() {
        let pos = (index + 1).to_string();
        tx.execute(&sql, &[&pos, &hue, &score, &frame, &caption, &shown, &token])
            .await
            .unwrap_or_else(|err| fail_db("insert", &err));
    }
    tx.commit().await.expect("insert commit");
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical length");
    assert_eq!(rest.len(), 60, "physical body");
    assert!(rest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    format!("\"{name}\"")
}

async fn analyze_shadow(client: &Client, table: &str) {
    exec_hidden(client, "analyze", &format!("ANALYZE \"_oo\".{}", quote_physical(table, "pt_"))).await;
}

fn select_sql(table: &str, columns: &[String], order: &str) -> String {
    let list = columns.iter().map(|column| quote_physical(column, "pc_")).collect::<Vec<_>>().join(", ");
    format!(
        "SELECT {list} FROM \"_oo\".{table} ORDER BY {order}",
        table = quote_physical(table, "pt_"),
        order = quote_physical(order, "pc_")
    )
}

#[derive(Clone, Copy)]
enum Kind {
    I16,
    I32,
    I64,
    Text,
    Bool,
}

async fn fetch_typed(client: &Client, sql: &str, kinds: &[Kind]) -> Vec<Vec<Value>> {
    let rows = client.query(sql, &[]).await.unwrap_or_else(|err| fail_db("admin-read", &err));
    rows.iter().map(|row| decode(row, kinds)).collect()
}

fn decode(row: &tokio_postgres::Row, kinds: &[Kind]) -> Vec<Value> {
    kinds
        .iter()
        .enumerate()
        .map(|(index, kind)| match kind {
            Kind::I16 => cell_int(&row.get::<_, i16>(index).to_string()),
            Kind::I32 => cell_int(&row.get::<_, i32>(index).to_string()),
            Kind::I64 => cell_int(&row.get::<_, i64>(index).to_string()),
            Kind::Text => match row.get::<_, Option<String>>(index) {
                Some(text) => cell_text(&text),
                None => cell_null(),
            },
            Kind::Bool => cell_bool(row.get(index)),
        })
        .collect()
}

async fn fetch_business(client: &Client, shadow: &Shadow) -> Vec<Business> {
    let sql = select_sql(
        &shadow.table,
        &[
            shadow.hue.clone(),
            shadow.score.clone(),
            shadow.frame.clone(),
            shadow.caption.clone(),
            shadow.shown.clone(),
            shadow.token.clone(),
        ],
        &shadow.hue,
    );
    let rows = client.query(&sql, &[]).await.unwrap_or_else(|err| fail_db("business", &err));
    rows.iter()
        .map(|row| Business {
            hue: row.get(0),
            score: row.get(1),
            frame: row.get(2),
            caption: row.get(3),
            shown: row.get(4),
            token: row.get(5),
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
    client.execute("SELECT pg_stat_clear_snapshot()", &[]).await.unwrap_or_else(|err| fail_db("tuples", &err));
    let row = client
        .query_one(
            "SELECT n_tup_ins, n_tup_upd, n_tup_del FROM pg_stat_user_tables WHERE schemaname = '_oo' AND relname = $1",
            &[&table],
        )
        .await
        .unwrap_or_else(|err| fail_db("tuples", &err));
    (row.get(0), row.get(1), row.get(2))
}

fn finish_child(mut child: std::process::Child, early: Option<std::process::ExitStatus>) -> Output {
    if let Some(status) = early {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_end(&mut stdout).expect("stdout");
        }
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_end(&mut stderr).expect("stderr");
        }
        return Output { status, stdout, stderr };
    }
    child.wait_with_output().expect("wait child")
}

fn fact_token(raw: &str) -> String {
    raw.chars()
        .filter(|char| char.is_ascii_alphanumeric() || matches!(char, '_' | '-' | '/' | ':' | ',' | '.'))
        .take(180)
        .collect()
}

async fn activity_summary(tx: &tokio_postgres::Transaction<'_>) -> String {
    let rows = tx
        .query(
            "SELECT coalesce(wait_event_type, 'none'), coalesce(wait_event, 'none'), \
                    coalesce(state, 'none'), count(*)::int8 \
             FROM pg_stat_activity \
             WHERE datname = $1 AND application_name = 'oo-gateway-policy' \
             GROUP BY 1, 2, 3 ORDER BY 1, 2, 3",
            &[&DATABASE],
        )
        .await
        .unwrap_or_else(|err| fail_db("activity", &err));
    if rows.is_empty() {
        return "none".to_string();
    }
    rows.iter()
        .map(|row| {
            let kind: String = row.get(0);
            let event: String = row.get(1);
            let state: String = row.get(2);
            let count: i64 = row.get(3);
            format!("{kind}/{event}/{state}/{count}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

async fn count_lock_waits(tx: &tokio_postgres::Transaction<'_>) -> i64 {
    tx.query_one(
        "SELECT count(*)::int8 FROM pg_stat_activity \
         WHERE datname = $1 AND application_name = 'oo-gateway-policy' AND wait_event_type = 'Lock'",
        &[&DATABASE],
    )
    .await
    .unwrap_or_else(|err| fail_db("lock-wait", &err))
    .get(0)
}

async fn direct_denial(login_pass: &str, sql: &str) -> String {
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca");
    let login = connect_db(DATABASE, LOGIN, login_pass, &ca).await;
    let wrapped = format!("SET ROLE oo_r_reader; {sql}");
    match login.batch_execute(&wrapped).await {
        Ok(()) => panic!("hidden select was allowed"),
        Err(err) => err.code().map(|code| code.code().to_string()).unwrap_or_default(),
    }
}

