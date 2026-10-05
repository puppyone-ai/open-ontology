//! CLI chain on one private TLS PostgreSQL 17.10.
//!
//! Compiled commands, in order: `catalog-pg`, `captured-pg-draft`,
//! `review-pg-draft`, then `role-pack` fed the review command's own stdout.
//! `import-pg` runs after the public installer API, because that command has
//! no install subcommand. `query-pg` then loads the live catalog. Column
//! grants have no command. `install_column_acl` is called only after a
//! draft-shaped policy has been refused. The product installer grants column
//! `SELECT` for the first table of a role and returns
//! "existing column grants do not match" for a second table. This binary
//! records that refusal and does not add the grant by hand. `query-pg` then
//! reads only the granted party table. The operator policy is labeled as
//! such. This binary does not publish a shadow and does not edit product code.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use oo_gateway::{
    connect_require_tls, install_column_acl, ClassStatus, ColumnAclPolicy, ColumnSpec,
    InstallEffect, LogicalTable, PiiClass, TlsConnection, TlsEndpoint,
};
use oo_pipeline::target_installer::{install, install_full_import};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use tokio_postgres::Client;

const SOURCE_ID: &str = "a211src";
const SOURCE_DB: &str = "oo_a211_src";
const TARGET_DB: &str = "oo_a211_dst";
const READER: &str = "oo_a211_reader";
const WRITER: &str = "oo_a211_writer";
const RUNTIME: &str = "oo_a211_runtime";
const LOGIN: &str = "oo_a211_login";
const SCHEMA: &str = "trade";
const OWNER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a211";
const SHARD: &str = "a211main";
const GENERATION: u64 = 11;
const COMMENT_MARKER: &str = "甲注评";

struct Live {
    evidence: PathBuf,
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    reader_password: String,
    writer_password: String,
    runtime_password: String,
    login_password: String,
    ca: Vec<u8>,
    pipeline: PathBuf,
    builder: PathBuf,
    query: PathBuf,
}

struct Stop {
    step: String,
    detail: String,
}

struct CmdOut {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

struct CellExpect {
    kind: &'static str,
    value: Option<&'static str>,
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let code = match runtime.block_on(run()) {
        Ok(()) => 0,
        Err(stop) => {
            eprintln!("stop step={} detail={}", stop.step, stop.detail);
            let _ = fs::write(
                "/evidence/live/stop.json",
                format!(
                    "{{\"step\":\"{}\",\"detail\":{}}}\n",
                    stop.step,
                    serde_json::to_string(&stop.detail).unwrap_or_else(|_| "\"unprintable\"".to_string())
                ),
            );
            1
        }
    };
    std::process::exit(code);
}

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
async fn run() -> Result<(), Stop> {
    let live = load_live()?;
    fs::create_dir_all(live.evidence.join("steps")).map_err(|err| disk("evidence", &err))?;
    let mut admin = dial(&live, &live.admin_user, &live.admin_password, "postgres").await?;
    reset_cluster(&mut admin).await?;
    create_roles(&live, &mut admin).await?;
    exec_sql(
        &mut admin,
        &format!(
            "CREATE DATABASE {SOURCE_DB} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'"
        ),
    )
    .await?;
    exec_sql(
        &mut admin,
        &format!(
            "CREATE DATABASE {TARGET_DB} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'"
        ),
    )
    .await?;
    exec_sql(
        &mut admin,
        &format!("GRANT CONNECT ON DATABASE {SOURCE_DB} TO {READER}, {WRITER}"),
    )
    .await?;
    exec_sql(
        &mut admin,
        &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN}"),
    )
    .await?;
    drop(admin);

    let mut source = dial(&live, &live.admin_user, &live.admin_password, SOURCE_DB).await?;
    exec_sql(&mut source, FIXTURE_SQL).await?;
    let reader_insert: bool = scalar_bool(
        &mut source,
        "SELECT has_table_privilege('oo_a211_reader', 'trade.party', 'INSERT')",
    )
    .await?;
    if reader_insert {
        return Err(Stop::new("fixture", "reader has INSERT on trade.party"));
    }
    drop(source);

    let mut reader = dial(&live, READER, &live.reader_password, SOURCE_DB).await?;
    let denied = expect_sqlstate(
        &mut reader,
        "INSERT INTO trade.party (party_id, display_name, active) VALUES (9, 'no', true)",
        "42501",
    )
    .await?;
    if !denied {
        return Err(Stop::new("source-write", "reader insert was not 42501"));
    }
    drop(reader);

    let catalog_bytes = source_request(&live);
    let catalog = run_cmd(&live.pipeline, &["catalog-pg"], &catalog_bytes, 120)?;
    save_cmd(&live, "catalog-pg", &catalog)?;
    if catalog.code != 0 {
        return Err(Stop::new(
            "catalog-pg",
            format!("exit {} stderr={}", catalog.code, stderr_text(&catalog.stderr)),
        ));
    }
    if catalog.stdout.windows(COMMENT_MARKER.len()).any(|w| w == COMMENT_MARKER.as_bytes()) {
        return Err(Stop::new("catalog-pg", "comment body was copied into the catalog"));
    }
    let catalog_value: Value = serde_json::from_slice(&catalog.stdout)
        .map_err(|_| Stop::new("catalog-pg", "stdout is not one JSON value"))?;
    if catalog_value["protocol"] != "oo.source.pgsql/v1" {
        return Err(Stop::new("catalog-pg", "protocol is not oo.source.pgsql/v1"));
    }

    let captured = run_cmd(&live.builder, &["captured-pg-draft"], &catalog.stdout, 60)?;
    save_cmd(&live, "captured-pg-draft", &captured)?;
    if captured.code != 0 {
        return Err(Stop::new(
            "captured-pg-draft",
            format!("exit {} stderr={}", captured.code, stderr_text(&captured.stderr)),
        ));
    }
    let captured_value: Value = serde_json::from_slice(&captured.stdout)
        .map_err(|_| Stop::new("captured-pg-draft", "stdout is not one JSON value"))?;
    if captured_value["protocol"] != "oo.builder.captured-pg-draft-command/v1" {
        return Err(Stop::new("captured-pg-draft", "protocol mismatch"));
    }
    let reviews = structural_reviews(&captured_value)?;
    let review_input = json!({
        "catalog": catalog_value,
        "reviews": reviews,
    });
    let review_bytes = serde_json::to_vec(&review_input)
        .map_err(|_| Stop::new("review-pg-draft", "could not encode the review document"))?;
    let reviewed = run_cmd(&live.builder, &["review-pg-draft"], &review_bytes, 60)?;
    save_cmd(&live, "review-pg-draft", &reviewed)?;
    if reviewed.code != 0 {
        return Err(Stop::new(
            "review-pg-draft",
            format!("exit {} stderr={}", reviewed.code, stderr_text(&reviewed.stderr)),
        ));
    }
    let reviewed_value: Value = serde_json::from_slice(&reviewed.stdout)
        .map_err(|_| Stop::new("review-pg-draft", "stdout is not one JSON value"))?;
    let bare_hash = captured_value["wrapper"]["draft_hash"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let reviewed_hash = reviewed_value["wrapper"]["draft_hash"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if reviews.as_array().is_some_and(|items| !items.is_empty()) && bare_hash == reviewed_hash {
        return Err(Stop::new(
            "review-pg-draft",
            "structural accepts did not change draft_hash",
        ));
    }
    let publication = reviewed_value["wrapper"]["publication"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let grants = reviewed_value["wrapper"]["grants"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let live_review = reviewed_value["wrapper"]["live_catalog_review"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let row_import = reviewed_value["wrapper"]["row_import"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let integration = reviewed_value["draft"]["provenance"]["pg_catalog_integration"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if publication != "unavailable"
        || grants != "not_authorized"
        || live_review != "pending"
        || row_import != "not_proven"
        || integration != "not_accepted"
    {
        return Err(Stop::new(
            "review-pg-draft",
            format!("status publication={publication} grants={grants} live={live_review} import={row_import} integration={integration}"),
        ));
    }
    if !reviewed_value["draft"]["effective"]["datasets"]
        .as_array()
        .is_some_and(|sets| {
            sets.iter().all(|set| {
                set["columns"].as_array().is_some_and(|columns| {
                    columns.iter().all(|column| column["pii"].is_null() && column["semantic_type"].is_null())
                })
            })
        })
    {
        return Err(Stop::new(
            "review-pg-draft",
            "effective column pii or semantic_type was filled",
        ));
    }

    let raw_pack = run_cmd(&live.builder, &["role-pack"], &reviewed.stdout, 60)?;
    save_cmd(&live, "role-pack-raw", &raw_pack)?;
    if raw_pack.code == 0 || !raw_pack.stdout.is_empty() {
        return Err(Stop::new(
            "role-pack-raw",
            "review stdout was accepted as a role pack",
        ));
    }
    let wrapped = json!({
        "protocol": "oo.builder.role-pack-command/v1",
        "effective_oos": reviewed_value["draft"],
        "visibility": {"protocol": "oo.builder.role-visibility/v1"}
    });
    let wrapped_bytes = serde_json::to_vec(&wrapped)
        .map_err(|_| Stop::new("role-pack-wrapped", "could not encode the probe"))?;
    let wrapped_pack = run_cmd(&live.builder, &["role-pack"], &wrapped_bytes, 60)?;
    save_cmd(&live, "role-pack-wrapped", &wrapped_pack)?;
    if wrapped_pack.code == 0 || !wrapped_pack.stdout.is_empty() {
        return Err(Stop::new(
            "role-pack-wrapped",
            "the structure draft was accepted as effective OOS",
        ));
    }

    let mut target = dial(&live, &live.admin_user, &live.admin_password, TARGET_DB).await?;
    let installed = install(&mut target).await.map_err(|err| {
        Stop::new("install", format!("installer API {}", scrub(&err.to_string(), &live)))
    })?;
    fs::write(
        live.evidence.join("install-outcome.txt"),
        format!("{installed}\n"),
    )
    .map_err(|err| disk("install", &err))?;
    let early = run_cmd(&live.pipeline, &["import-pg"], &import_request(&live), 120)?;
    save_cmd(&live, "import-pg-not-ready", &early)?;
    if early.code != 1
        || !early.stdout.is_empty()
        || !stderr_text(&early.stderr).contains("target schema is not ready")
    {
        return Err(Stop::new(
            "import-pg-not-ready",
            format!("exit {} stderr={}", early.code, stderr_text(&early.stderr)),
        ));
    }
    let full = install_full_import(&mut target).await.map_err(|err| {
        Stop::new("install-full", format!("installer API {}", scrub(&err.to_string(), &live)))
    })?;
    fs::write(
        live.evidence.join("install-full-outcome.txt"),
        format!("{full}\n"),
    )
    .map_err(|err| disk("install-full", &err))?;
    exec_sql(&mut target, "GRANT oo_pipeline_runtime TO oo_a211_runtime").await?;
    drop(target);

    let imported = run_cmd(&live.pipeline, &["import-pg"], &import_request(&live), 180)?;
    save_cmd(&live, "import-pg", &imported)?;
    if imported.code != 0 {
        return Err(Stop::new(
            "import-pg",
            format!("exit {} stderr={}", imported.code, stderr_text(&imported.stderr)),
        ));
    }
    let import_value: Value = serde_json::from_slice(imported.stdout.trim_ascii_end())
        .map_err(|_| Stop::new("import-pg", "stdout is not one JSON value"))?;
    if import_value["protocol"] != "oo.pipeline.import-pg-result/v1"
        || import_value["state"] != "awaiting_publish"
        || import_value["source_generation"] != GENERATION
        || import_value["table_count"] != 2
    {
        return Err(Stop::new("import-pg", "result is not the awaiting_publish summary"));
    }
    let mut target = dial(&live, &live.admin_user, &live.admin_password, TARGET_DB).await?;
    compare_import(&live, &mut target, &import_value).await?;
    let (config_digest, generation) = mapping_identity(&mut target).await?;
    let source_generation = import_value["source_generation"].as_i64().unwrap_or(-1);
    if generation != source_generation {
        return Err(Stop::new(
            "mapping",
            "mapping run_generation differs from source_generation",
        ));
    }
    exec_sql(
        &mut target,
        "GRANT USAGE ON SCHEMA _oo TO oo_a211_login;\n\
         GRANT SELECT ON TABLE _oo.physical_mappings TO oo_a211_login;\n\
         GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_a211_login",
    )
    .await?;
    let publications: String = scalar_text(&mut target, "SELECT count(*)::text FROM pg_catalog.pg_publication").await?;
    if publications != "0" {
        return Err(Stop::new("publication", format!("pg_publication count {publications}")));
    }
    drop(target);

    let mut source = dial(&live, &live.admin_user, &live.admin_password, SOURCE_DB).await?;
    let reader_writes = scalar_text(
        &mut source,
        "SELECT count(*)::text FROM trade.write_audit WHERE who = 'oo_a211_reader'",
    )
    .await?;
    if reader_writes != "0" {
        return Err(Stop::new("source-audit", format!("reader writes {reader_writes}")));
    }
    drop(source);
    let mut writer = dial(&live, WRITER, &live.writer_password, SOURCE_DB).await?;
    exec_sql(
        &mut writer,
        "INSERT INTO trade.party (party_id, display_name, active) VALUES (9, 'after', true)",
    )
    .await?;
    drop(writer);
    let mut source = dial(&live, &live.admin_user, &live.admin_password, SOURCE_DB).await?;
    let reader_writes = scalar_text(
        &mut source,
        "SELECT count(*)::text FROM trade.write_audit WHERE who = 'oo_a211_reader'",
    )
    .await?;
    let writer_writes = scalar_text(
        &mut source,
        "SELECT count(*)::text FROM trade.write_audit WHERE who = 'oo_a211_writer'",
    )
    .await?;
    if reader_writes != "0" || writer_writes != "1" {
        return Err(Stop::new(
            "source-audit",
            format!("reader={reader_writes} writer={writer_writes}"),
        ));
    }
    drop(source);

    let draft_policy = query_request(
        &live,
        "clerk",
        &config_digest,
        generation,
        "audit-draft-policy.jsonl",
        &[
            ("party", &["party_id", "display_name", "active", "note"]),
            ("ledger", &["entry_id", "party_id", "amount", "memo"]),
        ],
        "unclassified",
        "unknown",
        "request",
        Some("SELECT party_id FROM trade.party"),
    )?;
    let draft_query = run_query(&live, "query-draft-policy", &draft_policy, None)?;
    if draft_query.code == 0 {
        return Err(Stop::new(
            "query-draft-policy",
            "unpublished classification was accepted",
        ));
    }
    if draft_query.code != 5 {
        return Err(Stop::new(
            "query-draft-policy",
            format!("exit {} stderr={}", draft_query.code, stderr_text(&draft_query.stderr)),
        ));
    }
    if Path::new("/evidence/live/audit-draft-policy.jsonl").exists() {
        return Err(Stop::new("query-draft-policy", "audit file was created"));
    }

    let acl = install_acls(&live, &config_digest, generation).await?;
    prove_direct_acl(&live, &import_value).await?;

    let party_all = "SELECT party_id, display_name, active, note FROM trade.party ORDER BY party_id";
    let party_visible = "SELECT party_id, display_name FROM trade.party ORDER BY party_id";
    let clerk_turns = format!(
        "{}\n{}\n",
        json!({"version":"oo.gateway.query-pg-turn/v1","sql": party_all}),
        json!({"version":"oo.gateway.query-pg-turn/v1","sql": party_visible})
    );
    let before_request = query_request(
        &live,
        "clerk",
        &config_digest,
        generation,
        "audit-clerk-before-analyze.jsonl",
        &[("party", &["party_id", "display_name", "active", "note"])],
        "confirmed",
        "none",
        "stdin",
        None,
    )?;
    let before_analyze = run_query(
        &live,
        "query-clerk-before-analyze",
        &before_request,
        Some(clerk_turns.as_bytes()),
    )?;
    if before_analyze.code != 5 || !before_analyze.stdout.is_empty() {
        return Err(Stop::new(
            "query-clerk-before-analyze",
            format!(
                "exit {} stderr={}",
                before_analyze.code,
                stderr_text(&before_analyze.stderr)
            ),
        ));
    }
    if Path::new("/evidence/live/audit-clerk-before-analyze.jsonl").exists() {
        return Err(Stop::new(
            "query-clerk-before-analyze",
            "unknown reltuples created an audit file",
        ));
    }
    analyze_imported(&live, &import_value).await?;
    let clerk_request = query_request(
        &live,
        "clerk",
        &config_digest,
        generation,
        "audit-clerk.jsonl",
        &[("party", &["party_id", "display_name", "active", "note"])],
        "confirmed",
        "none",
        "stdin",
        None,
    )?;
    let clerk = run_query(&live, "query-clerk", &clerk_request, Some(clerk_turns.as_bytes()))?;
    if clerk.code != 0 {
        return Err(Stop::new(
            "query-clerk",
            format!("exit {} stderr={}", clerk.code, stderr_text(&clerk.stderr)),
        ));
    }
    expect_query(
        &clerk.stdout,
        &[
            (
                0,
                &[
                    &[
                        CellExpect { kind: "int", value: Some("1") },
                        CellExpect { kind: "text", value: Some("甲公司") },
                        CellExpect { kind: "bool", value: Some("true") },
                        CellExpect { kind: "null", value: None },
                    ][..],
                    &[
                        CellExpect { kind: "int", value: Some("2") },
                        CellExpect { kind: "text", value: Some("乙部") },
                        CellExpect { kind: "bool", value: Some("false") },
                        CellExpect { kind: "text", value: Some("备注") },
                    ][..],
                ][..],
            ),
            (
                1,
                &[
                    &[
                        CellExpect { kind: "int", value: Some("1") },
                        CellExpect { kind: "text", value: Some("甲公司") },
                    ][..],
                    &[
                        CellExpect { kind: "int", value: Some("2") },
                        CellExpect { kind: "text", value: Some("乙部") },
                    ][..],
                ][..],
            ),
        ],
    )?;
    expect_audit(
        &live,
        "/evidence/live/audit-clerk.jsonl",
        "oo_r_clerk",
        &config_digest,
        &[party_all, party_visible],
    )?;

    let ledger_sql = "SELECT entry_id, party_id, amount, memo FROM trade.ledger ORDER BY entry_id";
    let ledger_request = query_request(
        &live,
        "clerk",
        &config_digest,
        generation,
        "audit-ledger.jsonl",
        &[("ledger", &["entry_id", "party_id", "amount", "memo"])],
        "confirmed",
        "none",
        "request",
        Some(ledger_sql),
    )?;
    let ledger = run_query(&live, "query-ledger", &ledger_request, None)?;
    expect_refusal(&live, "query-ledger", &ledger, "/evidence/live/audit-ledger.jsonl")?;

    let peek_request = query_request(
        &live,
        "peek",
        &config_digest,
        generation,
        "audit-peek.jsonl",
        &[("party", &["party_id", "display_name"])],
        "confirmed",
        "none",
        "request",
        Some(party_visible),
    )?;
    let peek = run_query(&live, "query-peek", &peek_request, None)?;
    if peek.code != 0 {
        return Err(Stop::new(
            "query-peek",
            format!("exit {} stderr={}", peek.code, stderr_text(&peek.stderr)),
        ));
    }
    expect_query(
        &peek.stdout,
        &[(
            0,
            &[
                &[
                    CellExpect { kind: "int", value: Some("1") },
                    CellExpect { kind: "text", value: Some("甲公司") },
                ][..],
                &[
                    CellExpect { kind: "int", value: Some("2") },
                    CellExpect { kind: "text", value: Some("乙部") },
                ][..],
            ][..],
        )],
    )?;
    expect_audit(
        &live,
        "/evidence/live/audit-peek.jsonl",
        "oo_r_peek",
        &config_digest,
        &[party_visible],
    )?;

    let hidden = query_request(
        &live,
        "peek",
        &config_digest,
        generation,
        "audit-hidden.jsonl",
        &[("party", &["party_id", "display_name"])],
        "confirmed",
        "none",
        "request",
        Some("SELECT note FROM trade.party"),
    )?;
    let hidden_run = run_query(&live, "query-hidden", &hidden, None)?;
    if hidden_run.code != 6 {
        return Err(Stop::new(
            "query-hidden",
            format!("exit {} stderr={}", hidden_run.code, stderr_text(&hidden_run.stderr)),
        ));
    }
    if Path::new("/evidence/live/audit-hidden.jsonl").exists() {
        return Err(Stop::new("query-hidden", "guard refusal created an audit file"));
    }

    let write = query_request(
        &live,
        "clerk",
        &config_digest,
        generation,
        "audit-write.jsonl",
        &[("party", &["party_id", "display_name", "active", "note"])],
        "confirmed",
        "none",
        "request",
        Some("INSERT INTO trade.party (party_id, display_name, active) VALUES (8, 'x', true)"),
    )?;
    let write_run = run_query(&live, "query-write", &write, None)?;
    if write_run.code != 6 {
        return Err(Stop::new(
            "query-write",
            format!("exit {} stderr={}", write_run.code, stderr_text(&write_run.stderr)),
        ));
    }
    if Path::new("/evidence/live/audit-write.jsonl").exists() {
        return Err(Stop::new("query-write", "guard refusal created an audit file"));
    }

    let accepted = count_state(&reviewed_value["draft"], "accepted");
    let mut outcome = json!({
        "fixed_commit": "a661dcdf72ff91561ae6b7cb79916e582884d08a",
        "catalog_protocol": catalog_value["protocol"],
        "captured_protocol": captured_value["protocol"],
        "schema_hash": reviewed_value["wrapper"]["schema_hash"],
        "bare_draft_hash": bare_hash,
        "reviewed_draft_hash": reviewed_hash,
        "structural_reviews": reviews.as_array().map(Vec::len).unwrap_or(0),
        "accepted_queue_items": accepted,
        "publication": publication,
        "grants": grants,
        "live_catalog_review": live_review,
        "row_import": row_import,
        "pg_catalog_integration": integration,
        "role_pack_raw_exit": raw_pack.code,
        "role_pack_raw_stderr": stderr_text(&raw_pack.stderr),
        "role_pack_wrapped_exit": wrapped_pack.code,
        "role_pack_wrapped_stderr": stderr_text(&wrapped_pack.stderr),
        "installer_cli": "absent",
        "column_acl_cli": "absent",
        "import_state": import_value["state"],
        "import_run_id": import_value["run_id"],
        "source_generation": import_value["source_generation"],
        "mapping_run_generation": generation,
        "target_generation": import_value["target_generation"],
        "config_digest": config_digest,
        "query_draft_policy_exit": draft_query.code,
        "query_clerk_exit": clerk.code,
        "query_peek_exit": peek.code,
        "query_hidden_exit": hidden_run.code,
        "query_write_exit": write_run.code,
        "reader_source_writes": reader_writes,
        "writer_source_writes": writer_writes,
        "publications": publications,
        "operator_policy": "confirmed and pii none are operator labels, not draft fields",
        "semantic_publication": "missing",
        "authentication": "not_claimed",
        "harness_mount": "not_performed",
    });
    let acl_fields = json!({
        "column_acl_clerk_party": acl.clerk_party,
        "column_acl_clerk_ledger": acl.clerk_ledger,
        "column_acl_peek_party": acl.peek_party,
        "column_acl_peek_ledger": acl.peek_ledger,
        "column_acl_second_table": acl.clerk_ledger,
        "column_acl_ledger": "not_granted",
        "query_clerk_scope": "party only; ledger was not granted",
        "query_clerk_before_analyze_exit": before_analyze.code,
        "catalog_analyze": "admin ANALYZE of imported physical tables; import-pg does not analyze; reltuples was -1",
        "query_ledger_exit": ledger.code,
        "query_ledger_audit_present": Path::new("/evidence/live/audit-ledger.jsonl").exists(),
        "query_peek_scope": "party_id and display_name only",
    });
    let object = outcome
        .as_object_mut()
        .ok_or_else(|| Stop::new("outcome", "outcome was not an object"))?;
    if let Some(extra) = acl_fields.as_object() {
        for (key, value) in extra {
            object.insert(key.clone(), value.clone());
        }
    }
    write_json(&live, "outcome.json", &outcome)?;
    fs::write(live.evidence.join("outcome.txt"), "pg_cli_chain_ok\n").map_err(|err| disk("outcome", &err))?;
    println!("pg_cli_chain_ok");
    Ok(())
}

fn load_live() -> Result<Live, Stop> {
    let port: u16 = env_var("OO_A211_PORT")?
        .parse()
        .map_err(|_| Stop::new("env", "OO_A211_PORT"))?;
    let ca = fs::read(env_var("OO_A211_CA_FILE")?).map_err(|err| disk("ca", &err))?;
    Ok(Live {
        evidence: PathBuf::from(env_var("OO_A211_EVIDENCE")?),
        host: env_var("OO_A211_HOST")?,
        port,
        admin_user: env_var("OO_A211_ADMIN_USER")?,
        admin_password: read_hex(&env_var("OO_A211_ADMIN_PASSWORD_FILE")?)?,
        reader_password: read_hex(&env_var("OO_A211_READER_PASSWORD_FILE")?)?,
        writer_password: read_hex(&env_var("OO_A211_WRITER_PASSWORD_FILE")?)?,
        runtime_password: read_hex(&env_var("OO_A211_RUNTIME_PASSWORD_FILE")?)?,
        login_password: read_hex(&env_var("OO_A211_LOGIN_PASSWORD_FILE")?)?,
        ca,
        pipeline: PathBuf::from(env_var("OO_A211_PIPELINE")?),
        builder: PathBuf::from(env_var("OO_A211_BUILDER")?),
        query: PathBuf::from(env_var("OO_A211_QUERY")?),
    })
}

fn env_var(name: &str) -> Result<String, Stop> {
    env::var(name).map_err(|_| Stop::new("env", name))
}

fn read_hex(path: &str) -> Result<String, Stop> {
    let text = fs::read_to_string(path).map_err(|err| disk("secret_file", &err))?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(Stop::new("secret_file", "password file was rejected"));
    }
    Ok(trimmed)
}

async fn dial(live: &Live, user: &str, password: &str, database: &str) -> Result<PgConnection, Stop> {
    let options = PgConnectOptions::new()
        .host(&live.host)
        .port(live.port)
        .username(user)
        .password(password)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(live.ca.clone());
    let options = options.disable_statement_logging();
    PgConnection::connect_with(&options)
        .await
        .map_err(|_| Stop::new("dial", format!("user={user} db={database}")))
}

async fn reset_cluster(admin: &mut PgConnection) -> Result<(), Stop> {
    for database in [SOURCE_DB, TARGET_DB] {
        exec_sql(admin, &format!("DROP DATABASE IF EXISTS {database} WITH (FORCE)")).await?;
    }
    for role in [
        READER, WRITER, RUNTIME, LOGIN, "oo_r_clerk", "oo_r_peek", "oo_pipeline_runtime",
        "oo_target_owner",
    ] {
        let _ = exec_sql(admin, &format!("DROP ROLE IF EXISTS {role}")).await;
    }
    Ok(())
}

async fn create_roles(live: &Live, admin: &mut PgConnection) -> Result<(), Stop> {
    for (name, password, inherit) in [
        (READER, live.reader_password.as_str(), "INHERIT"),
        (WRITER, live.writer_password.as_str(), "INHERIT"),
        (RUNTIME, live.runtime_password.as_str(), "INHERIT"),
        (LOGIN, live.login_password.as_str(), "NOINHERIT"),
    ] {
        if !ident(name) {
            return Err(Stop::new("role", "role name was rejected"));
        }
        exec_sql(
            admin,
            &format!(
                "CREATE ROLE {name} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION {inherit}"
            ),
        )
        .await?;
    }
    Ok(())
}

async fn exec_sql(conn: &mut PgConnection, sql: &str) -> Result<(), Stop> {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
        .map_err(|err| Stop::new("sql", sqlstate(&err)))?;
    Ok(())
}

async fn scalar_text(conn: &mut PgConnection, sql: &str) -> Result<String, Stop> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| Stop::new("sql", sqlstate(&err)))
}

async fn scalar_bool(conn: &mut PgConnection, sql: &str) -> Result<bool, Stop> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| Stop::new("sql", sqlstate(&err)))
}

async fn expect_sqlstate(conn: &mut PgConnection, sql: &str, state: &str) -> Result<bool, Stop> {
    match sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
    {
        Ok(_) => Ok(false),
        Err(err) => Ok(sqlstate(&err) == state),
    }
}

async fn text_column(conn: &mut PgConnection, sql: &str) -> Result<Vec<Option<String>>, Stop> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .map_err(|err| Stop::new("sql", sqlstate(&err)))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(row.try_get(0).map_err(|_| Stop::new("sql", "text column"))?);
    }
    Ok(out)
}

fn source_object(live: &Live) -> Value {
    json!({
        "source_id": SOURCE_ID,
        "host": live.host,
        "port": live.port,
        "database": SOURCE_DB,
        "username": READER,
        "secret_ref": "secret://pg/a211/reader",
        "password_file": "/secrets/reader.pass",
        "ca_file": "/secrets/ca.crt",
        "tables": [
            {"schema": SCHEMA, "table": "party"},
            {"schema": SCHEMA, "table": "ledger"}
        ]
    })
}

fn source_request(live: &Live) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.catalog-pg/v1",
        "source": source_object(live)
    }))
    .expect("catalog request")
}

fn import_request(live: &Live) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.import-pg/v1",
        "source": source_object(live),
        "target": {
            "host": live.host,
            "port": live.port,
            "database": TARGET_DB,
            "runtime_principal": RUNTIME,
            "password_file": "/secrets/runtime.pass",
            "ca_file": "/secrets/ca.crt"
        },
        "shard": SHARD,
        "owner": OWNER,
        "source_generation": GENERATION
    }))
    .expect("import request")
}

fn structural_reviews(captured: &Value) -> Result<Value, Stop> {
    let schema_hash = captured["wrapper"]["schema_hash"]
        .as_str()
        .ok_or_else(|| Stop::new("captured-pg-draft", "schema_hash missing"))?;
    if schema_hash.len() != 64 {
        return Err(Stop::new("captured-pg-draft", "schema_hash length"));
    }
    let queue = captured["draft"]["review_queue"]
        .as_array()
        .ok_or_else(|| Stop::new("captured-pg-draft", "review_queue missing"))?;
    let mut reviews = Vec::new();
    for item in queue {
        let kind = item["kind"].as_str().unwrap_or("");
        let item_id = item["item_id"].as_str().unwrap_or("");
        if kind != "structural_column" && kind != "candidate_relationship" {
            continue;
        }
        if item_id.starts_with("pii/")
            || item_id.starts_with("semantic/")
            || item_id.starts_with("type/")
            || item_id.starts_with("classification/")
        {
            continue;
        }
        let fingerprint = item["fact_fingerprint"]
            .as_str()
            .ok_or_else(|| Stop::new("captured-pg-draft", "fact_fingerprint missing"))?;
        reviews.push(json!({
            "item_id": item_id,
            "action": "accept",
            "schema_hash": schema_hash,
            "fact_fingerprint": fingerprint
        }));
    }
    if reviews.is_empty() {
        return Err(Stop::new("captured-pg-draft", "no structural review items"));
    }
    Ok(Value::Array(reviews))
}

async fn compare_import(live: &Live, conn: &mut PgConnection, doc: &Value) -> Result<(), Stop> {
    let run_id = doc["run_id"]
        .as_str()
        .ok_or_else(|| Stop::new("import-pg", "run_id"))?
        .to_string();
    let state: String = sqlx::query_scalar(sqlx::AssertSqlSafe(
        "SELECT state FROM _oo.full_import_run WHERE run_id = $1::uuid".to_string(),
    ))
    .bind(&run_id)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| Stop::new("seal", sqlstate(&err)))?;
    let sealed: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(
        "SELECT sealed_at IS NOT NULL FROM _oo.full_import_run WHERE run_id = $1::uuid".to_string(),
    ))
    .bind(&run_id)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| Stop::new("seal", sqlstate(&err)))?;
    if state != "awaiting_publish" || !sealed {
        return Err(Stop::new("seal", format!("state={state} sealed={sealed}")));
    }
    let tables = doc["tables"]
        .as_array()
        .ok_or_else(|| Stop::new("import-pg", "tables"))?;
    check_table(conn, &run_id, &tables[0], "party", 2, &[
        ("party_id", &["1".into(), "2".into()]),
        ("display_name", &["甲公司".into(), "乙部".into()]),
        ("active", &["t".into(), "f".into()]),
        ("note", &[Value::Null, Value::String("备注".into())]),
    ]).await?;
    check_table(conn, &run_id, &tables[1], "ledger", 3, &[
        ("entry_id", &["10".into(), "11".into(), "12".into()]),
        ("party_id", &["1".into(), "1".into(), "2".into()]),
        ("amount", &["-9223372036854775808".into(), "9223372036854775807".into(), "0".into()]),
        ("memo", &[Value::String("下限".into()), Value::Null, Value::String("零".into())]),
    ]).await?;
    let _ = live;
    Ok(())
}

#[allow(clippy::cognitive_complexity)]
async fn check_table(
    conn: &mut PgConnection,
    run_id: &str,
    table: &Value,
    name: &str,
    rows: u64,
    columns: &[(&str, &[Value])],
) -> Result<(), Stop> {
    if table["schema"] != SCHEMA || table["name"] != name || table["rows"] != rows {
        return Err(Stop::new("import-pg", format!("table header {name}")));
    }
    let physical = table["physical_name"]
        .as_str()
        .ok_or_else(|| Stop::new("import-pg", "physical_name"))?;
    let digest = table["canonical_digest"]
        .as_str()
        .ok_or_else(|| Stop::new("import-pg", "canonical_digest"))?;
    if !physical_name(physical, "pt_") || digest.len() != 64 {
        return Err(Stop::new("import-pg", "physical name or digest"));
    }
    let stored: (String, i64) = sqlx::query_as(sqlx::AssertSqlSafe(
        "SELECT canonical_digest, applied_rows FROM _oo.full_import_table WHERE run_id = $1::uuid AND physical_name = $2"
            .to_string(),
    ))
    .bind(run_id)
    .bind(physical)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| Stop::new("receipt", sqlstate(&err)))?;
    if stored.0 != digest || u64::try_from(stored.1).unwrap_or(0) != rows {
        return Err(Stop::new("receipt", format!("digest or rows for {name}")));
    }
    let mapped: String = sqlx::query_scalar(sqlx::AssertSqlSafe(
        "SELECT physical_name FROM _oo.physical_mappings WHERE source = 'a211src' AND level = 't' AND logical_namespace = 'trade' AND logical_table = $1 AND logical_column = ''"
            .to_string(),
    ))
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| Stop::new("mapping", sqlstate(&err)))?;
    if mapped != physical {
        return Err(Stop::new("mapping", format!("table {name}")));
    }
    let listed = table["columns"]
        .as_array()
        .ok_or_else(|| Stop::new("import-pg", "columns"))?;
    for (logical, expect) in columns {
        let spec = listed
            .iter()
            .find(|item| item["name"] == *logical)
            .ok_or_else(|| Stop::new("import-pg", format!("missing column {logical}")))?;
        let column_physical = spec["physical_name"]
            .as_str()
            .ok_or_else(|| Stop::new("import-pg", "column physical"))?;
        if !physical_name(column_physical, "pc_") {
            return Err(Stop::new("import-pg", "column physical shape"));
        }
        let stored_column: String = sqlx::query_scalar(sqlx::AssertSqlSafe(
            "SELECT physical_name FROM _oo.physical_mappings WHERE source = 'a211src' AND level = 'c' AND logical_namespace = 'trade' AND logical_table = $1 AND logical_column = $2"
                .to_string(),
        ))
        .bind(name)
        .bind(*logical)
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| Stop::new("mapping", sqlstate(&err)))?;
        if stored_column != column_physical {
            return Err(Stop::new("mapping", format!("column {name}.{logical}")));
        }
        let got = text_column(
            conn,
            &format!("SELECT \"{column_physical}\"::text FROM _oo.\"{physical}\" ORDER BY \"_oo_pos\""),
        )
        .await?;
        let expect_text: Vec<Option<String>> = expect
            .iter()
            .map(|value| value.as_str().map(str::to_string))
            .collect();
        let normalized: Vec<Option<String>> = got.into_iter().map(|value| value.map(|text| norm_bool(&text))).collect();
        let expect_norm: Vec<Option<String>> = expect_text.into_iter().map(|value| value.map(|text| norm_bool(&text))).collect();
        if normalized != expect_norm {
            return Err(Stop::new("rows", format!("shadow {name}.{logical}")));
        }
    }
    Ok(())
}

async fn mapping_identity(conn: &mut PgConnection) -> Result<(String, i64), Stop> {
    let rows: Vec<(String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(
        "SELECT DISTINCT config_digest, run_generation FROM _oo.physical_mappings WHERE source = 'a211src'"
            .to_string(),
    ))
    .fetch_all(&mut *conn)
    .await
    .map_err(|err| Stop::new("mapping", sqlstate(&err)))?;
    if rows.len() != 1 || rows[0].0.len() != 64 {
        return Err(Stop::new("mapping", "config digest was not one value"));
    }
    Ok((rows[0].0.clone(), rows[0].1))
}

#[allow(clippy::too_many_arguments)]
fn query_request(
    live: &Live,
    role: &str,
    digest: &str,
    generation: i64,
    audit_name: &str,
    tables: &[(&str, &[&str])],
    status: &str,
    pii: &str,
    source: &str,
    sql: Option<&str>,
) -> Result<Value, Stop> {
    let policies: Vec<Value> = tables
        .iter()
        .map(|(table, columns)| {
            json!({
                "project": SOURCE_ID,
                "config_digest": digest,
                "generation": generation,
                "role": role,
                "login": LOGIN,
                "table": {
                    "namespace": SCHEMA,
                    "name": table,
                    "columns": columns.iter().map(|name| json!({
                        "name": name,
                        "status": status,
                        "semantic_type": "operator_label",
                        "pii": {"class": pii}
                    })).collect::<Vec<_>>()
                }
            })
        })
        .collect();
    let mut statement = json!({
        "source": source,
        "max_rows": 20,
        "max_result_bytes": 8192,
        "wall_clock_ms": 30000
    });
    if let Some(text) = sql {
        statement["sql"] = json!(text);
    }
    Ok(json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": {
            "host": live.host,
            "port": live.port,
            "database": TARGET_DB,
            "user": LOGIN
        },
        "password_file": "/secrets/login.pass",
        "ca_file": "/secrets/ca.crt",
        "audit_file": format!("/evidence/live/{audit_name}"),
        "identity": {
            "project": SOURCE_ID,
            "role": format!("oo_r_{role}"),
            "policy_id": digest
        },
        "policies": policies,
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1_000_000,
            "unknown_rows": "refuse"
        },
        "statement": statement
    }))
}

struct AclNote {
    clerk_party: String,
    clerk_ledger: String,
    peek_party: String,
    peek_ledger: String,
}

fn column_policy(digest: &str, generation: i64, role: &str, table: &str, columns: &[&str]) -> ColumnAclPolicy {
    ColumnAclPolicy {
        project: SOURCE_ID.to_string(),
        config_digest: digest.to_string(),
        generation,
        role: role.to_string(),
        login: LOGIN.to_string(),
        table: LogicalTable {
            namespace: SCHEMA.to_string(),
            name: table.to_string(),
            columns: columns
                .iter()
                .map(|name| ColumnSpec {
                    name: (*name).to_string(),
                    status: ClassStatus::Confirmed,
                    semantic_type: "operator_label".to_string(),
                    pii: PiiClass::None,
                })
                .collect(),
        },
        row_filter: None,
    }
}

fn write_acl_note(live: &Live, role: &str, table: &str, text: &str) -> Result<(), Stop> {
    fs::write(
        live.evidence.join(format!("column-acl-{role}-{table}.txt")),
        format!("{text}\n"),
    )
    .map_err(|err| disk("install-column-acl", &err))
}

async fn analyze_imported(live: &Live, doc: &Value) -> Result<(), Stop> {
    let tables = doc["tables"]
        .as_array()
        .ok_or_else(|| Stop::new("analyze", "tables"))?;
    if tables.len() != 2 {
        return Err(Stop::new("analyze", "table count"));
    }
    let mut admin = dial(live, &live.admin_user, &live.admin_password, TARGET_DB).await?;
    let mut before_lines = Vec::new();
    let mut after_lines = Vec::new();
    for table in tables {
        let physical = table["physical_name"].as_str().unwrap_or("");
        if !physical_name(physical, "pt_") {
            return Err(Stop::new("analyze", "physical name"));
        }
        let before = reltuples(&mut admin, physical).await?;
        if before != "-1" {
            return Err(Stop::new("analyze", format!("reltuples before {before}")));
        }
        exec_sql(&mut admin, &format!("ANALYZE _oo.\"{physical}\"")).await?;
        let after = reltuples(&mut admin, physical).await?;
        if after == "-1" || after.starts_with('-') {
            return Err(Stop::new("analyze", format!("reltuples after {after}")));
        }
        before_lines.push(format!("{physical} {before}"));
        after_lines.push(format!("{physical} {after}"));
    }
    drop(admin);
    fs::write(
        live.evidence.join("reltuples-before.txt"),
        before_lines.join("\n") + "\n",
    )
    .map_err(|err| disk("analyze", &err))?;
    fs::write(
        live.evidence.join("reltuples-after.txt"),
        after_lines.join("\n") + "\n",
    )
    .map_err(|err| disk("analyze", &err))?;
    Ok(())
}

async fn reltuples(conn: &mut PgConnection, physical: &str) -> Result<String, Stop> {
    scalar_text(
        conn,
        &format!(
            "SELECT c.reltuples::text FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             WHERE n.nspname = '_oo' AND c.relname = '{physical}'"
        ),
    )
    .await
}

async fn install_acls(live: &Live, digest: &str, generation: i64) -> Result<AclNote, Stop> {
    let (mut client, connection) = admin_client(live).await?;
    let handle = tokio::spawn(async move {
        let _ = connection.await;
    });
    let clerk_party = install_first(
        &mut client,
        live,
        digest,
        generation,
        "clerk",
        "party",
        &["party_id", "display_name", "active", "note"],
    )
    .await?;
    let clerk_ledger = install_second_refused(
        &mut client,
        live,
        digest,
        generation,
        "clerk",
        "ledger",
        &["entry_id", "party_id", "amount", "memo"],
    )
    .await?;
    let peek_party = install_first(
        &mut client,
        live,
        digest,
        generation,
        "peek",
        "party",
        &["party_id", "display_name"],
    )
    .await?;
    let peek_ledger = install_second_refused(
        &mut client,
        live,
        digest,
        generation,
        "peek",
        "ledger",
        &["entry_id", "amount"],
    )
    .await?;
    drop(client);
    let _ = handle.await;
    if clerk_ledger != peek_ledger {
        return Err(Stop::new(
            "install-column-acl",
            "the two second-table refusals differ",
        ));
    }
    Ok(AclNote {
        clerk_party,
        clerk_ledger,
        peek_party,
        peek_ledger,
    })
}

async fn install_first(
    client: &mut Client,
    live: &Live,
    digest: &str,
    generation: i64,
    role: &str,
    table: &str,
    columns: &[&str],
) -> Result<String, Stop> {
    let policy = column_policy(digest, generation, role, table, columns);
    match install_column_acl(client, &policy).await {
        Ok(outcome) if outcome.effect == InstallEffect::Installed => {
            let text = "installed".to_string();
            write_acl_note(live, role, table, &text)?;
            Ok(text)
        }
        Ok(_) => Err(Stop::new(
            "install-column-acl",
            format!("{role} {table} first table was a replay"),
        )),
        Err(err) => Err(Stop::new(
            "install-column-acl",
            format!("{role} {table} {}", scrub(&err.to_string(), live)),
        )),
    }
}

async fn install_second_refused(
    client: &mut Client,
    live: &Live,
    digest: &str,
    generation: i64,
    role: &str,
    table: &str,
    columns: &[&str],
) -> Result<String, Stop> {
    let policy = column_policy(digest, generation, role, table, columns);
    match install_column_acl(client, &policy).await {
        Ok(_) => Err(Stop::new(
            "install-column-acl",
            format!("{role} {table} second table was granted"),
        )),
        Err(err) => {
            let text = scrub(&err.to_string(), live);
            if text != "existing column grants do not match" {
                return Err(Stop::new("install-column-acl", format!("{role} {table} {text}")));
            }
            write_acl_note(live, role, table, &text)?;
            Ok(text)
        }
    }
}

async fn admin_client(live: &Live) -> Result<(Client, TlsConnection), Stop> {
    let endpoint = TlsEndpoint {
        host: live.host.clone(),
        port: live.port,
        database: TARGET_DB.to_string(),
        user: live.admin_user.clone(),
    };
    connect_require_tls(&endpoint, &live.admin_password, &live.ca)
        .await
        .map_err(|_| Stop::new("dial", "admin tls"))
}

async fn login_client(live: &Live) -> Result<(Client, TlsConnection), Stop> {
    let endpoint = TlsEndpoint {
        host: live.host.clone(),
        port: live.port,
        database: TARGET_DB.to_string(),
        user: LOGIN.to_string(),
    };
    connect_require_tls(&endpoint, &live.login_password, &live.ca)
        .await
        .map_err(|_| Stop::new("dial", "login tls"))
}

async fn prove_direct_acl(live: &Live, doc: &Value) -> Result<(), Stop> {
    let party = doc["tables"]
        .as_array()
        .and_then(|tables| tables.iter().find(|table| table["name"] == "party"))
        .ok_or_else(|| Stop::new("acl", "party table"))?;
    let physical = party["physical_name"].as_str().unwrap_or("");
    let note = party["columns"]
        .as_array()
        .and_then(|columns| columns.iter().find(|column| column["name"] == "note"))
        .and_then(|column| column["physical_name"].as_str())
        .unwrap_or("");
    let display = party["columns"]
        .as_array()
        .and_then(|columns| columns.iter().find(|column| column["name"] == "display_name"))
        .and_then(|column| column["physical_name"].as_str())
        .unwrap_or("");
    let party_id = party["columns"]
        .as_array()
        .and_then(|columns| columns.iter().find(|column| column["name"] == "party_id"))
        .and_then(|column| column["physical_name"].as_str())
        .unwrap_or("");
    if !physical_name(physical, "pt_")
        || !physical_name(note, "pc_")
        || !physical_name(display, "pc_")
        || !physical_name(party_id, "pc_")
    {
        return Err(Stop::new("acl", "physical names"));
    }
    let (client, connection) = login_client(live).await?;
    let handle = tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .batch_execute("SET ROLE oo_r_peek")
        .await
        .map_err(|_| Stop::new("acl", "set role peek"))?;
    let hidden = client
        .batch_execute(&format!("SELECT \"{note}\" FROM _oo.\"{physical}\""))
        .await;
    let hidden_state = match hidden {
        Ok(_) => String::new(),
        Err(err) => err.code().map(|code| code.code().to_string()).unwrap_or_default(),
    };
    if hidden_state != "42501" {
        return Err(Stop::new("acl", format!("peek hidden sqlstate={hidden_state}")));
    }
    let visible = client
        .query(
            &format!("SELECT \"{display}\"::text FROM _oo.\"{physical}\" ORDER BY \"{party_id}\""),
            &[],
        )
        .await
        .map_err(|err| {
            let state = err
                .code()
                .map(|code| code.code().to_string())
                .unwrap_or_else(|| "none".to_string());
            Stop::new("acl", format!("peek visible select sqlstate={state}"))
        })?;
    let texts: Vec<String> = visible.iter().map(|row| row.get(0)).collect();
    if texts != ["甲公司", "乙部"] {
        return Err(Stop::new("acl", "peek visible values"));
    }
    client
        .batch_execute("SET ROLE oo_r_clerk")
        .await
        .map_err(|_| Stop::new("acl", "set role clerk"))?;
    let write = client
        .batch_execute(&format!("INSERT INTO _oo.\"{physical}\" (\"{display}\") VALUES ('x')"))
        .await;
    let write_state = match write {
        Ok(_) => String::new(),
        Err(err) => err.code().map(|code| code.code().to_string()).unwrap_or_default(),
    };
    if write_state != "42501" {
        return Err(Stop::new("acl", format!("clerk insert sqlstate={write_state}")));
    }
    drop(client);
    let _ = handle.await;
    Ok(())
}

fn run_query(live: &Live, name: &str, request: &Value, stdin: Option<&[u8]>) -> Result<CmdOut, Stop> {
    let path = live.evidence.join(format!("{name}.request.json"));
    let bytes = serde_json::to_vec(request).map_err(|_| Stop::new(name, "request encode"))?;
    fs::write(&path, &bytes).map_err(|err| disk(name, &err))?;
    let output = run_cmd(&live.query, &["--request", &path.to_string_lossy()], stdin.unwrap_or(b""), 120)?;
    save_cmd(live, name, &output)?;
    Ok(output)
}

fn expect_refusal(live: &Live, name: &str, output: &CmdOut, audit: &str) -> Result<(), Stop> {
    if output.code == 0 {
        return Err(Stop::new(name, "exit 0"));
    }
    if let Ok(lines) = json_lines(&output.stdout) {
        for line in &lines {
            if line["record"] == "row" || line["status"] == "succeeded" {
                return Err(Stop::new(name, "refusal printed a successful row"));
            }
        }
    }
    let path = Path::new(audit);
    if output.code == 5 || output.code == 6 {
        if !output.stdout.is_empty() {
            return Err(Stop::new(name, format!("exit {} wrote stdout", output.code)));
        }
        if path.exists() {
            return Err(Stop::new(name, "policy or guard refusal created an audit file"));
        }
        return Ok(());
    }
    if path.exists() {
        let raw = fs::read(path).map_err(|err| disk(name, &err))?;
        if raw
            .windows(live.login_password.len())
            .any(|window| window == live.login_password.as_bytes())
        {
            return Err(Stop::new(name, "audit stored a password"));
        }
        if raw.windows(b"\"succeeded\"".len()).any(|window| window == b"\"succeeded\"") {
            return Err(Stop::new(name, "audit claimed success"));
        }
    }
    Ok(())
}

fn expect_query(stdout: &[u8], turns: &[(u32, &[&[CellExpect]])]) -> Result<(), Stop> {
    let lines = json_lines(stdout)?;
    for (turn, rows) in turns {
        let got: Vec<&Value> = lines
            .iter()
            .filter(|line| line["record"] == "row" && line["turn"] == *turn)
            .collect();
        if got.len() != rows.len() {
            return Err(Stop::new("query-rows", format!("turn {turn} row count")));
        }
        for (index, expect) in rows.iter().enumerate() {
            let cells = got[index]["cells"]
                .as_array()
                .ok_or_else(|| Stop::new("query-rows", "cells"))?;
            if cells.len() != expect.len() {
                return Err(Stop::new("query-rows", format!("turn {turn} width")));
            }
            for (cell, wanted) in cells.iter().zip(expect.iter()) {
                if !cell_matches(cell, wanted) {
                    return Err(Stop::new("query-rows", format!("turn {turn} cell {index}")));
                }
            }
        }
        let summary = lines
            .iter()
            .find(|line| line["record"] == "summary" && line["turn"] == *turn)
            .ok_or_else(|| Stop::new("query-rows", "summary"))?;
        if summary["status"] != "succeeded"
            || summary["truncated"] != false
            || summary["end_audit_ok"] != true
            || summary["returned_rows"] != rows.len()
        {
            return Err(Stop::new("query-rows", format!("turn {turn} summary")));
        }
    }
    Ok(())
}

fn cell_matches(cell: &Value, expect: &CellExpect) -> bool {
    if cell["kind"] != expect.kind {
        return false;
    }
    match expect.kind {
        "null" => cell.get("value").is_none() || cell["value"].is_null(),
        "bool" => {
            let text = match cell["value"].as_bool() {
                Some(true) => "true",
                Some(false) => "false",
                None => return false,
            };
            Some(text) == expect.value
        }
        "int" | "text" => cell["value"].as_str() == expect.value,
        _ => false,
    }
}

fn expect_audit(live: &Live, path: &str, role: &str, digest: &str, sqls: &[&str]) -> Result<(), Stop> {
    let raw = fs::read(path).map_err(|err| disk("audit", &err))?;
    if raw.windows(b"SELECT".len()).any(|window| window == b"SELECT") {
        return Err(Stop::new("audit", "SQL text was stored"));
    }
    if raw.windows(live.login_password.len()).any(|window| window == live.login_password.as_bytes()) {
        return Err(Stop::new("audit", "password was stored"));
    }
    let lines = json_lines(&raw)?;
    if lines.len() != sqls.len() * 2 {
        return Err(Stop::new("audit", "record count"));
    }
    for (index, sql) in sqls.iter().enumerate() {
        let admit = &lines[index * 2];
        let complete = &lines[index * 2 + 1];
        if admit["record"] != "admit" || complete["record"] != "complete" {
            return Err(Stop::new("audit", "admit did not precede complete"));
        }
        if admit["protocol"] != "oo.gateway.query-pg-audit/v1"
            || admit["role"] != role
            || admit["project"] != SOURCE_ID
            || admit["policy_id"] != digest
        {
            return Err(Stop::new("audit", "identity"));
        }
        let hash = sha256_hex(sql.as_bytes());
        if admit["original_sha256"] != hash || complete["original_sha256"] != hash {
            return Err(Stop::new("audit", "original hash"));
        }
        if complete["status"] != "succeeded" {
            return Err(Stop::new("audit", "completion status"));
        }
    }
    Ok(())
}

fn json_lines(bytes: &[u8]) -> Result<Vec<Value>, Stop> {
    let text = std::str::from_utf8(bytes).map_err(|_| Stop::new("json", "utf8"))?;
    let mut out = Vec::new();
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        out.push(serde_json::from_str(line).map_err(|_| Stop::new("json", "line"))?);
    }
    Ok(out)
}

fn count_state(draft: &Value, state: &str) -> usize {
    draft["review_queue"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|item| item["state"] == state)
                .count()
        })
        .unwrap_or(0)
}

fn run_cmd(bin: &Path, args: &[&str], stdin_bytes: &[u8], timeout_secs: u64) -> Result<CmdOut, Stop> {
    let mut child = Command::new(bin)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
        .env("HOME", "/tmp")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Stop::new("spawn", "command did not start"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| Stop::new("spawn", "stdin"))?;
    stdin
        .write_all(stdin_bytes)
        .map_err(|_| Stop::new("spawn", "stdin write"))?;
    drop(stdin);
    let mut stdout = child.stdout.take().ok_or_else(|| Stop::new("spawn", "stdout"))?;
    let mut stderr = child.stderr.take().ok_or_else(|| Stop::new("spawn", "stderr"))?;
    let stdout_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let stderr_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > Duration::from_secs(timeout_secs) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Stop::new("spawn", "command timed out"));
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => return Err(Stop::new("spawn", "wait failed")),
        }
    };
    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();
    Ok(CmdOut {
        code: status.code().unwrap_or(9),
        stdout,
        stderr,
    })
}

fn save_cmd(live: &Live, name: &str, output: &CmdOut) -> Result<(), Stop> {
    let dir = live.evidence.join("steps");
    fs::create_dir_all(&dir).map_err(|err| disk(name, &err))?;
    let stdout = scrub_bytes(&output.stdout, live)?;
    let stderr = scrub_bytes(&output.stderr, live)?;
    fs::write(dir.join(format!("{name}.stdout")), stdout).map_err(|err| disk(name, &err))?;
    fs::write(dir.join(format!("{name}.stderr")), stderr).map_err(|err| disk(name, &err))?;
    fs::write(dir.join(format!("{name}.exit")), format!("{}\n", output.code)).map_err(|err| disk(name, &err))?;
    Ok(())
}

fn scrub_bytes(bytes: &[u8], live: &Live) -> Result<Vec<u8>, Stop> {
    let secrets = [
        live.admin_password.as_str(),
        live.reader_password.as_str(),
        live.writer_password.as_str(),
        live.runtime_password.as_str(),
        live.login_password.as_str(),
    ];
    for secret in secrets {
        if !secret.is_empty() && bytes.windows(secret.len()).any(|window| window == secret.as_bytes()) {
            return Err(Stop::new("redaction", "command output contained a password"));
        }
    }
    Ok(bytes.to_vec())
}

fn scrub(text: &str, live: &Live) -> String {
    let mut out = text.to_string();
    for secret in [
        live.admin_password.as_str(),
        live.reader_password.as_str(),
        live.writer_password.as_str(),
        live.runtime_password.as_str(),
        live.login_password.as_str(),
    ] {
        if !secret.is_empty() {
            out = out.replace(secret, "[redacted]");
        }
    }
    out
}

fn write_json(live: &Live, name: &str, value: &Value) -> Result<(), Stop> {
    let text = serde_json::to_string_pretty(value).map_err(|_| Stop::new("json", name))?;
    let clean = scrub(&text, live);
    fs::write(live.evidence.join(name), clean + "\n").map_err(|err| disk(name, &err))?;
    Ok(())
}

fn stderr_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).chars().take(400).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sqlstate(err: &sqlx::Error) -> String {
    match err {
        sqlx::Error::Database(db) => db.code().map(|code| code.to_string()).unwrap_or_else(|| "db".to_string()),
        _ => "transport".to_string(),
    }
}

fn physical_name(name: &str, prefix: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    rest.len() == 60 && rest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn norm_bool(text: &str) -> String {
    match text {
        "true" => "t".to_string(),
        "false" => "f".to_string(),
        other => other.to_string(),
    }
}

fn disk(step: &str, err: &impl std::fmt::Display) -> Stop {
    Stop::new(step, err.to_string())
}

impl Stop {
    fn new(step: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            step: step.into(),
            detail: detail.into(),
        }
    }
}

const FIXTURE_SQL: &str = "\
CREATE SCHEMA trade;\n\
CREATE TABLE trade.party (\n\
  party_id int8 PRIMARY KEY,\n\
  display_name text,\n\
  active bool NOT NULL,\n\
  note text\n\
);\n\
CREATE TABLE trade.ledger (\n\
  entry_id int8 PRIMARY KEY,\n\
  party_id int8 NOT NULL REFERENCES trade.party (party_id),\n\
  amount int8 NOT NULL,\n\
  memo text\n\
);\n\
COMMENT ON TABLE trade.party IS '甲注评';\n\
COMMENT ON COLUMN trade.party.display_name IS '甲注列';\n\
CREATE TABLE trade.write_audit (who text NOT NULL);\n\
CREATE FUNCTION trade.note_write() RETURNS trigger LANGUAGE plpgsql AS $audit$\n\
BEGIN\n\
  INSERT INTO trade.write_audit (who) VALUES (current_user);\n\
  RETURN NEW;\n\
END;\n\
$audit$;\n\
CREATE TRIGGER party_audit BEFORE INSERT OR UPDATE OR DELETE ON trade.party\n\
  FOR EACH ROW EXECUTE FUNCTION trade.note_write();\n\
CREATE TRIGGER ledger_audit BEFORE INSERT OR UPDATE OR DELETE ON trade.ledger\n\
  FOR EACH ROW EXECUTE FUNCTION trade.note_write();\n\
INSERT INTO trade.party (party_id, display_name, active, note) VALUES\n\
  (1, '甲公司', true, NULL),\n\
  (2, '乙部', false, '备注');\n\
INSERT INTO trade.ledger (entry_id, party_id, amount, memo) VALUES\n\
  (10, 1, -9223372036854775808, '下限'),\n\
  (11, 1, 9223372036854775807, NULL),\n\
  (12, 2, 0, '零');\n\
GRANT USAGE ON SCHEMA trade TO oo_a211_reader, oo_a211_writer;\n\
GRANT SELECT ON trade.party, trade.ledger TO oo_a211_reader;\n\
GRANT INSERT ON trade.party, trade.ledger, trade.write_audit TO oo_a211_writer;\n\
";
