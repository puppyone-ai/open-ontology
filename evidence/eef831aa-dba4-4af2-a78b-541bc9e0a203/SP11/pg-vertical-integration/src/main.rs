//! API-only PostgreSQL vertical composition at fixed base 0ee7cf86.
//! This binary is evidence. It does not publish a shadow and it does not edit product code.

#![forbid(unsafe_code)]

mod audit;
mod drafts;
mod fixture;
mod stop;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use audit::DbAudit;
use drafts::{compile_passes, queue_snapshot, role_pack_attempts, visibility_for_draft};
use fixture::{
    semantic_type, Expect, HIDDEN_INVOICE, HIDDEN_PARTY, INVOICE_ALL, INVOICE_ROWS, INVOICE_VISIBLE,
    LOGIN_INVOICE, LOGIN_PARTY, OWNER, PARTY_ALL, PARTY_ROWS, PARTY_VISIBLE, READER, ROLE_INVOICE,
    ROLE_PARTY, RUNTIME, SCHEMA_SQL, SECRET_REF, SHARD, SLUG_INVOICE, SLUG_PARTY, SOURCE_DB,
    SOURCE_ID, TARGET_DB,
};
use oo_connectors::{
    PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, PgTypePolicy,
    SecretRef, SelectedTable, SourceId, StreamBudget, TableSelection,
};
use oo_gateway::policy::catalog::{load_policy_catalog, CatalogLimits, UnknownRows};
use oo_gateway::query_executor::{Cell, DedicatedSession, ExecError, SelectRequest};
use oo_gateway::{
    compile_column_acl, connect_require_tls, install_column_acl, rustls_connector, ClassStatus,
    ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass, PolicyIdentity, TlsEndpoint,
};
use oo_pipeline::copy_encoder::CopyBudgets;
use oo_pipeline::full_import::{import_unpublished, ImportRequest, RunState, RunSummary};
use oo_pipeline::target_installer::{install, install_full_import, FullImportInstallOutcome, InstallOutcome};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{Connection, Row};
use stop::{Stop, FIXED_BASE};
use tokio::task::JoinHandle;
use tokio_postgres::Client;

struct Live {
    evidence: PathBuf,
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    reader_password: String,
    runtime_password: String,
    party_password: String,
    invoice_password: String,
    ca: Vec<u8>,
}

struct MapRow {
    level: String,
    namespace: String,
    table: String,
    column: String,
    physical: String,
    digest: String,
    generation: i64,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let code = match run().await {
        Ok(()) => 0,
        Err(stop) => {
            let dir = env::var("OO_A203_EVIDENCE").unwrap_or_else(|_| "/evidence".to_string());
            let live = Path::new(&dir).join("live");
            let _ = fs::create_dir_all(&live);
            let body = json!({
                "step": stop.step,
                "input": stop.input,
                "expected": stop.expected,
                "observed": stop.observed,
                "source_sha": FIXED_BASE,
                "owned_path_needed": stop.owned_path_needed,
            });
            let _ = fs::write(live.join("blocker.json"), serde_json::to_string_pretty(&body).unwrap_or_default());
            eprintln!("a203 stop: {stop}");
            1
        }
    };
    std::process::exit(code);
}

async fn run() -> Result<(), Stop> {
    let live = load_live()?;
    fs::create_dir_all(live.evidence.join("live")).map_err(|err| disk("mkdir", &err))?;
    write_text(&live, "source.sql", &SCHEMA_SQL.join(";\n"))?;
    let mut admin = dial(&live, &live.admin_user, &live.admin_password, "postgres").await?;
    reset_cluster(&live, &mut admin).await?;
    create_roles(&live, &mut admin).await?;
    exec_ok(&mut admin, &format!("CREATE DATABASE {SOURCE_DB} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'")).await?;
    exec_ok(&mut admin, &format!("CREATE DATABASE {TARGET_DB} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'")).await?;
    exec_ok(&mut admin, &format!("GRANT CONNECT ON DATABASE {SOURCE_DB} TO {READER}")).await?;
    exec_ok(
        &mut admin,
        &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN_PARTY}, {LOGIN_INVOICE}"),
    )
    .await?;
    drop(admin);

    let mut source_admin = dial(&live, &live.admin_user, &live.admin_password, SOURCE_DB).await?;
    for statement in SCHEMA_SQL {
        exec_ok(&mut source_admin, statement).await?;
    }
    drop(source_admin);

    prove_reader_readonly(&live).await?;
    let (catalog_json, config_hex, snapshot) = capture_and_prepare(&live).await?;
    write_text(&live, "catalog.json", &catalog_json)?;
    let mut target = dial(&live, &live.admin_user, &live.admin_password, TARGET_DB).await?;
    let install_effect = install(&mut target).await.map_err(|err| {
        Stop::new("install", "target admin", "Bootstrapped", err.to_string(), "none")
    })?;
    if install_effect != InstallOutcome::Bootstrapped {
        return Err(Stop::new(
            "install",
            "fresh target",
            "Bootstrapped",
            install_effect.to_string(),
            "none",
        ));
    }
    let full_effect = install_full_import(&mut target).await.map_err(|err| {
        Stop::new("install_full_import", "target admin", "Installed", err.to_string(), "none")
    })?;
    if full_effect != FullImportInstallOutcome::Installed {
        return Err(Stop::new(
            "install_full_import",
            "fresh target",
            "Installed",
            full_effect.to_string(),
            "none",
        ));
    }
    exec_ok(&mut target, &format!("GRANT {RUNTIME_ROLE} TO {RUNTIME}")).await?;
    drop(target);

    let summary = import_rows(&live, snapshot).await?;
    let mut target = dial(&live, &live.admin_user, &live.admin_password, TARGET_DB).await?;
    let mappings = read_mappings(&mut target).await?;
    prove_seal(&mut target, &summary, &mappings, &config_hex).await?;
    let passes = compile_passes(&catalog_json)?;
    write_drafts(&live, &passes)?;
    check_reviewed_queue(&passes.reviewed.draft)?;
    let (attempts, envelope, datasets_only) = role_pack_attempts(&passes.reviewed.draft)?;
    let visibility = visibility_for_draft(&passes.reviewed.draft)?;
    write_text(&live, "rolepack-visibility.json", &visibility)?;
    write_text(&live, "rolepack-envelope.json", &envelope)?;
    if !datasets_only.is_empty() {
        write_text(&live, "rolepack-datasets-only.json", &datasets_only)?;
    }
    let pack_notes = pack_notes(&attempts);
    if attempts.iter().any(|item| item.compiled) {
        write_json(&live, "rolepack-attempts.json", &pack_notes)?;
        return Err(Stop::new(
            "role_pack",
            "compiler datasets with null pii",
            "classification refusal",
            "compile_role_pack returned a pack",
            "do not treat this pack as classified; production code was not changed",
        ));
    }
    write_json(&live, "rolepack-attempts.json", &pack_notes)?;

    exec_ok(&mut target, AUDIT_SQL).await?;
    let policies = install_acls(&live, &catalog_json, &mappings).await?;
    let query_notes = run_role_queries(&live, &policies).await?;
    prove_direct_denies(&live, &mappings).await?;
    compare_stores(&live, &mut target, &catalog_json, &mappings).await?;
    prove_seal(&mut target, &summary, &mappings, &config_hex).await?;
    let outcome = json!({
        "status": "api_slice_completed",
        "source_sha": FIXED_BASE,
        "install": install_effect.to_string(),
        "install_full_import": full_effect.to_string(),
        "install_calls": 1,
        "full_import_calls": 1,
        "run_id": summary.run_id,
        "state": "awaiting_publish",
        "source_id": summary.source_id,
        "source_generation": summary.source_generation,
        "target_generation": summary.target_generation,
        "config_digest": config_hex,
        "tables": summary.tables.iter().map(|table| json!({
            "schema": table.schema,
            "name": table.name,
            "physical_name": table.physical_name,
            "rows": table.rows,
            "canonical_digest": table.canonical_digest,
            "columns": table.columns.iter().map(|column| json!({
                "name": column.name,
                "attnum": column.attnum,
                "physical_name": column.physical_name,
                "type_oid": column.type_oid,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "oracle_sha256": oracle_sha256(),
        "schema_hash": passes.reviewed.schema_hash,
        "captured_catalog_hash": passes.reviewed.captured_catalog_hash,
        "draft_hash": passes.reviewed.draft_hash,
        "wrapper_hash": passes.reviewed.wrapper_hash,
        "manifest_hash": passes.reviewed.manifest_hash,
        "label_attempt": passes.label_attempt,
        "review_note": passes.review_note,
        "role_pack": pack_notes,
        "queries": query_notes,
        "publication": "unavailable",
        "shadow_marked_published": false,
        "query_cli": "not_exercised",
        "parent_acceptance": false,
    });
    write_json(&live, "outcome.json", &outcome)?;
    Ok(())
}

const RUNTIME_ROLE: &str = "oo_pipeline_runtime";

const AUDIT_SQL: &str = "CREATE TABLE public.a203_audit (
    audit_id int8 GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    phase text NOT NULL,
    original_sha256 text NOT NULL,
    executed_sha256 text NOT NULL,
    fingerprint_hex text NOT NULL,
    role_name text NOT NULL,
    project text NOT NULL,
    policy_id text NOT NULL,
    backend_pid int4 NOT NULL,
    backend_start text NOT NULL,
    status text,
    returned_rows int4,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp()
)";

fn load_live() -> Result<Live, Stop> {
    let port: u16 = env_var("OO_A203_PORT")?
        .parse()
        .map_err(|_| Stop::new("env", "OO_A203_PORT", "u16", "rejected", "none"))?;
    let ca_path = env_var("OO_A203_CA_FILE")?;
    let ca = fs::read(&ca_path).map_err(|err| disk("ca", &err))?;
    Ok(Live {
        evidence: PathBuf::from(env_var("OO_A203_EVIDENCE")?),
        host: env_var("OO_A203_HOST")?,
        port,
        admin_user: env_var("OO_A203_ADMIN_USER")?,
        admin_password: read_hex(&env_var("OO_A203_ADMIN_PASSWORD_FILE")?)?,
        reader_password: read_hex(&env_var("OO_A203_READER_PASSWORD_FILE")?)?,
        runtime_password: read_hex(&env_var("OO_A203_RUNTIME_PASSWORD_FILE")?)?,
        party_password: read_hex(&env_var("OO_A203_PARTY_PASSWORD_FILE")?)?,
        invoice_password: read_hex(&env_var("OO_A203_INVOICE_PASSWORD_FILE")?)?,
        ca,
    })
}

fn env_var(name: &str) -> Result<String, Stop> {
    env::var(name).map_err(|_| Stop::new("env", name, "set", "missing", "none"))
}

fn read_hex(path: &str) -> Result<String, Stop> {
    let text = fs::read_to_string(path).map_err(|err| disk("secret_file", &err))?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(Stop::new("secret_file", "password file", "hex", "rejected", "none"));
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
    PgConnection::connect_with(&options).await.map_err(|err| {
        Stop::new(
            "dial",
            format!("user={user} db={database}"),
            "verify-full session",
            sql_observed(&err, live),
            "none",
        )
    })
}

async fn reset_cluster(live: &Live, admin: &mut PgConnection) -> Result<(), Stop> {
    for database in [SOURCE_DB, TARGET_DB] {
        exec_ok(admin, &format!("DROP DATABASE IF EXISTS {database} WITH (FORCE)")).await?;
    }
    for role in [
        READER,
        RUNTIME,
        LOGIN_PARTY,
        LOGIN_INVOICE,
        ROLE_PARTY,
        ROLE_INVOICE,
        RUNTIME_ROLE,
        "oo_target_owner",
    ] {
        exec_hidden(admin, format!("DROP ROLE IF EXISTS {role}"), live).await?;
    }
    Ok(())
}

async fn create_roles(live: &Live, admin: &mut PgConnection) -> Result<(), Stop> {
    for (name, password) in [
        (READER, live.reader_password.as_str()),
        (RUNTIME, live.runtime_password.as_str()),
        (LOGIN_PARTY, live.party_password.as_str()),
        (LOGIN_INVOICE, live.invoice_password.as_str()),
    ] {
        if !plain(name) {
            return Err(Stop::new("role", name, "plain ident", "rejected", "none"));
        }
        let sql = format!(
            "CREATE ROLE {name} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION"
        );
        exec_hidden(admin, sql, live).await?;
    }
    Ok(())
}

async fn prove_reader_readonly(live: &Live) -> Result<(), Stop> {
    let mut reader = dial(live, READER, &live.reader_password, SOURCE_DB).await?;
    expect_42501(
        &mut reader,
        "INSERT INTO ent.party (party_id, active, credit_limit) VALUES (9, true, 0)",
        live,
    )
    .await?;
    let count = scalar_text(&mut reader, "SELECT count(*)::text FROM ent.party").await?;
    if count != "3" {
        return Err(Stop::new("reader_count", "ent.party", "3", count, "none"));
    }
    let invoice = scalar_text(&mut reader, "SELECT count(*)::text FROM ent.invoice").await?;
    if invoice != "3" {
        return Err(Stop::new("reader_count", "ent.invoice", "3", invoice, "none"));
    }
    Ok(())
}

async fn capture_and_prepare(
    live: &Live,
) -> Result<(String, String, oo_connectors::SnapshotRequest), Stop> {
    let config = source_config(live)?;
    let connector = PgConnector::new(config.clone());
    let mut session = connector.connect().await.map_err(|err| source_stop("inspect", &err))?;
    let report = session.inspect_access().await.map_err(|err| source_stop("inspect_access", &err))?;
    let report_json = serde_json::to_string_pretty(&report).map_err(|err| disk("report", &err))?;
    write_text(live, "access-report.json", &report_json)?;
    if !report.registration_allowed {
        return Err(Stop::new(
            "registration",
            SOURCE_ID,
            "registration_allowed true",
            report_json,
            "do not widen source grants; record block_reasons",
        ));
    }
    let catalog = session.catalog().await.map_err(|err| source_stop("catalog", &err))?;
    if catalog.protocol != "oo.source.pgsql/v1" {
        return Err(Stop::new(
            "catalog",
            "PgSession::catalog",
            "oo.source.pgsql/v1",
            catalog.protocol,
            "none",
        ));
    }
    let catalog_json = serde_json::to_string(&catalog).map_err(|err| disk("catalog", &err))?;
    drop(session);
    let deadline = Instant::now() + Duration::from_secs(90);
    let snapshot = connector
        .prepare_snapshot(PgTypePolicy::PRESERVE_ROW, 1, deadline)
        .await
        .map_err(|err| source_stop("prepare_snapshot", &err))?;
    let config_hex = hex_bytes(&snapshot.config_identity);
    Ok((catalog_json, config_hex, snapshot))
}

fn source_config(live: &Live) -> Result<PgSourceConfig, Stop> {
    let source_id = SourceId::new(SOURCE_ID).map_err(|err| source_stop("source_id", &err))?;
    let endpoint = PgEndpoint::new(&live.host, live.port, SOURCE_DB).map_err(|err| source_stop("endpoint", &err))?;
    let credentials = PgCredentials::new(READER, live.reader_password.clone()).map_err(|err| source_stop("credentials", &err))?;
    let secret = SecretRef::new(SECRET_REF).map_err(|err| source_stop("secret_ref", &err))?;
    let tls = PgTlsConfig::from_ca_pem(&live.ca).map_err(|err| source_stop("tls", &err))?;
    let budget = PgBudget::new(
        Duration::from_secs(20),
        Duration::from_secs(40),
        Duration::from_secs(15),
        10_000,
        1_048_576,
    )
    .map_err(|err| source_stop("budget", &err))?;
    let selection = TableSelection::new(vec![
        SelectedTable::new("ent", "party").map_err(|err| source_stop("selection", &err))?,
        SelectedTable::new("ent", "invoice").map_err(|err| source_stop("selection", &err))?,
    ])
    .map_err(|err| source_stop("selection", &err))?;
    PgSourceConfig::try_new(source_id, endpoint, credentials, secret, tls, budget, selection)
        .map_err(|err| source_stop("config", &err))
}

async fn import_rows(live: &Live, snapshot: oo_connectors::SnapshotRequest) -> Result<RunSummary, Stop> {
    let mut runtime = dial(live, RUNTIME, &live.runtime_password, TARGET_DB).await?;
    let summary = import_unpublished(
        &mut runtime,
        ImportRequest {
            shard: SHARD.to_string(),
            owner: OWNER.to_string(),
            snapshot,
            stream: StreamBudget::try_new(100, 1_048_576, 1_048_576, 2_097_152)
                .map_err(|err| source_stop("stream", &err))?,
            copy: CopyBudgets {
                max_value_bytes: 1_048_576,
                max_row_bytes: 4_194_304,
                max_chunk_bytes: 8_388_608,
                max_rows_per_chunk: 1024,
            },
        },
    )
    .await
    .map_err(|err| Stop::new("import_unpublished", SOURCE_ID, "AwaitingPublish", err.to_string(), "none"))?;
    if summary.state != RunState::AwaitingPublish {
        return Err(Stop::new(
            "import_unpublished",
            summary.run_id.clone(),
            "AwaitingPublish",
            "other state",
            "none",
        ));
    }
    Ok(summary)
}

async fn read_mappings(conn: &mut PgConnection) -> Result<Vec<MapRow>, Stop> {
    let rows = sqlx::query(
        "SELECT level, logical_namespace, logical_table, logical_column, physical_name, config_digest, run_generation \
         FROM _oo.physical_mappings WHERE source = $1 ORDER BY level, logical_namespace, logical_table, logical_column",
    )
    .bind(SOURCE_ID)
    .fetch_all(&mut *conn)
    .await
    .map_err(|err| Stop::new("mappings", SOURCE_ID, "rows", err.to_string(), "none"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(MapRow {
            level: row.try_get(0).map_err(|err| map_err(&err))?,
            namespace: row.try_get(1).map_err(|err| map_err(&err))?,
            table: row.try_get(2).map_err(|err| map_err(&err))?,
            column: row.try_get(3).map_err(|err| map_err(&err))?,
            physical: row.try_get(4).map_err(|err| map_err(&err))?,
            digest: row.try_get(5).map_err(|err| map_err(&err))?,
            generation: row.try_get(6).map_err(|err| map_err(&err))?,
        });
    }
    Ok(out)
}

async fn prove_seal(
    conn: &mut PgConnection,
    summary: &RunSummary,
    mappings: &[MapRow],
    config_hex: &str,
) -> Result<(), Stop> {
    if !uuid_text(&summary.run_id) {
        return Err(Stop::new("seal", "run_id", "uuid", "rejected", "none"));
    }
    let state = scalar_text(
        conn,
        &format!(
            "SELECT state FROM _oo.full_import_run WHERE run_id = '{}'::uuid",
            summary.run_id
        ),
    )
    .await?;
    if state != "awaiting_publish" {
        return Err(Stop::new("seal", summary.run_id.clone(), "awaiting_publish", state, "none"));
    }
    let publications = scalar_text(conn, "SELECT count(*)::text FROM pg_catalog.pg_publication").await?;
    if publications != "0" {
        return Err(Stop::new("publication", "pg_publication", "0", publications, "none"));
    }
    if summary.source_id != SOURCE_ID {
        return Err(Stop::new("source_id", SOURCE_ID, SOURCE_ID, summary.source_id.clone(), "none"));
    }
    if mappings.is_empty() {
        return Err(Stop::new("mappings", SOURCE_ID, "rows", "empty", "none"));
    }
    if mappings.iter().any(|row| row.digest != config_hex) {
        return Err(Stop::new(
            "config_digest",
            "physical_mappings",
            config_hex,
            mappings[0].digest.clone(),
            "none",
        ));
    }
    let generation = i64::try_from(summary.source_generation).unwrap_or(-1);
    if mappings.iter().any(|row| row.generation != generation) {
        return Err(Stop::new(
            "generation",
            "physical_mappings.run_generation",
            generation.to_string(),
            mappings[0].generation.to_string(),
            "none",
        ));
    }
    for table in &summary.tables {
        if table.rows != 3 {
            return Err(Stop::new("import_rows", &table.name, "3", table.rows.to_string(), "none"));
        }
    }
    Ok(())
}

fn write_drafts(live: &Live, passes: &drafts::DraftPass) -> Result<(), Stop> {
    write_text(live, "draft-unreviewed.json", &passes.unreviewed.draft)?;
    write_text(live, "curated.json", &passes.curated_input)?;
    write_text(live, "draft-curated.json", &passes.curated.draft)?;
    write_text(live, "reviews.json", &passes.reviews_input)?;
    write_text(live, "draft-reviewed.json", &passes.reviewed.draft)?;
    write_text(live, "wrapper.json", &passes.reviewed.wrapper)?;
    write_text(live, "evidence-envelope.json", &passes.reviewed.evidence_envelope)?;
    let snap = queue_snapshot(&passes.reviewed.draft)?;
    write_json(live, "review-queue.json", &snap)?;
    Ok(())
}

fn check_reviewed_queue(draft: &str) -> Result<(), Stop> {
    let value: Value = serde_json::from_str(draft).map_err(|err| disk("draft", &err))?;
    let publication = value.pointer("/provenance/publication").and_then(Value::as_str).unwrap_or("");
    if publication != "unavailable" {
        return Err(Stop::new("publication_field", "provenance.publication", "unavailable", publication, "none"));
    }
    let integration = value
        .pointer("/provenance/pg_catalog_integration")
        .and_then(Value::as_str)
        .unwrap_or("");
    if integration != "not_accepted" {
        return Err(Stop::new(
            "pg_catalog_integration",
            "provenance",
            "not_accepted",
            integration,
            "fixed snapshot stays not_accepted",
        ));
    }
    let queue = value.get("review_queue").and_then(Value::as_array).ok_or_else(|| {
        Stop::new("review_queue", "reviewed draft", "array", "missing", "none")
    })?;
    for item in queue {
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
        let state = item.get("state").and_then(Value::as_str).unwrap_or("");
        let publishable = item.get("publishable").and_then(Value::as_bool).unwrap_or(true);
        if publishable {
            return Err(Stop::new("publishable", kind, "false", "true", "none"));
        }
        if matches!(kind, "structural_column" | "structural_primary_key") && state != "accepted" {
            return Err(Stop::new("review_state", kind, "accepted", state, "none"));
        }
        if matches!(kind, "pii_unknown" | "semantic_unknown") && state != "pending" {
            return Err(Stop::new("review_state", kind, "pending", state, "none"));
        }
    }
    Ok(())
}

fn pack_notes(attempts: &[drafts::PackAttempt]) -> Value {
    Value::Array(
        attempts
            .iter()
            .map(|item| {
                json!({
                    "input_kind": item.input_kind,
                    "compiled": item.compiled,
                    "error_code": item.error_code,
                    "error_path": item.error_path,
                    "error_message": item.error_message,
                })
            })
            .collect(),
    )
}

struct InstalledRole {
    login: &'static str,
    password_slot: PasswordSlot,
    role: &'static str,
    namespace: String,
    table: &'static str,
    columns: Vec<String>,
    catalog: oo_gateway::PolicyCatalog,
}

enum PasswordSlot {
    Party,
    Invoice,
}

async fn install_acls(live: &Live, catalog_json: &str, mappings: &[MapRow]) -> Result<Vec<InstalledRole>, Stop> {
    let digest = &mappings[0].digest;
    let generation = mappings[0].generation;
    let catalog: Value = serde_json::from_str(catalog_json).map_err(|err| disk("catalog", &err))?;
    let context = PolicyContext {
        catalog: &catalog,
        mappings,
        digest,
        generation,
    };
    let party = policy_for(&context, "party", HIDDEN_PARTY, SLUG_PARTY, LOGIN_PARTY)?;
    let invoice = policy_for(
        &context,
        "invoice",
        HIDDEN_INVOICE,
        SLUG_INVOICE,
        LOGIN_INVOICE,
    )?;
    let compiled_party = compile_column_acl(&party).map_err(|err| {
        Stop::new("compile_column_acl", "party", "compiled", err.to_string(), "none")
    })?;
    let compiled_invoice = compile_column_acl(&invoice).map_err(|err| {
        Stop::new("compile_column_acl", "invoice", "compiled", err.to_string(), "none")
    })?;
    let (mut client, driver) = admin_client(live).await?;
    let install_party = install_column_acl(&mut client, &party).await.map_err(|err| {
        Stop::new("install_column_acl", "party", "installed", err.to_string(), "none")
    })?;
    let install_invoice = install_column_acl(&mut client, &invoice).await.map_err(|err| {
        Stop::new("install_column_acl", "invoice", "installed", err.to_string(), "none")
    })?;
    let party_effect = install_party.effect;
    let invoice_effect = install_invoice.effect;
    write_json(
        live,
        "column-acl.json",
        &json!({
            "bridge": "trusted administrative classification; ontology compiler left pii and classification unset",
            "party": {"effect": format!("{party_effect:?}"), "role": install_party.role, "physical_table": install_party.physical_table, "physical_columns": install_party.physical_columns},
            "invoice": {"effect": format!("{invoice_effect:?}"), "role": install_invoice.role, "physical_table": install_invoice.physical_table, "physical_columns": install_invoice.physical_columns},
        }),
    )?;
    let limits = CatalogLimits {
        max_tables: 4,
        max_columns: 64,
        max_functions: 4,
        max_operators: 64,
        large_table_row_threshold: 1_000_000,
        unknown_rows: UnknownRows::Zero,
    };
    let party_identity = PolicyIdentity {
        project: SOURCE_ID.to_string(),
        role: ROLE_PARTY.to_string(),
        policy_id: digest.clone(),
    };
    let invoice_identity = PolicyIdentity {
        project: SOURCE_ID.to_string(),
        role: ROLE_INVOICE.to_string(),
        policy_id: digest.clone(),
    };
    let party_catalog = load_policy_catalog(&mut client, &party_identity, &[compiled_party], &limits)
        .await
        .map_err(|err| Stop::new("load_policy_catalog", ROLE_PARTY, "catalog", err.to_string(), "none"))?;
    let invoice_catalog = load_policy_catalog(&mut client, &invoice_identity, &[compiled_invoice], &limits)
        .await
        .map_err(|err| Stop::new("load_policy_catalog", ROLE_INVOICE, "catalog", err.to_string(), "none"))?;
    drop(client);
    driver.abort();
    write_json(
        live,
        "policy-catalog-view.json",
        &json!({
            "party": catalog_view(&party_catalog),
            "invoice": catalog_view(&invoice_catalog),
        }),
    )?;
    Ok(vec![
        InstalledRole {
            login: LOGIN_PARTY,
            password_slot: PasswordSlot::Party,
            role: ROLE_PARTY,
            namespace: "ent".to_string(),
            table: "party",
            columns: PARTY_VISIBLE.iter().map(|item| (*item).to_string()).collect(),
            catalog: party_catalog,
        },
        InstalledRole {
            login: LOGIN_INVOICE,
            password_slot: PasswordSlot::Invoice,
            role: ROLE_INVOICE,
            namespace: "ent".to_string(),
            table: "invoice",
            columns: INVOICE_VISIBLE.iter().map(|item| (*item).to_string()).collect(),
            catalog: invoice_catalog,
        },
    ])
}

struct PolicyContext<'a> {
    catalog: &'a Value,
    mappings: &'a [MapRow],
    digest: &'a str,
    generation: i64,
}

fn policy_for(
    context: &PolicyContext<'_>,
    table: &str,
    hidden: &str,
    slug: &str,
    login: &str,
) -> Result<ColumnAclPolicy, Stop> {
    let catalog = context.catalog;
    let mappings = context.mappings;
    let digest = context.digest;
    let generation = context.generation;
    let spec = catalog
        .get("tables")
        .and_then(Value::as_array)
        .and_then(|tables| tables.iter().find(|item| item.get("name").and_then(Value::as_str) == Some(table)))
        .ok_or_else(|| Stop::new("policy_table", table, "catalog table", "missing", "none"))?;
    let namespace = spec.get("schema_ident").and_then(Value::as_str).unwrap_or("");
    let table_ident = spec.get("name_ident").and_then(Value::as_str).unwrap_or("");
    if namespace != "ent" || table_ident != table {
        return Err(Stop::new(
            "policy_ident",
            table,
            "ent.{table}",
            format!("{namespace}.{table_ident}"),
            "none",
        ));
    }
    if !mappings.iter().any(|row| row.level == "t" && row.namespace == namespace && row.table == table_ident) {
        return Err(Stop::new("policy_mapping", table, "table mapping", "absent", "none"));
    }
    let mut columns = Vec::new();
    let specs = spec.get("columns").and_then(Value::as_array).ok_or_else(|| {
        Stop::new("policy_columns", table, "columns", "missing", "none")
    })?;
    for column in specs {
        let name = column.get("name").and_then(Value::as_str).unwrap_or("");
        if name == hidden {
            continue;
        }
        let type_name = column.get("type_name").and_then(Value::as_str).unwrap_or("");
        let semantic = semantic_type(type_name).ok_or_else(|| {
            Stop::new("policy_type", name, "bool|int8|text", type_name, "executor type set")
        })?;
        columns.push(ColumnSpec {
            name: name.to_string(),
            status: ClassStatus::Confirmed,
            semantic_type: semantic.to_string(),
            pii: PiiClass::None,
        });
    }
    Ok(ColumnAclPolicy {
        project: SOURCE_ID.to_string(),
        config_digest: digest.to_string(),
        generation,
        role: slug.to_string(),
        login: login.to_string(),
        table: LogicalTable {
            namespace: namespace.to_string(),
            name: table_ident.to_string(),
            columns,
        },
        row_filter: None,
    })
}

fn catalog_view(catalog: &oo_gateway::PolicyCatalog) -> Value {
    json!({
        "role": catalog.identity.role,
        "project": catalog.identity.project,
        "policy_id": catalog.identity.policy_id,
        "large_table_row_threshold": catalog.large_table_row_threshold,
        "relations": catalog.relations.iter().map(|relation| json!({
            "schema": relation.schema,
            "name": relation.name,
            "physical_schema": relation.physical_schema,
            "physical_name": relation.physical_name,
            "pending_unclassified": relation.pending_unclassified,
            "estimated_rows": relation.estimated_rows,
            "columns": relation.columns.iter().map(|column| json!({
                "name": column.name,
                "physical_name": column.physical_name,
                "type_oid": column.type_oid,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

async fn run_role_queries(live: &Live, roles: &[InstalledRole]) -> Result<Value, Stop> {
    let mut notes = Vec::new();
    for role in roles {
        let password = match role.password_slot {
            PasswordSlot::Party => live.party_password.as_str(),
            PasswordSlot::Invoice => live.invoice_password.as_str(),
        };
        let mut session = bind_login(live, role.login, password).await?;
        let list = role.columns.join(", ");
        let selected = format!("SELECT {list} FROM {}.{}", role.namespace, role.table);
        let count_sql = format!("SELECT count(*) FROM {}.{}", role.namespace, role.table);
        let star_sql = format!("SELECT * FROM {}.{}", role.namespace, role.table);
        let hidden = if role.table == "party" { HIDDEN_PARTY } else { HIDDEN_INVOICE };
        let hidden_sql = format!("SELECT {hidden} FROM {}.{}", role.namespace, role.table);
        let write_sql = if role.table == "party" {
            "INSERT INTO ent.party (party_id, active, credit_limit) VALUES (9, true, 0)"
        } else {
            "INSERT INTO ent.invoice (invoice_id, party_id, posted) VALUES (9, 1, false)"
        };
        let expected = if role.table == "party" {
            project_visible(PARTY_ALL, PARTY_VISIBLE, PARTY_ROWS)
        } else {
            project_visible(INVOICE_ALL, INVOICE_VISIBLE, INVOICE_ROWS)
        };
        let got = positive_query(live, &mut session, role, &selected).await?;
        expect_cells(&selected, &got, &expected)?;
        notes.push(json!({"sql": selected, "rows": got.len(), "class": "rows"}));
        let counted = positive_query(live, &mut session, role, &count_sql).await?;
        expect_cells(&count_sql, &counted, &[vec![Expect::Int("3")]])?;
        notes.push(json!({"sql": count_sql, "class": "count"}));
        let starred = positive_query(live, &mut session, role, &star_sql).await?;
        expect_cells(&star_sql, &starred, &expected)?;
        notes.push(json!({"sql": star_sql, "rows": starred.len(), "class": "star"}));
        let hidden_end = refused_query(live, &mut session, role, &hidden_sql).await?;
        notes.push(hidden_end);
        let write_end = refused_query(live, &mut session, role, write_sql).await?;
        notes.push(write_end);
        session.close();
    }
    Ok(Value::Array(notes))
}

async fn positive_query(
    live: &Live,
    session: &mut DedicatedSession,
    role: &InstalledRole,
    sql: &str,
) -> Result<Vec<Vec<Cell>>, Stop> {
    let before_admit = audit_count(live, "phase = 'admit'").await?;
    let before_complete = audit_count(live, "phase LIKE 'complete:%'").await?;
    let identity = PolicyIdentity {
        project: SOURCE_ID.to_string(),
        role: role.role.to_string(),
        policy_id: role.catalog.identity.policy_id.clone(),
    };
    let audit = open_audit(live).await?;
    let request = SelectRequest {
        identity: &identity,
        catalog: &role.catalog,
        original_sql: sql,
        role: role.role,
        max_rows: 100,
        max_result_bytes: 65_536,
        wall_clock: Duration::from_secs(10),
    };
    let mut stream = session.execute_trusted_select(request, audit).await.map_err(|err| {
        Stop::new("execute_trusted_select", sql, "rows", err.to_string(), "none")
    })?;
    let after_admit = audit_count(live, "phase = 'admit'").await?;
    if after_admit != before_admit + 1 {
        return Err(Stop::new(
            "audit_admit",
            sql,
            "one admit row before the stream is read",
            format!("{before_admit} -> {after_admit}"),
            "audit callback must commit before the query rows are read",
        ));
    }
    let mut rows = Vec::new();
    loop {
        match stream.next_row().await {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => break,
            Err(err) => {
                return Err(Stop::new("next_row", sql, "row", err.to_string(), "none"));
            }
        }
    }
    let summary = stream.summary();
    drop(stream);
    session.wait_idle().await.map_err(|err| Stop::new("wait_idle", sql, "idle", err.to_string(), "none"))?;
    let after_complete = audit_count(live, "phase LIKE 'complete:%'").await?;
    if after_complete != before_complete + 1 {
        return Err(Stop::new(
            "audit_complete",
            sql,
            "one complete row after drain",
            format!("{before_complete} -> {after_complete}"),
            "audit complete must durably record",
        ));
    }
    let status = summary.as_ref().map(|item| item.status().as_str()).unwrap_or("missing");
    if status != "succeeded" {
        return Err(Stop::new("query_summary", sql, "succeeded", status, "none"));
    }
    if summary.as_ref().is_some_and(|item| !item.end_audit_ok()) {
        return Err(Stop::new("audit_complete", sql, "end_audit_ok", "false", "none"));
    }
    Ok(rows)
}

async fn refused_query(
    live: &Live,
    session: &mut DedicatedSession,
    role: &InstalledRole,
    sql: &str,
) -> Result<Value, Stop> {
    let before = audit_count(live, "true").await?;
    let identity = PolicyIdentity {
        project: SOURCE_ID.to_string(),
        role: role.role.to_string(),
        policy_id: role.catalog.identity.policy_id.clone(),
    };
    let audit = open_audit(live).await?;
    let request = SelectRequest {
        identity: &identity,
        catalog: &role.catalog,
        original_sql: sql,
        role: role.role,
        max_rows: 100,
        max_result_bytes: 65_536,
        wall_clock: Duration::from_secs(10),
    };
    let refused = session.execute_trusted_select(request, audit).await;
    let after = audit_count(live, "true").await?;
    match refused {
        Err(ExecError::Guard(error)) => {
            if after != before {
                return Err(Stop::new(
                    "guard_audit",
                    sql,
                    "no audit row on guard refusal",
                    format!("{before} -> {after}"),
                    "none",
                ));
            }
            Ok(json!({"sql": sql, "class": "guard_refusal", "detail": error.to_string()}))
        }
        Err(ExecError::Database { sqlstate }) => Ok(json!({
            "sql": sql,
            "class": "executor_database",
            "sqlstate": sqlstate,
            "note": "database sqlstate from the executor, distinct from a guard refusal",
        })),
        Err(other) => Err(Stop::new("refused_query", sql, "guard refusal", other.to_string(), "none")),
        Ok(_) => Err(Stop::new("refused_query", sql, "guard refusal", "rows returned", "none")),
    }
}

async fn prove_direct_denies(live: &Live, mappings: &[MapRow]) -> Result<(), Stop> {
    let mut reader = dial(live, READER, &live.reader_password, SOURCE_DB).await?;
    expect_42501(
        &mut reader,
        "INSERT INTO ent.party (party_id, active, credit_limit) VALUES (8, true, 0)",
        live,
    )
    .await?;
    let count = scalar_text(&mut reader, "SELECT count(*)::text FROM ent.party").await?;
    if count != "3" {
        return Err(Stop::new("reader_insert", "ent.party", "3", count, "none"));
    }
    deny_hidden(live, mappings, LOGIN_PARTY, &live.party_password, ROLE_PARTY, "party", HIDDEN_PARTY).await?;
    deny_hidden(
        live,
        mappings,
        LOGIN_INVOICE,
        &live.invoice_password,
        ROLE_INVOICE,
        "invoice",
        HIDDEN_INVOICE,
    )
    .await?;
    Ok(())
}

async fn deny_hidden(
    live: &Live,
    mappings: &[MapRow],
    login: &str,
    password: &str,
    role: &str,
    table: &str,
    column: &str,
) -> Result<(), Stop> {
    let physical_table = mappings
        .iter()
        .find(|row| row.level == "t" && row.table == table)
        .ok_or_else(|| Stop::new("deny", table, "physical table", "absent", "none"))?;
    let physical_column = mappings
        .iter()
        .find(|row| row.level == "c" && row.table == table && row.column == column)
        .ok_or_else(|| Stop::new("deny", column, "physical column", "absent", "none"))?;
    let table_sql = quote_physical(&physical_table.physical)?;
    let column_sql = quote_physical(&physical_column.physical)?;
    let mut conn = dial(live, login, password, TARGET_DB).await?;
    exec_ok(&mut conn, &format!("SET ROLE {role}")).await?;
    expect_42501(&mut conn, &format!("SELECT {column_sql} FROM \"_oo\".{table_sql}"), live).await?;
    expect_42501(
        &mut conn,
        &format!("INSERT INTO \"_oo\".{table_sql} (\"_oo_pos\") VALUES (1)"),
        live,
    )
    .await?;
    Ok(())
}

async fn compare_stores(
    live: &Live,
    target: &mut PgConnection,
    catalog_json: &str,
    mappings: &[MapRow],
) -> Result<(), Stop> {
    let mut reader = dial(live, READER, &live.reader_password, SOURCE_DB).await?;
    let party_source = fetch_text(
        &mut reader,
        "SELECT party_id::text, display_name, active::text, credit_limit::text, internal_note FROM ent.party ORDER BY party_id",
    )
    .await?;
    let invoice_source = fetch_text(
        &mut reader,
        "SELECT invoice_id::text, party_id::text, memo, posted::text, amount::text, clerk_note FROM ent.invoice ORDER BY invoice_id",
    )
    .await?;
    expect_text_rows("source.party", &party_source, PARTY_ALL, PARTY_ROWS)?;
    expect_text_rows("source.invoice", &invoice_source, INVOICE_ALL, INVOICE_ROWS)?;
    compare_shadow(target, mappings, "party", PARTY_ALL, PARTY_ROWS).await?;
    compare_shadow(target, mappings, "invoice", INVOICE_ALL, INVOICE_ROWS).await?;
    let comment_body = catalog_json.contains("交易方主数据") || catalog_json.contains("可空中文名称");
    write_json(
        live,
        "oracle.json",
        &json!({
            "oracle_sha256": oracle_sha256(),
            "oracle_encoding": "table name, then tab-separated cells, null as \\\\N, bool as true/false",
            "catalog_contains_comment_body": comment_body,
            "source_party": party_source,
            "source_invoice": invoice_source,
        }),
    )?;
    Ok(())
}

async fn compare_shadow(
    conn: &mut PgConnection,
    mappings: &[MapRow],
    table: &str,
    columns: &[&str],
    rows: &[&[Expect]],
) -> Result<(), Stop> {
    let physical = mappings
        .iter()
        .find(|row| row.level == "t" && row.table == table)
        .ok_or_else(|| Stop::new("shadow", table, "physical", "absent", "none"))?;
    let mut select_list = Vec::new();
    for column in columns {
        let mapped = mappings
            .iter()
            .find(|row| row.level == "c" && row.table == table && row.column == *column)
            .ok_or_else(|| Stop::new("shadow", *column, "physical column", "absent", "none"))?;
        let quoted = quote_physical(&mapped.physical)?;
        select_list.push(format!("{quoted}::text"));
    }
    let columns_sql = select_list.join(", ");
    let table_sql = quote_physical(&physical.physical)?;
    let sql = format!("SELECT {columns_sql} FROM \"_oo\".{table_sql} ORDER BY \"_oo_pos\"");
    let got = fetch_text(conn, &sql).await?;
    expect_text_rows(&format!("shadow.{table}"), &got, columns, rows)?;
    Ok(())
}

fn project_visible(all: &[&str], visible: &[&str], rows: &[&[Expect]]) -> Vec<Vec<Expect>> {
    let indexes: Vec<usize> = visible
        .iter()
        .filter_map(|name| all.iter().position(|item| item == name))
        .collect();
    rows.iter()
        .map(|row| indexes.iter().map(|index| row[*index]).collect())
        .collect()
}

fn expect_cells(sql: &str, got: &[Vec<Cell>], expected: &[Vec<Expect>]) -> Result<(), Stop> {
    let mut left: Vec<Vec<String>> = got.iter().map(|row| row.iter().map(cell_key).collect()).collect();
    let mut right: Vec<Vec<String>> = expected
        .iter()
        .map(|row| row.iter().copied().map(expect_key).collect())
        .collect();
    left.sort();
    right.sort();
    if left != right {
        return Err(Stop::new(
            "oracle",
            sql,
            format!("{right:?}"),
            format!("{left:?}"),
            "none",
        ));
    }
    Ok(())
}

fn expect_text_rows(step: &str, got: &[Vec<Option<String>>], columns: &[&str], expected: &[&[Expect]]) -> Result<(), Stop> {
    let mut left: Vec<Vec<String>> = got
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(index, value)| norm_db(columns.get(index).copied().unwrap_or(""), value.clone()))
                .collect()
        })
        .collect();
    let mut right: Vec<Vec<String>> = expected
        .iter()
        .map(|row| row.iter().copied().map(expect_key).collect())
        .collect();
    left.sort();
    right.sort();
    if left != right {
        return Err(Stop::new("store_oracle", step, format!("{right:?}"), format!("{left:?}"), "none"));
    }
    Ok(())
}

fn norm_db(column: &str, value: Option<String>) -> String {
    match value {
        None => "null".to_string(),
        Some(text) if matches!(column, "active" | "posted") && text == "t" => "true".to_string(),
        Some(text) if matches!(column, "active" | "posted") && text == "f" => "false".to_string(),
        Some(text) => text,
    }
}

fn cell_key(cell: &Cell) -> String {
    match cell {
        Cell::Null => "null".to_string(),
        Cell::Bool(true) => "true".to_string(),
        Cell::Bool(false) => "false".to_string(),
        Cell::Int(value) | Cell::Text(value) => value.clone(),
    }
}

fn expect_key(value: Expect) -> String {
    match value {
        Expect::Null => "null".to_string(),
        Expect::Bool(true) => "true".to_string(),
        Expect::Bool(false) => "false".to_string(),
        Expect::Int(value) | Expect::Text(value) => value.to_string(),
    }
}

fn oracle_sha256() -> String {
    let mut lines = Vec::new();
    lines.push("party".to_string());
    for row in PARTY_ROWS {
        lines.push(row.iter().copied().map(oracle_cell).collect::<Vec<_>>().join("\t"));
    }
    lines.push("invoice".to_string());
    for row in INVOICE_ROWS {
        lines.push(row.iter().copied().map(oracle_cell).collect::<Vec<_>>().join("\t"));
    }
    let digest = Sha256::digest(lines.join("\n").as_bytes());
    hex_bytes(&digest)
}

fn oracle_cell(value: Expect) -> String {
    match value {
        Expect::Null => "\\N".to_string(),
        other => expect_key(other),
    }
}

async fn bind_login(live: &Live, login: &str, password: &str) -> Result<DedicatedSession, Stop> {
    let tls = rustls_connector(&live.ca).map_err(|err| Stop::new("rustls", login, "connector", err.to_string(), "none"))?;
    let (client, connection) = connect_require_tls(
        &TlsEndpoint {
            host: live.host.clone(),
            port: live.port,
            database: TARGET_DB.to_string(),
            user: login.to_string(),
        },
        password,
        &live.ca,
    )
    .await
    .map_err(|err| Stop::new("login_tls", login, "tls session", err.to_string(), "none"))?;
    DedicatedSession::bind(client, connection, tls)
        .await
        .map_err(|err| Stop::new("bind", login, "dedicated session", err.to_string(), "none"))
}

async fn admin_client(live: &Live) -> Result<(Client, JoinHandle<()>), Stop> {
    let (client, connection) = connect_require_tls(
        &TlsEndpoint {
            host: live.host.clone(),
            port: live.port,
            database: TARGET_DB.to_string(),
            user: live.admin_user.clone(),
        },
        &live.admin_password,
        &live.ca,
    )
    .await
    .map_err(|err| Stop::new("admin_tls", "target", "tls session", err.to_string(), "none"))?;
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok((client, driver))
}

async fn open_audit(live: &Live) -> Result<DbAudit, Stop> {
    let (client, driver) = admin_client(live).await?;
    Ok(DbAudit::new(client, driver))
}

async fn audit_count(live: &Live, predicate: &str) -> Result<i64, Stop> {
    if !predicate.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '=' | '\'' | ':' | '%' | '_')) {
        return Err(Stop::new("audit_count", predicate, "static predicate", "rejected", "none"));
    }
    let mut admin = dial(live, &live.admin_user, &live.admin_password, TARGET_DB).await?;
    let sql = format!("SELECT count(*)::int8 FROM public.a203_audit WHERE {predicate}");
    let value: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(&mut admin)
        .await
        .map_err(|err| Stop::new("audit_count", predicate, "count", err.to_string(), "none"))?;
    Ok(value)
}

async fn fetch_text(conn: &mut PgConnection, sql: &str) -> Result<Vec<Vec<Option<String>>>, Stop> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .map_err(|err| Stop::new("fetch", sql, "rows", err.to_string(), "none"))?;
    let mut out = Vec::new();
    for row in rows {
        let mut cells = Vec::new();
        let mut index = 0;
        loop {
            match row.try_get::<Option<String>, _>(index) {
                Ok(value) => cells.push(value),
                Err(sqlx::Error::ColumnIndexOutOfBounds { .. }) => break,
                Err(err) => return Err(Stop::new("fetch", sql, "cell", err.to_string(), "none")),
            }
            index += 1;
        }
        out.push(cells);
    }
    Ok(out)
}

async fn scalar_text(conn: &mut PgConnection, sql: &str) -> Result<String, Stop> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| Stop::new("scalar", sql, "text", err.to_string(), "none"))
}

async fn exec_ok(conn: &mut PgConnection, sql: &str) -> Result<(), Stop> {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(|err| Stop::new("sql", "statement", "ok", err.to_string(), "none"))
}

async fn exec_hidden(conn: &mut PgConnection, sql: String, live: &Live) -> Result<(), Stop> {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(|err| Stop::new("sql", "redacted role statement", "ok", sql_observed(&err, live), "none"))
}

async fn expect_42501(conn: &mut PgConnection, sql: &str, live: &Live) -> Result<(), Stop> {
    match sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string())).execute(&mut *conn).await {
        Ok(_) => Err(Stop::new("42501", sql, "42501", "statement succeeded", "none")),
        Err(err) => {
            let state = err
                .as_database_error()
                .and_then(|db| db.code().map(|code| code.to_string()))
                .unwrap_or_default();
            if state == "42501" {
                Ok(())
            } else {
                let observed = sql_observed(&err, live);
                Err(Stop::new(
                    "42501",
                    sql,
                    "42501",
                    format!("{state} {observed}"),
                    "none",
                ))
            }
        }
    }
}

fn uuid_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => *byte == b'-',
        _ => byte.is_ascii_hexdigit(),
    })
}

fn quote_physical(name: &str) -> Result<String, Stop> {
    let rest = name
        .strip_prefix("pt_")
        .or_else(|| name.strip_prefix("pc_"))
        .or_else(|| name.strip_prefix("pn_"))
        .ok_or_else(|| Stop::new("quote", name, "pt_/pc_/pn_", "rejected", "none"))?;
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Stop::new("quote", name, "hex suffix", "rejected", "none"));
    }
    Ok(format!("\"{name}\""))
}

fn plain(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn sql_observed(err: &sqlx::Error, live: &Live) -> String {
    let mut text = err.to_string();
    for secret in [
        live.admin_password.as_str(),
        live.reader_password.as_str(),
        live.runtime_password.as_str(),
        live.party_password.as_str(),
        live.invoice_password.as_str(),
    ] {
        text = text.replace(secret, "[redacted]");
    }
    if text.len() > 400 {
        text.truncate(400);
    }
    text
}

fn source_stop(step: &'static str, err: &oo_connectors::SourceError) -> Stop {
    Stop::new(step, SOURCE_ID, "ok", err.to_string(), "none")
}

fn disk(step: &'static str, err: &impl std::fmt::Display) -> Stop {
    Stop::new(step, "evidence file", "written", err.to_string(), "none")
}

fn map_err(err: &sqlx::Error) -> Stop {
    Stop::new("mapping_row", "physical_mappings", "column", err.to_string(), "none")
}

fn write_text(live: &Live, name: &str, text: &str) -> Result<(), Stop> {
    fs::write(live.evidence.join("live").join(name), text)
        .map_err(|err| Stop::new("write", name, "written", err.to_string(), "none"))
}

fn write_json(live: &Live, name: &str, value: &Value) -> Result<(), Stop> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|err| Stop::new("write_json", name, "json", err.to_string(), "none"))?;
    write_text(live, name, &text)
}
